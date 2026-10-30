import { $, browser } from "@wdio/globals";
import { freshUser, invoke, mod, waitFor } from "../support/app";
import { chatSpawns, lead, makeTeammate } from "../support/agents";
import { expectNoSeriousA11yViolations } from "../support/axe";

type Routine = {
  id: string;
  name: string;
  teammateId: string | null;
  nextRunAt: number | null;
  lastStatus: string | null;
  lastError: string | null;
  lastRunChatId: string | null;
  paused: boolean;
};
type View = { routine: Routine; running: { id: string } | null };
type Run = { id: string; status: string; late: boolean; manual: boolean; chatId: string | null };
type Preview = { next: number[] };

const UTC = "UTC";
const routines = () => invoke<View[]>("routines_list");
const runs = async (id: string) => (await invoke<{ runs: Run[] }>("routine_runs", { id, before: null })).runs;
const tick = (at: number) => invoke<string[]>("test_routines_tick", { at });

async function routine(id: string): Promise<Routine> {
  const found = (await routines()).find((v) => v.routine.id === id);
  if (!found) throw new Error(`no routine ${id}`);
  return found.routine;
}

async function save(name: string, teammateId: string, expr: string, instruction: string): Promise<Routine> {
  return invoke<Routine>("routine_save", {
    input: {
      teammateId,
      name,
      instruction,
      schedule: { schedule: { kind: "cron", expr }, timezone: UTC },
      access: "edits",
      fullConfirmed: false,
      runMode: "in_app",
    },
  });
}

/** Each test's own routines only: a tick fires whatever else is due. */
async function clear(): Promise<void> {
  for (const v of await routines()) await invoke("routine_delete", { id: v.routine.id });
}

async function untilStatus(id: string, status: string, timeout = 30_000): Promise<Routine> {
  return waitFor(
    () => routine(id),
    (r) => r.lastStatus === status,
    `routine never reached ${status}`,
    {
      timeout,
      interval: 250,
    },
  );
}

async function textOf(selector: string): Promise<string> {
  return browser.execute((s) => document.querySelector(s)?.textContent ?? "", selector);
}

async function openRoutines(): Promise<void> {
  await browser.keys([mod(), "1"]);
  const footer = await $('[data-testid="agent-footer-routines"]');
  await footer.waitForDisplayed({ timeout: 10_000 });
  await footer.click();
  await $('[data-testid="routines-page"]').waitForDisplayed({ timeout: 10_000 });
}

