import { $, $$, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Key } from "webdriverio";
import {
  addFolder,
  freshUser,
  home,
  invoke,
  mod,
  paneIds,
  setPlan,
  split,
  typeKeys,
  fakeLog,
  userHome,
  waitForPaneText,
} from "../support/app";
import { waitForEntitlement } from "../support/hosted";

type Account = { id: string; engineId: string; homePath: string };
type Session = {
  key: string;
  engineId: string;
  accountId: string | null;
  sessionId: string;
  folder: string | null;
  folderExists: boolean;
};
type Spawn = { argv: string[]; envKeys: string[]; providerHome: string | null; cwd: string };

const CLAUDE_ID = "aaaa1111-2222-3333-4444-555566667777";
const OWN_ID = "bbbb1111-2222-3333-4444-555566667777";
const CODEX_ID = "cccc1111-2222-3333-4444-555566667777";

function claudeSession(dir: string, id: string, cwd: string, prompt: string): void {
  const folder = join(dir, "projects", cwd.replace(/[\\/]/g, "-"));
  mkdirSync(folder, { recursive: true });
  const lines = [
    { type: "user", sessionId: id, cwd, message: { role: "user", content: prompt } },
    {
      type: "assistant",
      sessionId: id,
      cwd,
      message: { role: "assistant", content: [{ type: "text", text: "Done." }] },
    },
  ];
  writeFileSync(join(folder, `${id}.jsonl`), lines.map((l) => JSON.stringify(l)).join("\n") + "\n");
}

/** The run's own HOME, under the temp folder e2e/run.ts made: this spec
 * writes and deletes `~/.claude` and `~/.codex`, so it refuses any other
 * (a developer's real sign-ins). */
function testUserHome(): string {
  const user = userHome();
  const run = home();
  if (!run.startsWith(join(tmpdir(), "tandem-e2e-")) || !user.startsWith(join(run, "home"))) {
    throw new Error(`refusing to touch ${user}: run this spec through e2e/run.ts`);
  }
  return user;
}

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

async function openLibrary(): Promise<void> {
  await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
  await browser.keys([mod(), "k"]);
  await $('input[placeholder="Type a command or search…"]').waitForDisplayed({ timeout: 5_000 });
  await typeKeys("Session library…");
  await browser.keys("Enter");
  await $('[data-testid="library"]').waitForDisplayed({ timeout: 10_000 });
}

/** The pane's command runs `claude --resume <id>` (shell-quoted in the wrapper). */
const resumes = (s: Spawn) =>
  /['"]?--resume['"]? ['"]?/.test(s.argv.join(" ")) && s.argv.join(" ").includes(CLAUDE_ID);
const rowSel = (sessionId: string) => `[data-testid="library-row"][data-session-id="${sessionId}"]`;
const row = (sessionId: string) => $(rowSel(sessionId));
/** A row's whole text, scrolled into view or not. */
const rowText = (sessionId: string) =>
  browser.execute(
    (s: string) => document.querySelector(s)?.textContent ?? "",
    rowSel(sessionId),
  ) as Promise<string>;
/** Clicks a row's button by test id, found fresh. */
async function rowClick(sessionId: string, testId: string): Promise<void> {
  await browser.waitUntil(
    () =>
      browser.execute(
        (s: string, t: string) => {
          const el = document.querySelector<HTMLButtonElement>(`${s} [data-testid="${t}"]`);
          if (!el || el.disabled) return false;
          el.click();
          return true;
        },
        rowSel(sessionId),
        testId,
      ),
    { timeout: 10_000, timeoutMsg: `${testId} on ${sessionId} wasn't clickable` },
  );
}

