import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { Key } from "webdriverio";
import type { CallView } from "@tandem/protocol";
import { PhoneClient } from "@tandem/remote/client";
import { parsePairingFragment } from "@tandem/remote/protocol";
import { MemoryPairingStore } from "@tandem/remote/store";
import {
  addFolder,
  fakeLog,
  freshUser,
  invoke,
  mod,
  paneIds,
  sendToPane,
  split,
  waitFor,
  waitForPaneText,
  waitForStatus,
} from "../support/app";
import { testRoute, waitForEntitlement } from "../support/hosted";

const REALTIME = "http://127.0.0.1:8794";
const GIT_ENV = {
  ...process.env,
  GIT_AUTHOR_NAME: "E2E",
  GIT_AUTHOR_EMAIL: "e2e@vibezeo.test",
  GIT_COMMITTER_NAME: "E2E",
  GIT_COMMITTER_EMAIL: "e2e@vibezeo.test",
};
const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", env: GIT_ENV }).trim();

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
  return waitFor(view, ok, what, { timeout, interval: 200 });
}

let injected = 0;
/** Runs one doer tool call through the call's executor; waits for its chip. */
async function tool(name: string, args: Record<string, unknown> = {}): Promise<void> {
  const id = `e2e.${++injected}.${name}`;
  expect(await invoke<boolean>("test_conductor_inject", { calls: [{ id, name, args }] })).toBe(true);
  const v = await until((x) => x.chips.some((c) => c.id === id && c.state !== "running"), `${name} ran`);
  const chip = v.chips.find((c) => c.id === id);
  expect(`${chip?.state} ${chip?.reason ?? ""}`.trim()).toBe("done");
}

type Item = { key: string; state: string };
const queue = async (): Promise<Item[]> => (await invoke<{ items: Item[] }>("review_queue", {})).items;

/** Everything the fake engines received as bracketed pastes since `mark`. */
function pastes(mark: number): string[] {
  const bytes = fakeLog()
    .slice(mark)
    .filter((l) => l.startsWith("raw: "))
    .map((l) => Buffer.from(l.slice(5), "hex").toString("utf8"))
    .join("");
  return bytes
    .split("\u001b[200~")
    .slice(1)
    .map((chunk) => chunk.split("\u001b[201~")[0] ?? "");
}

