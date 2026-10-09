import { browser } from "@wdio/globals";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { home, invoke } from "./app";

type Envelope = { turnId: string; event: { type: string; markdown?: string; reason?: string } };

/** Sends a message over IPC and waits for its turn to end; the turn's replies. */
export async function sendAndWait(threadId: string, text: string, timeout = 30_000): Promise<string[]> {
  const sent = await invoke<{ turnId: string | null }>("thread_send", {
    threadId,
    text,
    attachmentIds: [],
    queueIfBusy: false,
  });
  const turn = sent.turnId;
  if (!turn) throw new Error("the message was queued, not sent");
  let replies: string[] = [];
  await browser.waitUntil(
    async () => {
      const page = await invoke<{ items: Envelope[] }>("thread_events", { threadId, limit: 500 });
      const mine = page.items.filter((e) => e.turnId === turn);
      replies = mine.filter((e) => e.event.type === "assistant").map((e) => e.event.markdown ?? "");
      return mine.some((e) => e.event.type === "turn_end");
    },
    { timeout, interval: 200, timeoutMsg: `the turn for ${JSON.stringify(text)} never ended` },
  );
  return replies;
}

/** The gateway's port, from `$TANDEM_HOME/run/gateway.json`. */
export function gatewayPort(): number {
  const run = JSON.parse(readFileSync(join(home(), "run", "gateway.json"), "utf8")) as { port: number };
  return run.port;
}

export type TestGateway = {
  metrics: {
    port: number;
    listeners: number;
    leases: number;
    mcpSessions: number;
    requests: number;
    restarts: number;
    rssBytes: number | null;
  };
  starts: number;
};

export async function gatewayMetrics(): Promise<TestGateway> {
  return invoke<TestGateway>("test_gateway_metrics");
}

/** Connects a test vendor module; its connection id. */
export async function connectTestPlugin(pluginId: "test_echo" | "test_upper" | "test_down"): Promise<string> {
  const conn = await invoke<{ id: string }>("test_plugin_connect", { pluginId });
  return conn.id;
}

export type Teammate = { id: string; name: string; home: string; engineId: string };

/** Makes a teammate over IPC (the sheet is covered by E2E-M10-1). */
export async function makeTeammate(name: string, brief = "You keep the changelog."): Promise<Teammate> {
  return invoke<Teammate>("teammate_create", { input: { name, brief, engineId: "claude", places: [] } });
}

/** A new chat with a teammate; its thread id. */
export async function newChat(teammateId: string): Promise<string> {
  const t = await invoke<{ id: string }>("thread_create", { args: { kind: "teammate_chat", teammateId } });
  return t.id;
}

export type SpawnLine = { argv: string[]; envKeys: string[]; cwd: string };

/** Chat-engine spawns (Claude in stream-json mode), oldest first. */
export async function chatSpawns(): Promise<SpawnLine[]> {
  return (await invoke<SpawnLine[]>("test_spawn_log")).filter((r) => r.argv.includes("--input-format"));
}

/** The value after `flag` in an argv. */
export function argAfter(argv: string[], flag: string): string | null {
  const i = argv.indexOf(flag);
  return i >= 0 ? (argv[i + 1] ?? null) : null;
}
