// Demo mode (F-RM-05): a simulated computer that lives inside the phone app,
// for trying Vibezeo Remote without pairing (and for App Review). It speaks
// the real protocol: the app's PhoneClient pairs with it over an in-memory
// socket, so every screen runs the same code it runs against a real desktop.
// Everything it shows is made up and labelled as a demo.
import type { SocketFactory } from "./client";
import type {
  PhoneAgent,
  PhoneApproval,
  PhoneGrants,
  PhoneReview,
  PhoneRoutine,
  PhoneSnapshot,
  PhoneThread,
} from "./messages";
import {
  b64url,
  decodeHello,
  decodeJson,
  encodeJson,
  equalBytes,
  generateKeyPair,
  helloTag,
  newSession,
  openNext,
  rootSecret,
  sealNext,
  type PairingLink,
  type SessionState,
} from "./protocol";

export const DEMO_RELAY = "demo://vibezeo-remote";
export const DEMO_DESKTOP = "Demo computer";

type Cmd = {
  t: string;
  id?: number;
  cmd?: string;
  args?: Record<string, unknown>;
  device?: string;
  eph?: string;
};
type Item = { role: string; text: string; at: number };

function now(): number {
  return Date.now();
}

/** The simulated desktop's state and answers. */
export class DemoDesktop {
  readonly pid = "pr_demo0000000000";
  private readonly identity = generateKeyPair();
  private readonly s = crypto.getRandomValues(new Uint8Array(32));
  private session: SessionState | null = null;
  private socket: DemoSocket | null = null;
  private tick = 0;
  grants: PhoneGrants = { chat: true, terminal: true, approvals: true, routines: true };
  agents: PhoneAgent[];
  approvals: PhoneApproval[];
  threads: PhoneThread[];
  routines: PhoneRoutine[];
  transcripts: Record<string, Item[]>;
  typed: { paneId: string; text: string }[] = [];

  constructor() {
    const t = now();
    this.agents = [
      {
        key: "pane:pn_demo1",
        title: "Checkout tests",
        status: "working",
        engine: "Claude Code",
        workspace: "shop-web",
        paneId: "pn_demo1",
        threadId: null,
        preview: "Running 48 tests in checkout/…",
        since: t - 4 * 60_000,
      },
      {
        key: "thread:th_demo1",
        title: "Fix the login redirect",
        status: "waiting",
        engine: "Codex",
        workspace: "shop-web",
        paneId: null,
        threadId: "th_demo1",
        preview: "Wants to run a command",
        since: t - 2 * 60_000,
      },
      {
        key: "pane:pn_demo2",
        title: "Docs refresh",
        status: "done",
        engine: "Gemini CLI",
        workspace: "docs",
        paneId: "pn_demo2",
        threadId: null,
        preview: "Updated 6 pages",
        since: t - 20 * 60_000,
      },
    ];
    this.approvals = [
      {
        id: "ap_demo1",
        summary: "Run `pnpm test --filter checkout`",
        detail: "pnpm test --filter checkout",
        source: "Fix the login redirect",
        queued: false,
        createdAt: t - 60_000,
      },
    ];
    this.threads = [
      {
        id: "th_demo1",
        title: "Fix the login redirect",
        state: "waiting",
        updatedAt: t - 60_000,
        teammate: false,
      },
      { id: "th_demo2", title: "Ava (reviewer)", state: "idle", updatedAt: t - 30 * 60_000, teammate: true },
    ];
    this.routines = [
      { id: "rt_demo1", name: "Nightly dependency check", paused: false, running: false },
      { id: "rt_demo2", name: "Weekly changelog draft", paused: true, running: false },
    ];
    this.transcripts = {
      th_demo1: [
        {
          role: "user",
          text: "After sign-in we land on /home instead of the page we came from. Fix it.",
          at: t - 10 * 60_000,
        },
        { role: "tool", text: "read src/auth/redirect.ts", at: t - 9 * 60_000 },
        {
          role: "assistant",
          text: "The return path is dropped when the session cookie is refreshed. I'll keep it in the state parameter and add a test.",
          at: t - 8 * 60_000,
        },
      ],
      th_demo2: [
        { role: "user", text: "Can you review the checkout PR when the tests pass?", at: t - 40 * 60_000 },
        {
          role: "assistant",
          text: "Sure. I'll look at the diff and leave notes in the thread.",
          at: t - 39 * 60_000,
        },
      ],
    };
  }

