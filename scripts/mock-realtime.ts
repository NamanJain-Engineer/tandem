#!/usr/bin/env tsx
/**
 * The dev and E2E stand-in for Conductor's realtime voice provider
 * (F-CN-01, D-1102). The API's MockRealtime mints tokens pointing here when
 * `REALTIME_MOCK_URL=ws://127.0.0.1:8794/realtime`.
 *
 *   pnpm tsx scripts/mock-realtime.ts [--port 8794]
 *
 * The socket speaks the mock protocol of `crates/tandem-voice/src/realtime/mock.rs`
 * (JSON text frames, PCM16 24 kHz base64). It answers `session.update` with
 * `session.ready`, counts the audio it receives, and speaks each
 * `function_call.output` back as `transcript.assistant`.
 *
 * Control (plain HTTP on the same port):
 * - `GET /health` → `ok`
 * - `POST /__control {"connectDelayMs": n}`: hold each handshake this long.
 * - `POST /__say {"text": "..."}`: the user "says" text on the newest open
 *   socket: `speech.started`, `transcript.user`, then `function_call` act.
 *   Answers `{"callId"}`.
 * - `POST /__speak {"ms": n}`: the talker speaks for n ms (audio deltas).
 * - `POST /__close`: the provider hangs up the newest socket.
 * - `GET /__state`: connections, open sockets, the audio received (ms, and
 *   ms above a speech threshold), whether a session was configured with the
 *   one `act` tool, and every function output so far.
 * - `POST /__reset`
 *
 * No dependencies: a small RFC 6455 server on node:http.
 */
import { createHash } from "node:crypto";
import { createServer, type IncomingMessage } from "node:http";
import type { Duplex } from "node:stream";

const args = process.argv.slice(2);
const option = (name: string, fallback: string) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 && args[i + 1] ? (args[i + 1] as string) : fallback;
};
const port = Number(option("port", "8794"));
const RATE = 24_000;
const VOICED = 0.02 * 32_767;

type Conn = {
  id: number;
  socket: Duplex;
  open: boolean;
  configured: boolean;
  tools: string[];
  samples: number;
  voiced: number;
  buffer: Buffer;
};

let connectDelayMs = 0;
let nextConn = 0;
let nextCall = 0;
let conns: Conn[] = [];
let outputs: Array<{ callId: string; output: string }> = [];

function frame(text: string, opcode = 1): Buffer {
  const payload = Buffer.from(text, "utf8");
  const n = payload.length;
  const head =
    n < 126
      ? Buffer.from([0x80 | opcode, n])
      : n < 65_536
        ? Buffer.from([0x80 | opcode, 126, n >> 8, n & 0xff])
        : (() => {
            const b = Buffer.alloc(10);
            b[0] = 0x80 | opcode;
            b[1] = 127;
            b.writeBigUInt64BE(BigInt(n), 2);
            return b;
          })();
  return Buffer.concat([head, payload]);
}

function send(c: Conn, message: Record<string, unknown>): void {
  if (c.open) c.socket.write(frame(JSON.stringify(message)));
}

/** Parses whole frames out of the connection's buffer. */
function frames(c: Conn): Array<{ opcode: number; data: Buffer }> {
  const out: Array<{ opcode: number; data: Buffer }> = [];
  for (;;) {
    const b = c.buffer;
    if (b.length < 2) break;
    const opcode = (b[0] as number) & 0x0f;
    const masked = ((b[1] as number) & 0x80) !== 0;
    let len = (b[1] as number) & 0x7f;
    let at = 2;
    if (len === 126) {
      if (b.length < 4) break;
      len = b.readUInt16BE(2);
      at = 4;
    } else if (len === 127) {
      if (b.length < 10) break;
      len = Number(b.readBigUInt64BE(2));
      at = 10;
    }
    const mask = masked ? b.subarray(at, at + 4) : null;
    if (masked) at += 4;
    if (b.length < at + len) break;
    const data = Buffer.from(b.subarray(at, at + len));
    if (mask) for (let i = 0; i < data.length; i++) data[i] = (data[i] as number) ^ (mask[i % 4] as number);
    out.push({ opcode, data });
    c.buffer = b.subarray(at + len);
  }
  return out;
}

function onMessage(c: Conn, text: string): void {
  let m: Record<string, unknown>;
  try {
    m = JSON.parse(text) as Record<string, unknown>;
  } catch {
    return;
  }
  switch (m.type) {
    case "session.update": {
      const session = (m.session ?? {}) as { tools?: Array<{ name?: string }> };
      c.configured = true;
      c.tools = (session.tools ?? []).map((t) => String(t.name ?? ""));
      send(c, { type: "session.ready" });
      break;
    }
    case "input_audio.append": {
      const pcm = Buffer.from(String(m.audio ?? ""), "base64");
      for (let i = 0; i + 1 < pcm.length; i += 2) {
        c.samples++;
        if (Math.abs(pcm.readInt16LE(i)) > VOICED) c.voiced++;
      }
      break;
    }
    case "function_call.output": {
      const output = String(m.output ?? "");
      outputs.push({ callId: String(m.call_id ?? ""), output });
      send(c, { type: "transcript.assistant", text: output });
      send(c, { type: "response.done" });
      break;
    }
    case "response.create":
      send(c, { type: "response.done" });
      break;
  }
}

