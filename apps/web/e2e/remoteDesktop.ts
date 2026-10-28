// A scripted desktop for the phone companion's web E2E (F-RM-03): it
// registers a pairing with the API, joins the relay with its bearer and
// answers the PWA's commands with fixed data, using the protocol module the
// PWA uses (the desktop's Rust side is pinned to the same bytes by vectors).
import WebSocket from "ws";
import {
  b64url,
  decodeHello,
  decodeJson,
  encodeJson,
  encodePairingLink,
  equalBytes,
  generateKeyPair,
  helloTag,
  newSession,
  openNext,
  rootSecret,
  sealNext,
  type SessionState,
} from "@tandem/remote/protocol";
import { API } from "./helpers";

type Cmd = { t: string; id?: number; cmd?: string; args?: Record<string, unknown>; device?: string };

export class FakeDesktop {
  readonly pid = `pr_webe2e${Date.now().toString(36)}`;
  readonly identity = generateKeyPair();
  readonly s = crypto.getRandomValues(new Uint8Array(32));
  readonly received: Cmd[] = [];
  grants = { chat: true, terminal: false, approvals: true, routines: false };
  private session: SessionState | null = null;
  private ws: WebSocket | null = null;
  private approvals = [
    { id: "ap_web1", summary: "Run `pnpm deploy`", detail: "pnpm deploy", source: "Thread", queued: false, deskOnly: false, createdAt: 1 },
  ];

  constructor(private readonly bearer: string) {}

  /** Registers the pairing, joins the relay and returns the QR's link. */
  async start(web: string): Promise<string> {
    const res = await fetch(`${API}/v1/pairings`, {
      method: "POST",
      headers: { authorization: `Bearer ${this.bearer}`, "content-type": "application/json" },
      body: JSON.stringify({ pairing_id: this.pid, device_label: "Work laptop" }),
    });
    if (!res.ok) throw new Error(`pairing: ${res.status} ${await res.text()}`);
    const ticket = (await res.json()) as { pairing_token: string; relay_url: string };
    const ws = new WebSocket(ticket.relay_url, { headers: { authorization: `Bearer ${this.bearer}` } });
    this.ws = ws;
    ws.on("message", (data, isBinary) => {
      if (isBinary) this.frame(new Uint8Array(data as Buffer));
    });
    await new Promise<void>((resolve, reject) => {
      ws.once("open", () => resolve());
      ws.once("error", reject);
    });
    return encodePairingLink(`${web}/remote`, {
      v: 1,
      pid: this.pid,
      dpk: b64url(this.identity.public),
      s: b64url(this.s),
      relay: ticket.relay_url,
      ptok: ticket.pairing_token,
    });
  }

  send(message: unknown): void {
    if (!this.session || !this.ws) return;
    const sealed = sealNext(this.session, encodeJson(message));
    this.session = sealed.state;
    this.ws.send(sealed.frame);
  }

  close(): void {
    this.ws?.close();
  }

  private frame(bytes: Uint8Array): void {
    if (bytes[0] === 0x01) {
      const { phonePub, tag } = decodeHello(bytes);
      if (!equalBytes(helloTag(this.s, this.pid, phonePub), tag)) return;
      const root = rootSecret(this.s, this.pid, this.identity.public, phonePub, this.identity.secret, phonePub);
      this.session = newSession(this.pid, "desktop", root);
      return;
    }
    if (!this.session) return;
    const opened = openNext(this.session, bytes);
    this.session = opened.state;
    const msg = decodeJson<Cmd>(opened.plaintext);
    this.received.push(msg);
    if (msg.t === "hi") this.send({ t: "welcome", desktop: "Work laptop", grants: this.grants });
    if (msg.t === "cmd" && msg.id !== undefined) this.send({ t: "res", id: msg.id, ...this.answer(msg) });
  }

  private answer(msg: Cmd): { ok: boolean; data?: unknown; error?: { code: string; message: string } } {
    switch (msg.cmd) {
      case "view.snapshot":
        return {
          ok: true,
          data: {
            desktop: "Work laptop",
            grants: this.grants,
            armedUntil: null,
            agents: [
              {
                key: "pane:pn_1",
                title: "auth tests",
                status: "working",
                engine: "Claude Code",
                workspace: "shop",
                paneId: "pn_1",
                threadId: null,
                preview: "Running the login tests",
                since: 1,
              },
            ],
            approvals: this.approvals,
            threads: [{ id: "th_web1", title: "Fix the login bug", state: "idle", updatedAt: 1, teammate: false }],
            routines: [{ id: "rt_web1", name: "Nightly triage", paused: false, running: false }],
          },
        };
      case "approval.decide": {
        const allow = msg.args?.decision === "allow";
        this.approvals = this.approvals.map((a) => (a.id === msg.args?.approvalId ? { ...a, queued: allow } : a));
        if (!allow) this.approvals = this.approvals.filter((a) => a.id !== msg.args?.approvalId);
        return { ok: true, data: { state: allow ? "queued" : "applied" } };
      }
      case "thread.read":
        return {
          ok: true,
          data: {
            items: [
              { role: "user", text: "Why does login fail?", at: 1 },
              { role: "assistant", text: "The session cookie is set on the wrong path.", at: 2 },
            ],
            before: null,
          },
        };
      case "thread.send":
        return { ok: true, data: {} };
      default:
        return { ok: false, error: { code: "unknown_command", message: "Unknown command" } };
    }
  }
}