  /** The pairing link the demo "scans". */
  link(): PairingLink {
    return {
      v: 1,
      pid: this.pid,
      dpk: b64url(this.identity.public),
      s: b64url(this.s),
      relay: DEMO_RELAY,
      ptok: "demo",
    };
  }

  /** A socket factory that reaches this desktop (and nothing else). */
  socketFactory(): SocketFactory {
    return (url) => {
      if (url !== DEMO_RELAY) throw new Error("demo mode only reaches the demo computer");
      const socket = new DemoSocket(this);
      this.socket = socket;
      return socket;
    };
  }

  /** Moves the story on: the tests finish, then a new approval arrives. */
  advance(): void {
    this.tick += 1;
    if (this.tick === 1) {
      this.agents = this.agents.map((a) =>
        a.key === "pane:pn_demo1" ? { ...a, status: "done", preview: "48 passed", since: now() } : a,
      );
    } else if (this.tick === 2) {
      this.approvals = [
        ...this.approvals,
        {
          id: "ap_demo2",
          summary: "Edit `src/checkout/cart.ts`",
          detail: "src/checkout/cart.ts",
          source: "Checkout tests",
          queued: false,
          createdAt: now(),
        },
      ];
    }
    this.send({ t: "changed" });
  }

  private send(message: unknown): void {
    if (!this.session || !this.socket) return;
    const sealed = sealNext(this.session, encodeJson(message));
    this.session = sealed.state;
    this.socket.deliver(sealed.frame);
  }

  frame(bytes: Uint8Array): void {
    if (bytes[0] === 0x01) {
      const { phonePub, tag } = decodeHello(bytes);
      if (!equalBytes(helloTag(this.s, this.pid, phonePub), tag)) return;
      const root = rootSecret(
        this.s,
        this.pid,
        this.identity.public,
        phonePub,
        this.identity.secret,
        phonePub,
      );
      this.session = newSession(this.pid, "desktop", root);
      return;
    }
    if (!this.session) return;
    let msg: Cmd;
    try {
      const opened = openNext(this.session, bytes);
      this.session = opened.state;
      msg = decodeJson<Cmd>(opened.plaintext);
    } catch {
      this.send({ t: "reject", reason: "replay" });
      return;
    }
    if (msg.t === "hi")
      this.send({ t: "welcome", desktop: DEMO_DESKTOP, grants: this.grants, armedUntil: null });
    if (msg.t === "bye") this.session = null;
    if (msg.t === "cmd" && msg.id !== undefined) this.send({ t: "res", id: msg.id, ...this.answer(msg) });
  }

  /** F-CN-09: one made-up item waiting for review, and its diff. */
  review(): PhoneReview {
    return {
      item: {
        key: "pane:demo-1",
        title: "Ada",
        engine: "Claude Code",
        workspace: "shop",
        branch: "tandem/fix-auth",
        finishedAt: now() - 4 * 60_000,
        closing: "(Demo) Expired tokens now refresh once, then sign the user out.",
        files: 2,
        additions: 14,
        deletions: 3,
        risks: [{ rule: "auth", files: ["src/auth/session.ts"] }],
        check: { state: "passed", exit: 0, output: "" },
        changed: false,
      },
      files: [
        {
          path: "src/auth/session.ts",
          change: "modified",
          additions: 12,
          deletions: 3,
          patch:
            "@@ -40,6 +40,15 @@\n export async function refresh(token: Token) {\n-  if (expired(token)) return null;\n+  if (expired(token)) {\n+    const next = await renew(token);\n+    if (next) return next;\n+    signOut();\n+    return null;\n+  }\n   return token;\n }\n",
          truncated: false,
        },
        {
          path: "src/auth/session.test.ts",
          change: "modified",
          additions: 2,
          deletions: 0,
          patch:
            '@@ -10,3 +10,5 @@\n it("refreshes", async () => {\n+  expect(await refresh(old)).not.toBeNull();\n+  expect(signOut).not.toHaveBeenCalled();\n });\n',
          truncated: false,
        },
      ],
      position: 1,
      total: 1,
    };
  }

