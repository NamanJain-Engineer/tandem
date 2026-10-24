import { $, browser } from "@wdio/globals";
import { freshUser, invoke, mod } from "../support/app";
import { lead } from "../support/agents";

type Routine = { id: string; nextRunAt: number | null; lastStatus: string | null };
type Run = { id: string; status: string; late: boolean; dueAt: number };

const runs = async (id: string) => (await invoke<{ runs: Run[] }>("routine_runs", { id, before: null })).runs;
const tick = (at: number) => invoke<string[]>("test_routines_tick", { at });

async function save(name: string, teammateId: string, expr: string): Promise<Routine> {
  return invoke<Routine>("routine_save", {
    input: {
      teammateId,
      name,
      instruction: "Summarize what changed overnight in three bullets.",
      schedule: { schedule: { kind: "cron", expr }, timezone: "UTC" },
      access: "edits",
      fullConfirmed: false,
      runMode: "in_app",
    },
  });
}

// E2E-M11-4 Missed run (F-RT-02 AC2): the app was closed across a due time
// and opened again within 24 h. The scheduler's first passes after opening
// (at the reopening time, through `test_routines_tick`) give exactly one late
// run; a slot missed by more than 24 h gives none.
describe("E2E-M11-4 missed run", () => {
  before(async function () {
    this.timeout(180_000);
    await freshUser("m11-4");
  });

  it("F-RT-02.2 closed across the due time, reopened 2 h later: exactly one late run", async function () {
    this.timeout(90_000);
    const r = await save("Overnight digest", (await lead()).id, "0 6 * * *");
    const due = r.nextRunAt!;
    const reopened = due + 2 * 3_600_000;
    // The first pass after opening, then the next ones 30 s apart.
    expect(await tick(reopened)).toHaveLength(1);
    expect(await tick(reopened + 30_000)).toHaveLength(0);
    expect(await tick(reopened + 60_000)).toHaveLength(0);
    const all = await runs(r.id);
    expect(all).toHaveLength(1);
    expect(all[0]!.late).toBe(true);
    expect(all[0]!.dueAt).toBe(due);
    await browser.waitUntil(
      async () =>
        (await invoke<Array<{ routine: Routine }>>("routines_list")).find((v) => v.routine.id === r.id)
          ?.routine.lastStatus === "ok",
      { timeout: 30_000, timeoutMsg: "the late run never finished" },
    );
    // Run history says why it ran.
    await browser.keys([mod(), "1"]);
    await $('[data-testid="agent-footer-routines"]').click();
    const row = `[data-testid="routine-row"][data-id="${r.id}"]`;
    await $(row).waitForDisplayed({ timeout: 10_000 });
    await browser.execute((s) => (document.querySelector(s) as HTMLElement | null)?.focus(), row);
    await browser.keys("h");
    await $('[data-testid="run-history"]').waitForDisplayed({ timeout: 10_000 });
    await browser.waitUntil(
      async () =>
        (
          await browser.execute(() => document.querySelector('[data-testid="run-row"]')?.textContent ?? "")
        ).includes("Caught up after Vibezeo opened"),
      { timeout: 10_000, timeoutMsg: "the run's trigger wasn't shown" },
    );
    await browser.keys("Escape");
  });

  it("F-RT-02.2 a slot missed by more than 24 h isn't caught up", async function () {
    this.timeout(60_000);
    for (const v of await invoke<Array<{ routine: { id: string } }>>("routines_list")) {
      await invoke("routine_delete", { id: v.routine.id });
    }
    const r = await save("Stale digest", (await lead()).id, "15 5 * * *");
    const due = r.nextRunAt!;
    expect(await tick(due + 25 * 3_600_000)).toHaveLength(0);
    expect(await runs(r.id)).toHaveLength(0);
    // It's scheduled again from then, not lost.
    const after = (await invoke<Array<{ routine: Routine }>>("routines_list")).find(
      (v) => v.routine.id === r.id,
    )!.routine;
    expect(after.nextRunAt).toBeGreaterThan(due + 25 * 3_600_000);
  });
});
