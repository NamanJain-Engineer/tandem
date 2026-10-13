import { $, $$, browser } from "@wdio/globals";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import type { CallView } from "@tandem/protocol";
import {
  addFolder,
  textOf,
  fakeLog,
  freshUser,
  invoke,
  paneIds,
  paneText,
  sendToPane,
  split,
  waitForPaneText,
  waitForShell,
  waitForStatus,
} from "../support/app";
import { entitlement, testRoute, waitForEntitlement } from "../support/hosted";

// scripts/mock-realtime.ts, started by the runner; the API's MockRealtime
// mints sessions pointing at it (REALTIME_MOCK_URL, D-1102).
const REALTIME = "http://127.0.0.1:8794";

type MockState = {
  connections: number;
  open: number;
  configured: boolean;
  tools: string[];
  audioMs: number;
  voicedMs: number;
  outputs: Array<{ callId: string; output: string }>;
  speed: number;
  voice: string | null;
  instructions: string;
};

async function rt<T = unknown>(path: string, body?: unknown): Promise<T> {
  const res = await fetch(`${REALTIME}${path}`, {
    method: body === undefined ? "GET" : "POST",
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`${path}: ${res.status} ${await res.text()}`);
  return (await res.json()) as T;
}

const view = () => invoke<CallView>("conductor_state");

async function until(ok: (v: CallView) => boolean, what: string, timeout = 20_000): Promise<CallView> {
  let last: CallView | undefined;
  await browser.waitUntil(
    async () => {
      last = await view();
      return ok(last);
    },
    { timeout, interval: 200, timeoutMsg: `${what}: ${JSON.stringify(last)}` },
  );
  return last as CallView;
}

/** The user "says" something: the mock talker hands it to act. Returns the
 * talker's function output once the app answers it. */
async function say(text: string): Promise<string> {
  const { callId } = await rt<{ callId: string }>("/__say", { text });
  let out: string | undefined;
  await browser.waitUntil(
    async () => {
      out = (await rt<MockState>("/__state")).outputs.find((o) => o.callId === callId)?.output;
      return out !== undefined;
    },
    { timeout: 20_000, interval: 200, timeoutMsg: `no answer to "${text}"` },
  );
  return out as string;
}

let injected = 0;
async function inject(calls: Array<{ name: string; args: Record<string, unknown> }>): Promise<string[]> {
  const withIds = calls.map((c) => ({ id: `e2e.${++injected}.${c.name}`, ...c }));
  expect(await invoke<boolean>("test_conductor_inject", { calls: withIds })).toBe(true);
  return withIds.map((c) => c.id);
}

async function startCall(): Promise<CallView> {
  await $('[data-testid="conductor-button"]').click();
  return until((v) => v.phase === "listening", "the call connects", 30_000);
}

async function endCall(): Promise<void> {
  if (await $('[data-testid="conductor-end"]').isExisting()) await $('[data-testid="conductor-end"]').click();
  else await invoke("conductor_end_call");
  await until((v) => !["connecting", "listening", "thinking", "acting", "speaking"].includes(v.phase), "the call ends");
  // Clear the receipt or error.
  await invoke("conductor_end_call");
  await until((v) => v.phase === "idle", "the HUD clears", 10_000);
}

const hex = (s: string) => Buffer.from(s, "utf8").toString("hex");

/** 16 kHz mono PCM16 WAV: `toneMs` of a loud tone, then `quietMs` of silence. */
function toneWav(path: string, toneMs: number, quietMs: number): void {
  const rate = 16_000;
  const n = ((toneMs + quietMs) * rate) / 1000;
  const data = Buffer.alloc(n * 2);
  for (let i = 0; i < (toneMs * rate) / 1000; i++) data.writeInt16LE(Math.round(Math.sin((i / rate) * 2 * Math.PI * 440) * 16_000), i * 2);
  const head = Buffer.alloc(44);
  head.write("RIFF", 0);
  head.writeUInt32LE(36 + data.length, 4);
  head.write("WAVEfmt ", 8);
  head.writeUInt32LE(16, 16);
  head.writeUInt16LE(1, 20);
  head.writeUInt16LE(1, 22);
  head.writeUInt32LE(rate, 24);
  head.writeUInt32LE(rate * 2, 28);
  head.writeUInt16LE(2, 32);
  head.writeUInt16LE(16, 34);
  head.write("data", 36);
  head.writeUInt32LE(data.length, 40);
  writeFileSync(path, Buffer.concat([head, data]));
}

// E2E-M16-1 Conductor call (F-CN-01…04, F-CN-08): a Pro user's calls go
// through the real API (session, act, settle) to the mock talker. Doer tool
// calls the mock doer doesn't plan are injected through the same executor
// (test_conductor_inject).
describe("E2E-M16-1 Conductor call", function () {
  this.timeout(600_000);
  let email = "";
  let shell = "";
  let agent = "";

  before(async function () {
    this.timeout(180_000);
    await rt("/__reset", {});
    ({ email } = await freshUser("conductor"));
    await testRoute("plan", { email, plan: "pro", paid: true });
    await testRoute("credits", { email, monthly: 10_000, purchased: 0 });
    await waitForEntitlement((e) => e.plan === "pro" && e.credits?.monthly === 10_000, "Pro with 10,000 credits");
    await addFolder(`/tmp/tandem-e2e-conductor-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    shell = (await paneIds())[0] as string;
    await waitForShell(shell);
    agent = await split("right", "claude");
    await waitForPaneText(agent, "Fake Claude Code");
    await $('[data-testid="conductor-button"]').waitForExist({ timeout: 10_000 });
  });

  afterEach(async () => {
    // A case that failed while handshakes were held mustn't leave the next
    // case's call waiting to connect.
    await rt("/__control", { connectDelayMs: 0, release: true });
    if (!["idle"].includes((await view()).phase)) await endCall().catch(() => undefined);
  });

  it("F-CN-01.4 speech in the first 2 s before connect is delivered after connect", async () => {
    const wav = join(process.env.TANDEM_HOME ?? "/tmp", "conductor-speech.wav");
    toneWav(wav, 2_000, 8_000);
    await invoke("test_dictate_mic", { source: { source: "wav", path: wav } });
    // The test, not the mint's latency, decides when the call connects: the
    // call keeps the latest 3 s before connect, so connecting 2.2 s after the
    // click keeps all of the 2 s tone however long minting takes under load
    // (a fixed delay after the mint once connected 3.6 s in and lost 0.6 s).
    await rt("/__control", { hold: true });
    const clicked = Date.now();
    await $('[data-testid="conductor-button"]').click();
    await $('[data-testid="conductor-hud"]').waitForExist({ timeout: 5_000 });
    expect((await view()).phase).toBe("connecting");
    await browser.pause(Math.max(0, 2_200 - (Date.now() - clicked)));
    expect((await view()).phase).toBe("connecting");
    await rt("/__control", { release: true });
    await until((v) => v.phase === "listening", "connected", 30_000);
    await browser.waitUntil(async () => (await rt<MockState>("/__state")).voicedMs >= 1_800, {
      timeout: 10_000,
      timeoutMsg: `early speech never arrived: ${JSON.stringify(await rt("/__state"))}`,
    });
    const s = await rt<MockState>("/__state");
    // One tool for the talker: act.
    expect(s.configured).toBe(true);
    expect(s.tools).toEqual(["act"]);
    await endCall();
    await invoke("test_dictate_mic", { source: { source: "silence" } });
  });

  it("F-CN-01.1 a 65 s call settles 140 credits and the rest of the hold comes back", async function () {
    this.timeout(180_000);
    const before = (await entitlement()).credits?.monthly ?? 0;
    await startCall();
    await until((v) => v.elapsedMs >= 65_000, "65 s connected", 100_000);
    await $('[data-testid="conductor-end"]').click();
    const ended = await until((v) => v.phase === "ended", "the receipt");
    expect(ended.receipt?.seconds).toBeGreaterThanOrEqual(65);
    expect(ended.receipt?.seconds).toBeLessThanOrEqual(70);
    expect(ended.receipt?.credits).toBe(140);
    expect(await textOf('[data-testid="conductor-receipt"]')).toContain("140 credits");
    await waitForEntitlement((e) => e.credits?.monthly === before - 140, `${before} − 140 credits`, 20_000);
    await invoke("conductor_end_call");
    // The call log has it.
    const log = await invoke<Array<{ call: { id: string; credits: number | null } }>>("conductor_call_log", {});
    expect(log[0]?.call.credits).toBe(140);
  });

  it("F-CN-01.2 hanging up during a pending prompt_pane delivers nothing to the pane", async () => {
    await startCall();
    // The agent goes busy, so the prompt waits for it.
    await sendToPane(agent, "/script\r");
    await waitForStatus(agent, "working");
    const mark = fakeLog().length;
    await inject([{ name: "prompt_pane", args: { pane: agent, text: "E2E-NEVER-TYPED", send: true } }]);
    await until((v) => v.chips.some((c) => c.tool === "prompt_pane" && c.state === "running"), "the prompt is pending");
    await $('[data-testid="conductor-end"]').click();
    await until((v) => v.phase === "ended", "ended");
    // The agent finishes and goes back to its prompt; nothing arrives.
    await waitForStatus(agent, "idle", 20_000).catch(() => undefined);
    await browser.pause(2_000);
    expect(fakeLog().slice(mark).filter((l) => l.includes(hex("E2E-NEVER-TYPED")))).toEqual([]);
    expect(await paneText(agent)).not.toContain("E2E-NEVER-TYPED");
  });

  it("F-CN-01.3 the 30-minute cap ends the call with a notice", async () => {
    await startCall();
    await invoke("test_conductor_clock", { advanceMs: 28 * 60_000 });
    await until((v) => v.notice === "2 minutes left in this call", "the warning");
    expect(await textOf('[data-testid="conductor-notice"]')).toBe("2 minutes left in this call");
    await invoke("test_conductor_clock", { advanceMs: 2 * 60_000 });
    const v = await until((x) => x.phase === "error", "the cap");
    expect(v.error?.code).toBe("time_cap");
    expect(await textOf('[data-testid="conductor-error-message"]')).toBe("This call reached its 30-minute limit.");
    expect(await textOf('[data-testid="conductor-error-action"]')).toBe("Start a new call");
    await invoke("test_conductor_clock", { advanceMs: -30 * 60_000 });
    await $('[data-testid="conductor-dismiss"]').click();
    await until((x) => x.phase === "idle", "dismissed");
  });

  it("F-CN-04.1 after 5 actions, the HUD shows 3 chips and +2", async () => {
    await startCall();
    // One through the real doer: the mock plans list_panes for it.
    expect(await say("which agents are running")).toBe("Done.");
    await inject([
      { name: "switch_mode", args: { mode: "code" } },
      { name: "get_setting", args: { key: "theme" } },
      { name: "read_pane", args: { pane: agent, lines: 20 } },
      { name: "focus_pane", args: { pane: agent } },
    ]);
    const v = await until((x) => x.chips.length === 5 && x.chips.every((c) => c.state === "done"), "5 actions done");
    expect(v.chips[0]?.tool).toBe("list_panes");
    await browser.waitUntil(async () => (await $$('[data-testid="conductor-chip"]').length) === 3, {
      timeout: 5_000,
      timeoutMsg: "3 chips",
    });
    expect(await textOf('[data-testid="conductor-more"]')).toBe("+2");
    await endCall();
  });

  it("F-CN-02.2 set_setting outside the closed list is rejected", async () => {
    await startCall();
    // The doer's catalog doesn't have it, so the API says no…
    expect(await say("turn on telemetry")).toBe("I can't do that by voice.");
    // …and a call that got past the planner is refused here too.
    const [id] = await inject([{ name: "set_setting", args: { key: "telemetry_usage", value: true } }]);
    const v = await until((x) => x.chips.some((c) => c.id === id && c.state === "failed"), "refused");
    expect(v.chips.find((c) => c.id === id)?.reason).toContain("can't change that setting");
    const settings = await invoke<Record<string, unknown>>("settings_get_all");
    expect(settings.telemetry_usage).not.toBe(true);
    await endCall();
  });

  it("F-CN-02.3 run_command_in_shell runs nothing without the on-screen click", async () => {
    await startCall();
    await inject([{ name: "run_command_in_shell", args: { pane: shell, cmd: "echo E2E-SHELL-$((40+2))" } }]);
    const q = (await until((v) => v.question?.tool === "run_command_in_shell", "the question")).question;
    expect(q?.review).toBe("screen_only");
    expect(await textOf('[data-testid="conductor-question"]')).toContain("Confirm on screen");
    // A spoken yes, naming the request, is refused.
    expect(await say(`answer ${q?.requestId} yes`)).toContain("click on screen");
    await browser.pause(1_500);
    expect(await paneText(shell)).not.toContain("E2E-SHELL-42");
    expect((await view()).question?.requestId).toBe(q?.requestId);
    // The click runs it.
    await $('[data-testid="conductor-yes"]').click();
    await waitForPaneText(shell, "E2E-SHELL-42");
    await until((v) => v.chips.some((c) => c.tool === "run_command_in_shell" && c.state === "done"), "ran");
    await endCall();
    const log = await invoke<Array<{ events: Array<{ text: string }> }>>("conductor_call_log", {});
    expect(log[0]?.events.some((e) => e.text.includes("Confirmed on screen"))).toBe(true);
  });

  it("F-CN-02 the app's own tools answer by voice: launch, wait, usage, updates", async function () {
    this.timeout(120_000);
    await startCall();
    const before = await paneIds();
    const ids = await inject([
      { name: "launch_agents", args: { engine: "claude", count: 1, workspace: "current" } },
      { name: "get_usage", args: {} },
      { name: "check_updates", args: {} },
      { name: "list_routines", args: {} },
    ]);
    const v = await until(
      (x) => ids.every((id) => x.chips.some((c) => c.id === id && c.state !== "running")),
      "the four tools ran",
      60_000,
    );
    for (const id of ids) expect(v.chips.find((c) => c.id === id)?.state).toBe("done");
    // The launched agent is placed in the window, in its own tab (as the launch sheet does).
    let launched = "";
    await browser.waitUntil(
      async () => {
        launched = (await paneIds()).find((p) => !before.includes(p)) ?? "";
        return launched !== "";
      },
      { timeout: 30_000, timeoutMsg: "the launched pane" },
    );
    await waitForPaneText(launched, "Fake Claude Code");
    const [wait] = await inject([{ name: "wait_for_pane", args: { pane: launched, timeout: 5 } }]);
    await until((x) => x.chips.some((c) => c.id === wait && c.state === "done"), "waited");
    await endCall();
  });

  it("F-CN-01 \"talk faster\" moves the talker's speech rate a step", async () => {
    await startCall();
    expect(await say("talk faster")).toBe("Okay, I'll talk faster.");
    await browser.waitUntil(async () => (await rt<MockState>("/__state")).speed === 1.1, {
      timeout: 5_000,
      timeoutMsg: "the speed never changed",
    });
    expect(await say("slow down")).toBe("Okay, I'll talk slower.");
    await browser.waitUntil(async () => (await rt<MockState>("/__state")).speed === 1, { timeout: 5_000 });
    await endCall();
  });

  it("F-CN-04 the floating ring opens as its own window when turned on", async () => {
    const main = await browser.getWindowHandle();
    const before = (await browser.getWindowHandles()).length;
    await invoke("settings_set", { key: "conductor_floating_ring", value: true });
    await browser.waitUntil(async () => (await browser.getWindowHandles()).length > before, {
      timeout: 15_000,
      timeoutMsg: "no ring window",
    });
    const ring = (await browser.getWindowHandles()).find((h) => h !== main) as string;
    await browser.switchToWindow(ring);
    await $('[data-testid="floating-ring"]').waitForExist({ timeout: 10_000 });
    await browser.switchToWindow(main);
    await invoke("settings_set", { key: "conductor_floating_ring", value: false });
  });

  it("F-CN-03.2 a voice allow for approval A can't resolve approval B", async () => {
    await startCall();
    await inject([{ name: "execute_task", args: { task: "TAN-1" } }]);
    const a = (await until((v) => v.question?.tool === "execute_task", "question A")).question;
    await inject([{ name: "create_teammate", args: { description: "Reviews pull requests for security" } }]);
    await until((v) => v.chips.some((c) => c.tool === "create_teammate" && c.state === "waiting"), "B asked");
    expect((await view()).question?.requestId).toBe(a?.requestId);
    expect(await say(`answer ${a?.requestId} allow`)).toBe("Got it.");
    const b = (await until((v) => !!v.question && v.question.requestId !== a?.requestId, "B shown")).question;
    expect(b?.tool).toBe("create_teammate");
    // A again answers nothing, and B is still waiting.
    expect(await say(`answer ${a?.requestId} allow`)).toContain("no question");
    await browser.pause(1_000);
    const now = await view();
    expect(now.question?.requestId).toBe(b?.requestId);
    expect(now.chips.find((c) => c.tool === "create_teammate")?.state).toBe("waiting");
    expect(now.chips.find((c) => c.tool === "execute_task")?.state).not.toBe("waiting");
    // Hanging up withdraws B: it never ran.
    await endCall();
    const log = await invoke<Array<{ events: Array<{ text: string }> }>>("conductor_call_log", {});
    const lines = log[0]?.events.map((e) => e.text) ?? [];
    expect(lines.some((t) => t.startsWith("Execute task") && t.includes("Confirmed by voice"))).toBe(true);
    expect(lines.some((t) => t.startsWith("Create teammate") && t.includes("Confirmed"))).toBe(false);
  });
});

// The persona, wake and mic-owner seams (F-CN-06, F-CN-07), with their own
// Pro user: the API mints at most 10 voice sessions an hour per user, and
// the calls above use them.
describe("E2E-M16-1 Conductor call: persona, wake and microphone", function () {
  this.timeout(300_000);

  before(async function () {
    await rt("/__reset", {});
    const { email } = await freshUser("conductor-seams");
    await testRoute("plan", { email, plan: "pro", paid: true });
    await testRoute("credits", { email, monthly: 10_000, purchased: 0 });
    await waitForEntitlement((e) => e.plan === "pro" && e.credits?.monthly === 10_000, "Pro with 10,000 credits");
    await addFolder(`/tmp/tandem-e2e-conductor-seams-${Date.now()}`);
    await $('[data-testid="conductor-button"]').waitForExist({ timeout: 10_000 });
  });

  afterEach(async () => {
    if (!["idle"].includes((await view()).phase)) await endCall().catch(() => undefined);
  });

  it("F-CN-06 a call starts with the persona's voice, speed, name and memory", async () => {
    const persona = { conductor_name: "Ada", conductor_voice: "sage", conductor_speed: 5, conductor_memory: true };
    for (const [key, value] of Object.entries(persona)) await invoke("settings_set", { key, value });
    await invoke("conductor_memory_save", { text: "- Works on the billing service\n" });
    try {
      await startCall();
      const s = await rt<MockState>("/__state");
      expect(s.voice).toBe("sage");
      expect(s.instructions).toContain("Ada");
      expect(s.instructions).toContain("- Works on the billing service");
      // Speed 5 of 5: the talker starts at the fastest rate.
      await browser.waitUntil(async () => (await rt<MockState>("/__state")).speed === 1.2, {
        timeout: 5_000,
        timeoutMsg: "the persona's speed never reached the talker",
      });
      await endCall();
    } finally {
      await invoke("conductor_memory_clear");
      for (const key of Object.keys(persona)) await invoke("settings_reset", { key });
    }
  });

  it("F-CN-07 the wake phrase starts a call that holds the mic until it ends", async () => {
    expect(await invoke<string | null>("test_mic_owner")).toBe(null);
    await browser.waitUntil(async () => invoke<boolean>("test_conductor_wake"), {
      timeout: 10_000,
      timeoutMsg: "the wake service never came up",
    });
    await until((v) => v.phase === "listening", "the wake call connects", 30_000);
    expect(await invoke<string | null>("test_mic_owner")).toBe("call");
    // Heard again mid-call: the running call stays the only one.
    const id = (await view()).callId;
    expect(await invoke<boolean>("test_conductor_wake")).toBe(true);
    expect((await view()).callId).toBe(id);
    await endCall();
    await browser.waitUntil(async () => (await invoke<string | null>("test_mic_owner")) === null, {
      timeout: 5_000,
      timeoutMsg: "the call kept the mic after it ended",
    });
  });
});