  snapshot(): PhoneSnapshot {
    return {
      desktop: DEMO_DESKTOP,
      grants: this.grants,
      armedUntil: null,
      agents: this.agents,
      approvals: this.approvals,
      threads: this.threads,
      routines: this.routines,
    };
  }

  private answer(msg: Cmd): { ok: boolean; data?: unknown; error?: { code: string; message: string } } {
    const args = msg.args ?? {};
    switch (msg.cmd) {
      case "view.snapshot":
        return { ok: true, data: this.snapshot() };
      case "approval.decide": {
        // Like a desktop that isn't armed: deny applies, allow waits there.
        const allow = args.decision === "allow";
        this.approvals = allow
          ? this.approvals.map((a) => (a.id === args.approvalId ? { ...a, queued: true } : a))
          : this.approvals.filter((a) => a.id !== args.approvalId);
        return { ok: true, data: { state: allow ? "queued" : "applied" } };
      }
      case "thread.read": {
        const items = this.transcripts[String(args.threadId)];
        return items
          ? { ok: true, data: { items, before: null } }
          : { ok: false, error: { code: "not_found", message: "No such thread" } };
      }
      case "thread.send": {
        const id = String(args.threadId);
        const items = this.transcripts[id];
        if (!items) return { ok: false, error: { code: "not_found", message: "No such thread" } };
        items.push({ role: "user", text: String(args.text ?? ""), at: now() });
        items.push({
          role: "assistant",
          text: "(Demo) Got it. On a real computer, your agent answers here.",
          at: now(),
        });
        return { ok: true, data: {} };
      }
      case "pane.type": {
        const paneId = String(args.paneId);
        if (!this.agents.some((a) => a.paneId === paneId)) {
          return { ok: false, error: { code: "not_found", message: "That pane is closed" } };
        }
        this.typed.push({ paneId, text: String(args.text ?? "") });
        return { ok: true, data: {} };
      }
      case "review.current":
        return { ok: true, data: this.review() };
      case "routine.run_now": {
        const r = this.routines.find((x) => x.id === args.routineId);
        if (!r) return { ok: false, error: { code: "not_found", message: "No such routine" } };
        if (r.paused) return { ok: false, error: { code: "paused", message: "That routine is paused" } };
        this.routines = this.routines.map((x) => (x.id === r.id ? { ...x, running: true } : x));
        return { ok: true, data: {} };
      }
      default:
        return { ok: false, error: { code: "unknown_command", message: "Unknown command" } };
    }
  }
}

/** An in-memory socket between the app's client and the demo desktop. */
class DemoSocket {
  binaryType = "arraybuffer";
  readyState = 0;
  protocol = "tandem-pair.demo";
  onopen: ((ev: unknown) => void) | null = null;
  onclose: ((ev: { code: number; reason: string }) => void) | null = null;
  onerror: ((ev: unknown) => void) | null = null;
  onmessage: ((ev: { data: unknown }) => void) | null = null;

  constructor(private readonly desktop: DemoDesktop) {
    setTimeout(() => {
      this.readyState = 1;
      this.onopen?.({});
      // Like the relay: the desktop is there.
      this.onmessage?.({ data: JSON.stringify({ type: "peer", up: true }) });
    }, 0);
  }

  send(data: Uint8Array): void {
    const copy = new Uint8Array(data);
    setTimeout(() => this.desktop.frame(copy), 0);
  }

  deliver(frame: Uint8Array): void {
    if (this.readyState !== 1) return;
    const buf = frame.buffer.slice(frame.byteOffset, frame.byteOffset + frame.byteLength);
    setTimeout(() => this.onmessage?.({ data: buf }), 0);
  }

  close(code = 1000, reason = ""): void {
    if (this.readyState === 3) return;
    this.readyState = 3;
    setTimeout(() => this.onclose?.({ code, reason }), 0);
  }
}