// E2E-M18-CN-09 Walk-and-review (F-CN-09): Conductor walks the review queue
// in a call (tools injected through the call's executor, as E2E-M16-1 does),
// and a paired phone reads the current item's diff.
describe("E2E-M18-CN-09 walk-and-review", function () {
  this.timeout(600_000);
  const repo = `/tmp/tandem-e2e-cn-09-${Date.now()}`;
  const store = new MemoryPairingStore();
  const phone = new PhoneClient(store, {}, undefined, "E2E phone");
  let agent = "";

  before(async function () {
    this.timeout(180_000);
    await rt("/__reset", {});
    const { email } = await freshUser("walk");
    await testRoute("plan", { email, plan: "pro", paid: true });
    await testRoute("credits", { email, monthly: 10_000, purchased: 0 });
    await waitForEntitlement((e) => e.plan === "pro" && e.credits?.monthly === 10_000, "Pro with credits");
    mkdirSync(join(repo, "db/migrations"), { recursive: true });
    git(repo, "init", "-q", "-b", "main");
    writeFileSync(join(repo, "README.md"), "# app\n");
    git(repo, "add", ".");
    git(repo, "commit", "-q", "-m", "first");
    await addFolder(repo);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    agent = await split("right", "claude");
    await waitForPaneText(agent, "Fake Claude Code");
  });

  after(() => phone.disconnect());

  it("F-CN-09.1-3 Conductor walks the queue, sends work back and approves it", async () => {
    writeFileSync(join(repo, "db/migrations/0002.sql"), "alter table a add b int;\n");
    await sendToPane(agent, "finished\r");
    await waitForStatus(agent, "done", 15_000);
    await browser.waitUntil(async () => (await queue()).some((i) => i.key === `pane:${agent}`), {
      timeout: 5_000,
      timeoutMsg: "the agent never joined the queue",
    });
    await $('[data-testid="conductor-button"]').click();
    await until((v) => v.phase === "listening", "the call connects", 30_000);

    await tool("review_next");
    const mark = fakeLog().length;
    await tool("review_send_back", { text: "Handle expired tokens too" });
    await browser.waitUntil(
      async () => pastes(mark).some((p) => p.startsWith("Review of your changes on main:")),
      {
        timeout: 20_000,
        timeoutMsg: `pastes: ${JSON.stringify(pastes(mark))}`,
      },
    );
    expect(pastes(mark).find((p) => p.startsWith("Review of your changes"))).toContain(
      "Handle expired tokens too",
    );

    // The agent answers and finishes again: it's new work, read again, then approved.
    await waitForStatus(agent, "done", 15_000);
    await browser.waitUntil(async () => (await queue()).some((i) => i.key === `pane:${agent}`), {
      timeout: 5_000,
    });
    await tool("review_next");
    await tool("review_approve");
    expect((await queue()).find((i) => i.key === `pane:${agent}`)?.state).toBe("approved");

    await $('[data-testid="conductor-end"]').click();
    await until(
      (v) => !["connecting", "listening", "thinking", "acting", "speaking"].includes(v.phase),
      "the call ends",
    );
    await invoke("conductor_end_call");
  });

  it("F-CN-09.5 the paired phone reads the current item and its diff, and can't approve", async function () {
    this.timeout(120_000);
    await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
    await browser.keys([mod(), ","]);
    const nav = $('//nav[@aria-label="Settings pages"]//button[normalize-space()="Remote"]');
    await nav.waitForClickable({ timeout: 10_000 });
    await nav.click();
    await $('[data-testid="remote-pair"]').click();
    const linkEl = $('[data-testid="remote-pair-link"]');
    await linkEl.waitForExist({ timeout: 15_000 });
    const parsed = parsePairingFragment(new URL(String(await linkEl.getProperty("textContent"))).hash);
    await phone.pair(parsed!);
    await $('[data-testid="remote-paired"]').waitForDisplayed({ timeout: 15_000 });
    await browser.keys(Key.Escape);
    // The approved item changes: it's back to review, and the phone shows it.
    writeFileSync(join(repo, "db/migrations/0003.sql"), "alter table a add c int;\n");
    await sendToPane(agent, "one more\r");
    await waitForStatus(agent, "done", 15_000);
    await browser.waitUntil(
      async () => (await queue()).some((i) => i.key === `pane:${agent}` && i.state === "changed"),
      { timeout: 10_000, timeoutMsg: "the item never came back as changed" },
    );

    type Review = {
      item: { key: string; title: string; risks: Array<{ rule: string }> } | null;
      files: Array<{ path: string; patch: string }>;
    };
    const r = await phone.command<Review>("review.current");
    expect(r.ok).toBe(true);
    if (!r.ok) return;
    expect(r.data.item?.key).toBe(`pane:${agent}`);
    expect(r.data.item?.risks.map((x) => x.rule)).toContain("migrations");
    const file = r.data.files.find((f) => f.path === "db/migrations/0002.sql");
    expect(file?.patch).toContain("+alter table a add b int;");
    const approve = await phone.command("review.approve", { key: `pane:${agent}` });
    expect(approve.ok).toBe(false);
  });

  it("F-CN-09.2 a thread's item gets the spoken review as a message", async function () {
    this.timeout(180_000);
    const ws = (await invoke<Array<{ id: string; root: string }>>("workspace_list", {})).find(
      (w) => w.root === repo,
    );
    const thread = await invoke<{ id: string }>("thread_create", {
      args: { kind: "thread", workspaceId: ws?.id, engineId: "claude", access: "edits" },
    });
    await invoke("thread_send", { threadId: thread.id, text: "write notes.md=hello" });
    await browser.waitUntil(async () => (await queue()).some((i) => i.key === `thread:${thread.id}`), {
      timeout: 30_000,
      timeoutMsg: "the thread never joined the queue",
    });
    await $('[data-testid="conductor-button"]').click();
    await until((v) => v.phase === "listening", "the call connects", 30_000);
    // The pane's item changed too (same folder) and finished first: the thread is next.
    await tool("review_next");
    await tool("review_next");
    await tool("review_send_back", { text: "Add a heading to the notes" });
    type Page = { items: Array<{ event: { type: string; text?: string } }> };
    await browser.waitUntil(
      async () =>
        (await invoke<Page>("thread_events", { threadId: thread.id, limit: 50 })).items.some(
          (e) =>
            e.event.type === "user" &&
            (e.event.text ?? "").startsWith("Review of your changes on main:") &&
            (e.event.text ?? "").includes("Add a heading to the notes"),
        ),
      { timeout: 20_000, timeoutMsg: "the review never reached the thread" },
    );
    await $('[data-testid="conductor-end"]').click();
    await until(
      (v) => !["connecting", "listening", "thinking", "acting", "speaking"].includes(v.phase),
      "the call ends",
    );
    await invoke("conductor_end_call");
  });
});