function close(c: Conn): void {
  if (!c.open) return;
  c.open = false;
  try {
    c.socket.end(frame("", 8));
  } catch {
    // already gone
  }
}

function upgrade(req: IncomingMessage, socket: Duplex): void {
  const url = new URL(req.url ?? "/", "http://mock");
  const auth = req.headers.authorization ?? "";
  const token = auth.startsWith("Bearer ") ? auth.slice(7) : (url.searchParams.get("token") ?? "");
  const key = req.headers["sec-websocket-key"];
  if (url.pathname !== "/realtime" || !token.startsWith("rt_mock_") || typeof key !== "string") {
    socket.end("HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n");
    return;
  }
  const accept = createHash("sha1")
    .update(key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11")
    .digest("base64");
  const c: Conn = {
    id: ++nextConn,
    socket,
    open: true,
    configured: false,
    tools: [],
    samples: 0,
    voiced: 0,
    buffer: Buffer.alloc(0),
  };
  setTimeout(() => {
    socket.write(
      `HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`,
    );
    conns.push(c);
    socket.on("data", (chunk: Buffer) => {
      c.buffer = Buffer.concat([c.buffer, chunk]);
      for (const f of frames(c)) {
        if (f.opcode === 1) onMessage(c, f.data.toString("utf8"));
        else if (f.opcode === 8) close(c);
        else if (f.opcode === 9) socket.write(Buffer.concat([Buffer.from([0x8a, f.data.length]), f.data]));
      }
    });
    socket.on("close", () => (c.open = false));
    socket.on("error", () => (c.open = false));
  }, connectDelayMs);
}

function newest(): Conn | undefined {
  return [...conns].reverse().find((c) => c.open);
}

async function body(req: IncomingMessage): Promise<Record<string, unknown>> {
  let raw = "";
  for await (const chunk of req) raw += chunk;
  try {
    return raw ? (JSON.parse(raw) as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

const ms = (samples: number) => Math.round((samples * 1000) / RATE);

const server = createServer(async (req, res) => {
  const json = (status: number, value: unknown) => {
    res.writeHead(status, { "content-type": "application/json" });
    res.end(JSON.stringify(value));
  };
  const path = new URL(req.url ?? "/", "http://mock").pathname;
  if (path === "/health") {
    res.end("ok");
    return;
  }
  const b = req.method === "POST" ? await body(req) : {};
  if (path === "/__control") {
    if (typeof b.connectDelayMs === "number") connectDelayMs = b.connectDelayMs;
    return json(200, { connectDelayMs });
  }
  if (path === "/__reset") {
    for (const c of conns) close(c);
    conns = [];
    outputs = [];
    connectDelayMs = 0;
    return json(200, {});
  }
  if (path === "/__state") {
    const c = newest() ?? conns.at(-1);
    return json(200, {
      connections: conns.length,
      open: conns.filter((x) => x.open).length,
      configured: c?.configured ?? false,
      tools: c?.tools ?? [],
      audioMs: ms(c?.samples ?? 0),
      voicedMs: ms(c?.voiced ?? 0),
      outputs,
    });
  }
  const c = newest();
  if (!c) return json(409, { error: "no open socket" });
  if (path === "/__say") {
    const callId = `fc_${++nextCall}`;
    const text = String(b.text ?? "");
    send(c, { type: "speech.started" });
    send(c, { type: "transcript.user", text });
    send(c, { type: "function_call", call_id: callId, name: "act", arguments: JSON.stringify({ request: text }) });
    return json(200, { callId });
  }
  if (path === "/__speak") {
    const n = Math.round((Number(b.ms ?? 500) * RATE) / 1000);
    const pcm = Buffer.alloc(n * 2);
    for (let i = 0; i < n; i++) pcm.writeInt16LE(Math.round(Math.sin(i / 8) * 6000), i * 2);
    send(c, { type: "transcript.assistant", text: String(b.text ?? "Okay.") });
    send(c, { type: "audio.delta", audio: pcm.toString("base64") });
    send(c, { type: "response.done" });
    return json(200, {});
  }
  if (path === "/__close") {
    close(c);
    return json(200, {});
  }
  json(404, { error: "not found" });
});

server.on("upgrade", upgrade);
server.listen(port, "127.0.0.1", () => console.log(`mock-realtime listening on ${port}`));