// E2E-M18-TH17 Session library (F-TH-17): Claude Code sessions in an
// account's home and in Claude Code's own home, a Codex session in its own
// home and an unreadable file are listed; Resume runs `claude --resume <id>`
// in the session's folder with the account's CLAUDE_CONFIG_DIR; Continue
// opens the handoff sheet with the transcript's brief; search is Pro and
// turning it off deletes the index.
describe("E2E-M18-TH17 session library", function () {
  this.timeout(240_000);
  const repo = `/tmp/tandem-e2e-th17-${Date.now()}`;
  let email = "";
  let accountHome = "";

  before(async () => {
    mkdirSync(repo, { recursive: true });
    execFileSync("git", ["init", "-q", repo]);
    ({ email } = await freshUser("th17"));
    await addFolder(repo);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    // A Claude Code pane makes the default account and its home.
    await split("right", "claude");
    const claude = (await invoke<Account[]>("accounts_list")).find((a) => a.engineId === "claude");
    accountHome = claude?.homePath ?? "";
    expect(accountHome).not.toBe("");
    claudeSession(accountHome, CLAUDE_ID, repo, "Fix the checkout total rounding");
    writeFileSync(join(accountHome, "projects", repo.replace(/[\\/]/g, "-"), "broken.jsonl"), "not json\n");
    claudeSession(
      join(testUserHome(), ".claude"),
      OWN_ID,
      "/tmp/gone-folder-th17",
      "Write the release notes",
    );
    const codexDir = join(testUserHome(), ".codex", "sessions", "2026", "10", "14");
    mkdirSync(codexDir, { recursive: true });
    writeFileSync(
      join(codexDir, `rollout-2026-10-14-${CODEX_ID}.jsonl`),
      [
        { type: "session_meta", payload: { id: CODEX_ID, cwd: repo } },
        {
          type: "response_item",
          payload: {
            type: "message",
            role: "user",
            content: [{ type: "input_text", text: "Speed up the product search" }],
          },
        },
      ]
        .map((l) => JSON.stringify(l))
        .join("\n"),
    );
    writeFileSync(join(testUserHome(), ".codex", "auth.json"), "{}");
  });

  after(() => {
    const user = testUserHome();
    rmSync(join(user, ".claude"), { recursive: true, force: true });
    rmSync(join(user, ".codex"), { recursive: true, force: true });
  });

  it("F-TH-17.1 lists the sessions under their repositories with engine and account, and counts a skipped file", async () => {
    const listing = await invoke<{ sessions: Session[]; skipped: number }>("library_list");
    const ids = listing.sessions.map((s) => s.sessionId);
    expect(ids).toEqual(expect.arrayContaining([CLAUDE_ID, OWN_ID, CODEX_ID]));
    expect(listing.skipped).toBeGreaterThanOrEqual(1);
    const get = (id: string) => listing.sessions.find((s) => s.sessionId === id);
    expect(get(CLAUDE_ID)?.accountId).not.toBeNull();
    expect(get(OWN_ID)?.accountId).toBeNull();
    expect(get(CODEX_ID)?.engineId).toBe("codex");
    await openLibrary();
    await row(CLAUDE_ID).waitForDisplayed({ timeout: 10_000 });
    expect(await rowText(CODEX_ID)).toContain("Outside Vibezeo");
    expect(await $('[data-testid="library-skipped"]').getText()).toMatch(/files couldn't be read/);
    // A session whose folder is gone can't resume.
    expect(await row(OWN_ID).$('[data-testid="library-resume"]').isEnabled()).toBe(false);
  });

  it("F-TH-17.2 Resume runs claude --resume <id> in the session's folder with the account's CLAUDE_CONFIG_DIR", async () => {
    const before = (await invoke<Spawn[]>("test_spawn_log")).length;
    await rowClick(CLAUDE_ID, "library-resume");
    await $('[data-testid="library"]').waitForExist({ reverse: true, timeout: 10_000 });
    const resumed = () =>
      invoke<Spawn[]>("test_spawn_log").then((log) => log.slice(before).find((s) => resumes(s)));
    await browser
      .waitUntil(async () => (await resumed()) !== undefined, { timeout: 20_000 })
      .catch(async () => {
        const log = (await invoke<Spawn[]>("test_spawn_log")).slice(before).map((s) => s.argv.join(" "));
        throw new Error(`no pane resumed the session; spawns since: ${JSON.stringify(log)}`);
      });
    const spawn = (await resumed())!;
    expect(spawn.envKeys).toContain("CLAUDE_CONFIG_DIR");
    expect(spawn.providerHome).toBe(accountHome);
    expect(spawn.cwd).toBe(repo);
  });

  it("F-TH-17.3 Continue with another engine opens the handoff sheet with the transcript's brief", async () => {
    const panes = (await paneIds()).length;
    const spawned = (await invoke<Spawn[]>("test_spawn_log")).length;
    const mark = fakeLog().length;
    await openLibrary();
    await rowClick(CODEX_ID, "library-continue");
    const summary = $('[data-testid="handoff-summary"]');
    await browser.waitUntil(async () => (await summary.getValue()).includes("Speed up the product search"), {
      timeout: 20_000,
      timeoutMsg: "the brief didn't hold the transcript",
    });
    await $('[data-testid="handoff-start"]').click();
    await browser.waitUntil(async () => (await paneIds()).length === panes + 1, { timeout: 20_000 });
    const target = (await paneIds()).at(-1)!;
    await waitForPaneText(target, "Fake Claude Code (test engine)");
    // In the session's folder, with the brief typed as its first prompt.
    const started = (await invoke<Spawn[]>("test_spawn_log")).slice(spawned);
    expect(started.some((s) => s.cwd === repo && s.envKeys.includes("CLAUDE_CONFIG_DIR"))).toBe(true);
    await browser.waitUntil(async () => pastes(mark).some((p) => p.includes("Speed up the product search")), {
      timeout: 60_000,
      timeoutMsg: `first inputs: ${JSON.stringify(pastes(mark))}`,
    });
  });

  it("F-TH-17.4 search is Pro; on, a word from a prompt finds its session; off deletes the index", async () => {
    await expect(invoke("library_search", { query: "checkout" })).rejects.toThrow(/forbidden_plan/);
    await setPlan(email, "pro");
    await waitForEntitlement((e) => e.plan === "pro", "Pro", 30_000);
    await invoke("library_search_set", { on: true });
    const keys = await invoke<string[]>("library_search", { query: "checkout" });
    expect(keys).toHaveLength(1);
    expect(keys[0]).toContain(CLAUDE_ID);
    const index = join(home(), "session-index.db");
    expect(existsSync(index)).toBe(true);
    await invoke("library_search_set", { on: false });
    expect(existsSync(index)).toBe(false);
    await openLibrary();
    expect(await $$('[data-testid="library-search"]').length).toBe(0);
    await browser.keys(Key.Escape);
  });
});
