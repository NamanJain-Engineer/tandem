import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  closeAllPanes,
  freshOffline,
  invoke,
  paneIds,
  paneText,
  sendToPane,
  split,
  waitForPaneText,
} from "../support/app";
import { expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";
import { openFromPlus } from "../support/files";

// E2E-M18-EX06 Resource governor (F-EX-06). The test machine has no systemd
// user session, so this covers the unplaced path: usage from each pane's
// process tree, the memory cap refused with the reason, Freeze by signals,
// and "one check at a time". Scopes, caps and the OOM notice are covered by
// the Rust tests against recorded cgroup files (D-2547).

type Status = { grouping: boolean; capping: boolean; freezing: boolean; reason: string | null };
type Usage = { paneId: string; cpuPercent: number | null; memoryBytes: number | null; frozen: boolean };
type Lane = { seat: number; laneId: string | null; check: string };
type Run = { id: string; lanes: Lane[] };

function git(cwd: string, ...args: string[]): string {
  return execFileSync(
    "git",
    [
      "-c",
      "init.defaultBranch=main",
      "-c",
      "user.name=E2E",
      "-c",
      "user.email=e2e@example.invalid",
      "-c",
      "commit.gpgsign=false",
      ...args,
    ],
    { cwd, encoding: "utf8" },
  ).trim();
}

async function press(selector: string): Promise<void> {
  await $(selector).waitForExist({ timeout: 10_000 });
  await browser.execute((sel: string) => document.querySelector<HTMLElement>(sel)?.click(), selector);
}

/** Shows a sidebar tab, opening it only the first time (a second Ports tab
 * would hold a second, hidden Panes list). */
async function show(kind: string, label: string): Promise<void> {
  const pill = `[data-testid="sidebar-pill-${kind}"]`;
  if (await $(pill).isExisting()) await press(pill);
  else await openFromPlus(label);
}

/** The last `tick-N` the pane printed. */
async function lastTick(pane: string): Promise<number> {
  const ticks = [...(await paneText(pane)).matchAll(/tick-(\d+)/g)].map((m) => Number(m[1]));
  return ticks.length ? Math.max(...ticks) : 0;
}

/** Start and end times (ns) of each check, from the log they append to. */
function intervals(log: string): Array<[number, number]> {
  const lines = existsSync(log) ? readFileSync(log, "utf8").trim().split("\n") : [];
  const byLane = new Map<string, { s?: number; e?: number }>();
  for (const l of lines) {
    const [kind, lane, at] = l.split(" ");
    const entry = byLane.get(lane ?? "") ?? {};
    if (kind === "s") entry.s = Number(at);
    if (kind === "e") entry.e = Number(at);
    byLane.set(lane ?? "", entry);
  }
  return [...byLane.values()]
    .filter((e) => e.s !== undefined && e.e !== undefined)
    .map((e) => [e.s ?? 0, e.e ?? 0] as [number, number])
    .sort((a, b) => a[0] - b[0]);
}

describe("E2E-M18-EX06 resource governor", function () {
  this.timeout(240_000);
  const repo = `/tmp/tandem-e2e-m18-ex06-${Date.now()}`;
  let wsId = "";

  before(async function () {
    this.timeout(60_000);
    mkdirSync(repo, { recursive: true });
    writeFileSync(join(repo, "a.txt"), "start\n");
    git(repo, "init", "-q");
    git(repo, "config", "user.name", "E2E");
    git(repo, "config", "user.email", "e2e@example.invalid");
    git(repo, "config", "commit.gpgsign", "false");
    git(repo, "add", ".");
    git(repo, "commit", "-qm", "first");
    await freshOffline();
    await addFolder(repo);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    wsId =
      (await invoke<Array<{ id: string; root: string }>>("workspace_list")).find((w) => w.root === repo)
        ?.id ?? "";
  });

  // Nothing of this spec's outlives it.
  after(async () => {
    await closeAllPanes();
    for (const w of await invoke<Array<{ id: string; parentId: string | null }>>("workspace_list"))
      if (!w.parentId) await invoke("workspace_remove", { id: w.id });
    await invoke("settings_set", { key: "checks_one_at_a_time", value: false });
    rmSync(repo, { recursive: true, force: true });
  });

  it("F-EX-06.1 F-EX-06.2 an agent pane's CPU and memory show within 3 s, and a cap says why it can't apply", async () => {
    const status = await invoke<Status>("resources_status");
    expect(status.grouping).toBe(false);
    expect(status.freezing).toBe(true);
    // No user manager at all, or a session bus without one that can limit memory.
    expect([
      "Memory caps need a systemd user session.",
      "This system's user session can't limit memory.",
    ]).toContain(status.reason);
    // The cap can't be turned on where it can't apply.
    let refused = "";
    await invoke("settings_set", { key: "agent_memory_cap", value: "512" }).catch(
      (e: { message?: string }) => (refused = e.message ?? String(e)),
    );
    expect(refused).toContain(status.reason ?? "");

    const agent = await split("right", "claude");
    await waitForPaneText(agent, "Fake Claude Code");
    await show("ports", "Ports");
    const row = `[data-testid="pane-usage-row"][data-pane="${agent}"]`;
    const started = Date.now();
    await browser.waitUntil(
      async () =>
        (await $(row).isExisting()) &&
        /CPU \d+%/.test(await $(`${row} [data-testid="pane-usage-cpu"]`).getText()) &&
        /\d+ MB/.test(await $(`${row} [data-testid="pane-usage-memory"]`).getText()),
      {
        timeout: 10_000,
        timeoutMsg: `no usage row: ${await browser.execute(() => document.querySelector('[data-testid="pane-usage"]')?.textContent ?? "")}`,
      },
    );
    // Within 3 s of the tab showing: one reading, then the 2 s refresh.
    expect(Date.now() - started).toBeLessThan(3_500);
    const listed = await invoke<Usage[]>("resources_list");
    const mine = listed.find((u) => u.paneId === agent);
    expect(mine?.memoryBytes ?? 0).toBeGreaterThan(0);
    await expectNoSeriousA11yViolationsInBothThemes();

    // Its Dashboard card says the same.
    await show("dashboard", "Dashboard");
    await browser.waitUntil(
      async () =>
        browser.execute(() =>
          Array.from(document.querySelectorAll('[data-testid="agent-card-usage"]')).some((p) =>
            /^CPU (\d+%|—) · \d+ MB$/.test(p.textContent ?? ""),
          ),
        ),
      { timeout: 10_000, timeoutMsg: "no usage on the agent's Dashboard card" },
    );
  });

  it("F-EX-06.4 Freeze stops the pane's job without the shell taking the terminal back; Thaw resumes it", async () => {
    const pane = await split("down");
    await browser.pause(1_000);
    await sendToPane(pane, "i=0; while [ $i -lt 900 ]; do i=$((i+1)); echo tick-$i; sleep 0.2; done\r");
    await browser.waitUntil(async () => (await lastTick(pane)) >= 3, {
      timeout: 15_000,
      timeoutMsg: "no ticks",
    });
    await show("ports", "Ports");
    const row = `[data-testid="pane-usage-row"][data-pane="${pane}"]`;
    await press(`${row} [data-testid="pane-usage-freeze"]`);
    await browser.waitUntil(async () => (await $(row).getAttribute("data-frozen")) === "true", {
      timeout: 10_000,
      timeoutMsg: `never frozen: ${await browser.execute(
        (sel: string) =>
          [
            ...Array.from(document.querySelectorAll(sel)).map((r) => r.outerHTML.slice(0, 300)),
            ...Array.from(document.querySelectorAll("[data-sonner-toast], [role=status]")).map(
              (t) => t.textContent,
            ),
          ].join(" | "),
        row,
      )}`,
    });
    const chip = `[data-testid="pane"][data-pane-id="${pane}"] [data-testid="resource-chip"][data-kind="frozen"]`;
    await $(chip).waitForExist({ timeout: 5_000 });
    await browser.pause(500);
    const at = await lastTick(pane);
    await browser.pause(2_000);
    expect(await lastTick(pane)).toBe(at);
    // No CPU while frozen.
    const frozen = (await invoke<Usage[]>("resources_list")).find((u) => u.paneId === pane);
    expect(frozen?.frozen).toBe(true);
    await press(`${row} [data-testid="pane-usage-freeze"]`);
    await browser.waitUntil(async () => (await lastTick(pane)) > at + 3, {
      timeout: 15_000,
      timeoutMsg: "never resumed",
    });
    expect(await $(chip).isExisting()).toBe(false);
    // The shell never saw its job stop.
    expect(await paneText(pane)).not.toMatch(/Stopped|suspended/);
  });

  it("F-EX-06.5 with one check at a time, three lanes' checks run one after another", async function () {
    this.timeout(180_000);
    // Free includes 4 live panes: the three lanes need room.
    await closeAllPanes();
    const log = join(repo, "..", `ex06-checks-${Date.now()}.log`);
    const check = `echo "s $PWD $(date +%s%N)" >> ${log}; sleep 1; echo "e $PWD $(date +%s%N)" >> ${log}`;
    const run = await invoke<Run>("bestof_start", {
      workspaceId: wsId,
      prompt: "Tidy up",
      seats: [0, 1, 2].map(() => ({ engineId: "claude", accountId: null })),
      checkCommand: check,
    });
    await browser.waitUntil(
      async () =>
        ((await invoke<Run[]>("bestof_list", { workspaceId: wsId }))[0]?.lanes ?? []).filter((l) => l.laneId)
          .length === 3,
      { timeout: 60_000, timeoutMsg: "lanes never started" },
    );

    await invoke("settings_set", { key: "checks_one_at_a_time", value: true });
    await invoke("bestof_check", { runId: run.id });
    let sawQueued = false;
    await browser.waitUntil(
      async () => {
        const lanes = (await invoke<Run[]>("bestof_list", { workspaceId: wsId }))[0]?.lanes ?? [];
        if (lanes.some((l) => l.check === "queued")) sawQueued = true;
        return lanes.every((l) => l.check === "passed");
      },
      { timeout: 60_000, interval: 200, timeoutMsg: "checks never all passed" },
    );
    expect(sawQueued).toBe(true);
    const one = intervals(log);
    expect(one.length).toBe(3);
    for (let i = 1; i < one.length; i++)
      expect(one[i]?.[0] ?? 0).toBeGreaterThanOrEqual(one[i - 1]?.[1] ?? 0);

    // Off: at once.
    rmSync(log, { force: true });
    await invoke("settings_set", { key: "checks_one_at_a_time", value: false });
    await invoke("bestof_check", { runId: run.id });
    await browser.waitUntil(
      async () =>
        ((await invoke<Run[]>("bestof_list", { workspaceId: wsId }))[0]?.lanes ?? []).every(
          (l) => l.check === "passed",
        ) && intervals(log).length === 3,
      { timeout: 60_000, interval: 200, timeoutMsg: "checks never all passed" },
    );
    const all = intervals(log);
    const firstEnd = Math.min(...all.map((i) => i[1]));
    expect(all.every((i) => i[0] < firstEnd)).toBe(true);
    rmSync(log, { force: true });
  });
});