// E2E-M11-3 Routine (F-RT-01, F-RT-02, F-RT-04; free, D-2900): the cron builder says
// the schedule in words with the next runs; a due slot fires once into a
// chat named after the routine with a notification; Run now goes Running
// then Ready for review; a paused routine doesn't fire; a deleted
// teammate's routine errors on its row and spawns nothing.
describe("E2E-M11-3 routines", () => {
  let leadId = "";

  before(async function () {
    this.timeout(180_000);
    await freshUser("m11-3");
    leadId = (await lead()).id;
    await openRoutines();
  });

  it("F-RT-01.3 cron 0 9 * * 1-5 says Weekdays at 9:00 AM and skips the weekend; 0 10 * * 6-7 is Saturday and Sunday", async function () {
    this.timeout(60_000);
    await $('[data-testid="routine-new"]').click();
    await $('[data-testid="routine-editor"]').waitForDisplayed({ timeout: 10_000 });
    await $('[data-testid="cron-input"]').$("button=Cron").click();
    const expr = await $('[data-testid="cron-expr"]');
    await expr.waitForDisplayed({ timeout: 5_000 });
    await expr.clearValue();
    await expr.setValue("0 9 * * 1-5");
    await browser.waitUntil(
      async () => (await textOf('[data-testid="cron-words"]')).startsWith("Weekdays at 9:00 AM"),
      {
        timeout: 5_000,
        timeoutMsg: `words: ${await textOf('[data-testid="cron-preview"]')}`,
      },
    );
    expect((await textOf('[data-testid="cron-next"]')).split("·")).toHaveLength(3);
    await expectNoSeriousA11yViolations();
    // The core's next runs, in UTC: never a weekend.
    const weekdays = await invoke<Preview>("routine_preview_schedule", {
      spec: { schedule: { kind: "cron", expr: "0 9 * * 1-5" }, timezone: UTC },
    });
    expect(weekdays.next).toHaveLength(3);
    for (const n of weekdays.next) {
      const d = new Date(n);
      expect([1, 2, 3, 4, 5]).toContain(d.getUTCDay());
      expect(d.getUTCHours()).toBe(9);
    }
    await expr.clearValue();
    await expr.setValue("0 10 * * 6-7");
    await browser.waitUntil(
      async () => (await textOf('[data-testid="cron-words"]')).startsWith("Weekends at 10:00 AM"),
      {
        timeout: 5_000,
        timeoutMsg: `words: ${await textOf('[data-testid="cron-preview"]')}`,
      },
    );
    const weekends = await invoke<Preview>("routine_preview_schedule", {
      spec: { schedule: { kind: "cron", expr: "0 10 * * 6-7" }, timezone: UTC },
    });
    const days = new Set(weekends.next.map((n) => new Date(n).getUTCDay()));
    expect([...days].sort()).toEqual([0, 6]);
    // Esc on a changed form asks first; Discard closes it.
    await browser.keys("Escape");
    await $("button=Discard").click();
    await $('[data-testid="routine-editor"]').waitForExist({ timeout: 5_000, reverse: true });
  });

  it("F-RT-02.1 at 09:00 the routine fires once: a chat named after it and the notification", async function () {
    this.timeout(90_000);
    await clear();
    const r = await save(
      "Morning summary",
      leadId,
      "0 9 * * *",
      "Summarize yesterday's work in five short bullets.",
    );
    expect(r.nextRunAt).toBeTruthy();
    expect(new Date(r.nextRunAt!).getUTCHours()).toBe(9);
    const before = (await chatSpawns()).length;
    // The scheduler's clock reaches 09:00 (a few seconds past).
    expect(await tick(r.nextRunAt! + 5_000)).toHaveLength(1);
    expect(await tick(r.nextRunAt! + 35_000)).toHaveLength(0);
    const done = await untilStatus(r.id, "ok");
    expect((await chatSpawns()).length).toBe(before + 1);
    expect(await runs(r.id)).toHaveLength(1);
    const chats = await invoke<Array<{ id: string; title: string | null }>>("thread_list", {
      kind: "teammate_chat",
      teammateId: leadId,
      includeArchived: false,
    });
    expect(chats.find((c) => c.id === done.lastRunChatId)?.title).toBe("Morning summary");
    const notes = await invoke<{ items: Array<{ title: string }> }>("notifications_list", {});
    expect(notes.items.some((n) => n.title === "Ready for review — Morning summary")).toBe(true);
    // The row says it.
    await browser.waitUntil(
      async () =>
        (
          await textOf(`[data-testid="routine-row"][data-id="${r.id}"] [data-testid="routine-last"]`)
        ).startsWith("Ready for review"),
      { timeout: 10_000, timeoutMsg: "the row never said Ready for review" },
    );
  });

  it("F-RT-04.1 Run now: Running, then Ready for review", async function () {
    this.timeout(90_000);
    await clear();
    // The fake engine streams for ~4 s (`/slow`), so Running shows.
    const r = await save(
      "Slow check",
      leadId,
      "0 7 * * *",
      "/slow 30\nCheck the build output and list the warnings.",
    );
    const row = `[data-testid="routine-row"][data-id="${r.id}"]`;
    await $(row).waitForDisplayed({ timeout: 10_000 });
    await $(`${row} [data-testid="routine-run-now"]`).click();
    await browser.waitUntil(async () => (await textOf(`${row} [data-testid="routine-last"]`)) === "Running", {
      timeout: 10_000,
      timeoutMsg: `never Running: ${await textOf(row)}`,
    });
    await browser.waitUntil(
      async () => (await textOf(`${row} [data-testid="routine-last"]`)).startsWith("Ready for review"),
      {
        timeout: 30_000,
        timeoutMsg: `never Ready for review: ${await textOf(row)}`,
      },
    );
    const all = await runs(r.id);
    expect(all).toHaveLength(1);
    expect(all[0]!.manual).toBe(true);
    expect(all[0]!.status).toBe("ok");
    await expectNoSeriousA11yViolations();
  });

  it("F-RT-04.2 a paused routine doesn't fire; a deleted teammate's routine errors on its row and spawns nothing", async function () {
    this.timeout(90_000);
    await clear();
    const paused = await save(
      "Paused one",
      leadId,
      "30 8 * * *",
      "Write a short note about today's open tasks.",
    );
    const due = paused.nextRunAt!;
    // Pause from the row (P).
    const row = `[data-testid="routine-row"][data-id="${paused.id}"]`;
    await $(row).waitForDisplayed({ timeout: 10_000 });
    await browser.execute((s) => (document.querySelector(s) as HTMLElement | null)?.focus(), row);
    await browser.keys("p");
    await browser.waitUntil(async () => (await routine(paused.id)).paused, {
      timeout: 5_000,
      timeoutMsg: "not paused",
    });
    const spawns = (await chatSpawns()).length;
    expect(await tick(due + 5_000)).toHaveLength(0);
    expect(await runs(paused.id)).toHaveLength(0);

    const scout = await makeTeammate("Scout");
    const orphan = await save(
      "Orphaned",
      scout.id,
      "45 8 * * *",
      "Look for flaky tests in the last week's runs.",
    );
    await invoke("teammate_delete", { id: scout.id });
    expect(await tick(orphan.nextRunAt! + 5_000)).toHaveLength(1);
    const failed = await untilStatus(orphan.id, "error");
    expect(failed.lastError).toContain("deleted");
    expect((await chatSpawns()).length).toBe(spawns);
    const orphanRow = `[data-testid="routine-row"][data-id="${orphan.id}"]`;
    await browser.waitUntil(
      async () => (await textOf(`${orphanRow} [data-testid="routine-error"]`)).includes("deleted"),
      {
        timeout: 10_000,
        timeoutMsg: `no error line: ${await textOf(orphanRow)}`,
      },
    );
    expect(await $(orphanRow).$("button=Choose teammate").isExisting()).toBe(true);
  });
});
