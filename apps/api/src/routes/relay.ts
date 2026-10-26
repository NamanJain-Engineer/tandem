// The phone relay (F-BE-11, ARCHITECTURE §14.8): `/v1/relay/:pairing_id`.
// It pairs at most one desktop and one phone socket and forwards binary
// frames unchanged. Frames are end-to-end encrypted between the two ends
// (§13.3); this module can't read them and imports no crypto (AC2, enforced
// by ESLint `no-restricted-imports` and a test). It logs connection metadata
// only, never frame bytes.
import type { IncomingMessage } from "node:http";
import type { Duplex } from "node:stream";
import { WebSocketServer, type RawData, type WebSocket } from "ws";
import { logger } from "../lib/logger";
import type { RelayControl } from "../services/relayControl";

/** The largest frame the relay forwards; larger closes the socket with 1009. */
export const RELAY_MAX_FRAME = 64 * 1024;
/** Frames a second per pairing (both directions together), with an equal burst. */
export const RELAY_FRAMES_PER_SECOND = 50;
const PATH = /^\/v1\/relay\/(pr_[A-Za-z0-9_-]{4,64})$/;
const PING_MS = 30_000;
/** How long a pairing's drops are remembered: far longer than any check. */
const DROP_MEMORY_MS = 10 * 60_000;

export type RelayRole = "desktop" | "phone";
export type RelayPeer = { role: RelayRole; protocol?: string };
export type RelayAuthenticate = (
  pairingId: string,
  req: { bearer?: string; protocols: string[] },
) => Promise<RelayPeer | null>;

type Upgradable = { on(event: "upgrade", listener: (req: IncomingMessage, socket: Duplex, head: Buffer) => void): unknown };

type Pair = {
  desktop?: WebSocket;
  phone?: WebSocket;
  tokens: number;
  refilledAt: number;
  noticeAt: number;
};

export class Relay {
  private readonly wss: WebSocketServer;
  private readonly pairs = new Map<string, Pair>();
  private readonly chosen = new WeakMap<IncomingMessage, string | false>();
  private readonly alive = new WeakSet<WebSocket>();
  /** Each pairing's drops, so a socket checked before a drop can't join after it. */
  private readonly drops = new Map<string, { count: number; at: number }>();
  private readonly stopDrop: () => void;
  private readonly ping: NodeJS.Timeout;

  constructor(
    private readonly opts: { authenticate: RelayAuthenticate; control: RelayControl; now: () => number },
  ) {
    this.wss = new WebSocketServer({
      noServer: true,
      maxPayload: RELAY_MAX_FRAME,
      perMessageDeflate: false,
      handleProtocols: (_offered, req) => this.chosen.get(req) ?? false,
    });
    this.stopDrop = opts.control.onDrop((pairingId) => {
      this.noteDrop(pairingId);
      this.drop(pairingId, 1008, "revoked");
    });
    this.ping = setInterval(() => this.heartbeat(), PING_MS);
    this.ping.unref();
  }

  /** Serves upgrades on `/v1/relay/:pairing_id` from this HTTP server. */
  attach(server: Upgradable): void {
    server.on("upgrade", (req, socket, head) => {
      void this.upgrade(req, socket, head);
    });
  }

  close(): void {
    this.stopDrop();
    clearInterval(this.ping);
    for (const id of [...this.pairs.keys()]) this.drop(id, 1001, "going away");
    this.wss.close();
  }

  private async upgrade(req: IncomingMessage, socket: Duplex, head: Buffer): Promise<void> {
    const pathname = (req.url ?? "").split("?")[0] ?? "";
    const match = PATH.exec(pathname);
    if (!match?.[1]) {
      socket.end("HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n");
      return;
    }
    const pairingId = match[1];
    // A pairing deleted while its token is checked drops no socket, since
    // this one hasn't joined yet; it must not join after the drop either.
    const drops = this.dropsOf(pairingId);
    const header = req.headers.authorization;
    const bearer = header?.startsWith("Bearer ") ? header.slice(7).trim() : undefined;
    const protocols = (req.headers["sec-websocket-protocol"] ?? "")
      .split(",")
      .map((p) => p.trim())
      .filter(Boolean);
    let peer: RelayPeer | null = null;
    try {
      peer = await this.opts.authenticate(pairingId, { bearer, protocols });
    } catch (err) {
      logger.warn({ pairingId, err: err instanceof Error ? err.name : "error" }, "relay auth failed");
    }
    // Echo the offered subprotocol even when refusing, so a browser sees the
    // 1008 close instead of a bare handshake failure.
    this.chosen.set(req, peer?.protocol ?? protocols[0] ?? false);
    this.wss.handleUpgrade(req, socket, head, (ws) => {
      if (!peer) {
        logger.info({ pairingId }, "relay refused");
        ws.close(1008, "unauthorized");
        return;
      }
      if (this.dropsOf(pairingId) !== drops) {
        logger.info({ pairingId }, "relay refused");
        ws.close(1008, "revoked");
        return;
      }
      this.join(pairingId, peer.role, ws);
    });
  }

  private noteDrop(pairingId: string): void {
    const now = this.opts.now();
    for (const [id, drop] of this.drops) if (now - drop.at > DROP_MEMORY_MS) this.drops.delete(id);
    this.drops.set(pairingId, { count: this.dropsOf(pairingId) + 1, at: now });
  }

  private dropsOf(pairingId: string): number {
    return this.drops.get(pairingId)?.count ?? 0;
  }

  private join(pairingId: string, role: RelayRole, ws: WebSocket): void {
    let pair = this.pairs.get(pairingId);
    if (!pair) {
      pair = { tokens: RELAY_FRAMES_PER_SECOND, refilledAt: this.opts.now(), noticeAt: 0 };
      this.pairs.set(pairingId, pair);
    }
    const previous = pair[role];
    pair[role] = ws;
    if (previous) previous.close(4000, "replaced");
    this.alive.add(ws);
    logger.info({ pairingId, role }, "relay connect");

    const other: RelayRole = role === "desktop" ? "phone" : "desktop";
    const peerSocket = pair[other];
    sendText(ws, { type: "peer", up: Boolean(peerSocket) });
    if (peerSocket) sendText(peerSocket, { type: "peer", up: true });

    ws.on("pong", () => this.alive.add(ws));
    ws.on("message", (data, isBinary) => this.forward(pairingId, role, ws, data, isBinary));
    ws.on("error", () => undefined);
    ws.on("close", (code) => {
      const current = this.pairs.get(pairingId);
      if (!current || current[role] !== ws) return;
      current[role] = undefined;
      logger.info({ pairingId, role, code }, "relay disconnect");
      const remaining = current[other];
      if (remaining) sendText(remaining, { type: "peer", up: false });
      else this.pairs.delete(pairingId);
    });
  }

  private forward(pairingId: string, role: RelayRole, ws: WebSocket, data: RawData, isBinary: boolean): void {
    if (!isBinary) {
      ws.close(1003, "binary frames only");
      return;
    }
    const pair = this.pairs.get(pairingId);
    if (!pair || pair[role] !== ws) return;
    const now = this.opts.now();
    pair.tokens = Math.min(
      RELAY_FRAMES_PER_SECOND,
      pair.tokens + ((now - pair.refilledAt) / 1000) * RELAY_FRAMES_PER_SECOND,
    );
    pair.refilledAt = now;
    if (pair.tokens < 1) {
      if (now - pair.noticeAt >= 1000) {
        pair.noticeAt = now;
        sendText(ws, { type: "rate_limited" });
      }
      return;
    }
    pair.tokens -= 1;
    const target = pair[role === "desktop" ? "phone" : "desktop"];
    if (target && target.readyState === target.OPEN) target.send(data, { binary: true });
  }

  private drop(pairingId: string, code: number, reason: string): void {
    const pair = this.pairs.get(pairingId);
    if (!pair) return;
    this.pairs.delete(pairingId);
    for (const ws of [pair.desktop, pair.phone]) ws?.close(code, reason);
    logger.info({ pairingId, reason }, "relay dropped");
  }

  private heartbeat(): void {
    for (const pair of this.pairs.values()) {
      for (const ws of [pair.desktop, pair.phone]) {
        if (!ws) continue;
        if (!this.alive.has(ws)) {
          ws.terminate();
          continue;
        }
        this.alive.delete(ws);
        ws.ping();
      }
    }
  }
}

function sendText(ws: WebSocket, message: { type: "peer"; up: boolean } | { type: "rate_limited" }): void {
  if (ws.readyState === ws.OPEN) ws.send(JSON.stringify(message));
}
