import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type {
  Routine,
  RoutineInput,
  RoutineView,
  SchedulePreview,
  Teammate,
  TeammateSummary,
} from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useSession } from "../../stores/session";
import { useTeammates } from "../../stores/teammates";
import { FREE, FREE_VIEW, fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { RoutinesPage } from "./RoutinesPage";
import { clockChange, describe as words, time12h } from "./schedule";

const LEAD: Teammate = {
  id: "tm_lead",
  name: "Lead",
  avatarHue: null,
  avatarImage: null,
  brief: "You lead.",
  engineId: "claude",
  model: null,
  effort: null,
  preferredAccountId: null,
  home: "/data/teammates/tm_lead",
  places: [],
  settings: {
    memoryCap: 4096,
    learnMode: "ask",
    mayMessage: true,
    maySchedule: false,
    defaultAccess: "edits",
  },
  isLead: true,
  pinned: false,
  sortKey: "a",
  createdAt: 1,
};

const LEAD_SUMMARY: TeammateSummary = {
  teammate: LEAD,
  chatCapable: true,
  working: 0,
  waiting: false,
  failed: false,
  lastLine: null,
  lastAt: null,
  chats: 0,
};

function routine(over: Partial<Routine> = {}): Routine {
  return {
    id: "rt_1",
    teammateId: "tm_lead",
    name: "Morning summary",
    instruction: "Summarize yesterday's merged work in five bullets.",
    schedule: { schedule: { kind: "cron", expr: "0 9 * * 1-5" }, timezone: "UTC" },
    access: "edits",
    runMode: "in_app",
    paused: false,
    pausedByPlan: false,
    createdByTeammateId: null,
    lastRunChatId: null,
    lastStatus: null,
    lastError: null,
    lastErrorKind: null,
    nextRunAt: Date.now() + 3_600_000,
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

function view(r: Routine): RoutineView {
  return {
    routine: r,
    words: { kind: "weekdays", times: ["09:00"] },
    description: "Weekdays at 9:00 AM",
    running: null,
    today: [],
    last: null,
  };
}

const PREVIEW: SchedulePreview = {
  words: { kind: "weekdays", times: ["09:00"] },
  description: "Weekdays at 9:00 AM",
  next: [Date.UTC(2026, 9, 12, 9), Date.UTC(2026, 9, 13, 9), Date.UTC(2026, 9, 14, 9)],
};

function pro() {
  useSession.setState({
    entitlement: { ...FREE_VIEW, plan: "pro", limits: { ...FREE, routines: true, backgroundRoutines: true } },
  });
}

function backend(
  list: RoutineView[],
  extra: Record<string, (a?: Record<string, unknown>) => unknown> = {},
): Call[] {
  return fakeBackend({
    routines_list: () => list,
    routines_background_status: () => ({ installed: false, kind: "systemd_user", path: null }),
    teammate_list: () => [LEAD_SUMMARY],
    routine_preview_schedule: () => PREVIEW,
    ...extra,
  });
}

const renderPage = () =>
  render(
    <UiProvider>
      <RoutinesPage />
    </UiProvider>,
  );

beforeEach(() => {
  resetStores();
  useTeammates.setState({ list: [LEAD_SUMMARY], loaded: true });
});

describe("routines in words (F-RT-01 AC3)", () => {
  it("F-RT-01.3 says each kind of schedule in words", () => {
    expect(words({ kind: "weekdays", times: ["09:00"] })).toBe("Weekdays at 9:00 AM");
    expect(words({ kind: "weekends", times: ["10:00"] })).toBe("Weekends at 10:00 AM");
    expect(words({ kind: "every_day", times: ["07:30", "18:00"] })).toBe("Every day at 7:30 AM and 6:00 PM");
    expect(words({ kind: "days", days: [1, 3], times: ["12:00"] })).toBe(
      "Mondays and Wednesdays at 12:00 PM",
    );
    expect(words({ kind: "every_minutes", minutes: 360 })).toBe("Every 6 hours");
    expect(words({ kind: "every_minutes", minutes: 60 })).toBe("Every hour");
    expect(words({ kind: "every_minutes", minutes: 30 })).toBe("Every 30 minutes");
    expect(words({ kind: "hourly", minute: 5 })).toBe("Every hour at :05");
    expect(words({ kind: "cron", expr: "0 9 1 * *" })).toBe("Custom schedule (cron 0 9 1 * *)");
    expect(time12h("00:15")).toBe("12:15 AM");
  });

  it("F-RT-01.2 finds the next clock change within 60 days for the DST note", () => {
    const from = Date.UTC(2026, 9, 10, 12);
    const change = clockChange("Europe/London", from);
    expect(change).not.toBeNull();
    expect(new Date(change!).getUTCDate()).toBe(25);
    expect(clockChange("UTC", from)).toBeNull();
  });
});

describe("Routines page (F-RT-04, UI-50..53)", () => {
  it("F-RT-04 Free accounts see the upsell and no New routine", async () => {
    backend([]);
    renderPage();
    expect(await screen.findByText("Routines are part of Pro")).toBeTruthy();
    expect(screen.queryByTestId("routine-new")).toBeNull();
  });

  it("F-RT-04 Pro with no routines: the empty state's suggestion pre-fills the editor", async () => {
    pro();
    backend([]);
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: "Weekly update draft" }));
    const editor = await screen.findByTestId("routine-editor");
    expect((within(editor).getByTestId("routine-name-input") as HTMLInputElement).value).toBe(
      "Weekly update draft",
    );
  });

  it("F-RT-04 a row shows the schedule in words, the next run, the result, and a failure's fix", async () => {
    pro();
    backend([
      view(routine({ lastStatus: "ok" })),
      view(
        routine({
          id: "rt_2",
          name: "Dependency check",
          teammateId: null,
          lastStatus: "error",
          lastError: "The teammate this routine runs as was deleted.",
          lastErrorKind: "teammate_missing",
        }),
      ),
    ]);
    renderPage();
    const rows = await screen.findAllByTestId("routine-row");
    expect(rows).toHaveLength(2);
    const ok = rows.find((r) => r.dataset.id === "rt_1")!;
    expect(within(ok).getByTestId("routine-words").textContent).toBe("Weekdays at 9:00 AM");
    expect(within(ok).getByTestId("routine-next").textContent).toMatch(/^Next in /);
    expect(within(ok).getByTestId("routine-last").textContent).toMatch(/^Ready for review/);
    const failed = rows.find((r) => r.dataset.id === "rt_2")!;
    expect(within(failed).getByTestId("routine-error").textContent).toContain("was deleted");
    expect(within(failed).getByRole("button", { name: "Choose teammate" })).toBeTruthy();
  });

  it("F-RT-04 Run now and Pause call the core", async () => {
    pro();
    const calls = backend([view(routine())], { routine_run_now: () => "run_1", routine_pause: () => null });
    renderPage();
    fireEvent.click(await screen.findByTestId("routine-run-now"));
    await waitFor(() =>
      expect(calls.some((c) => c.cmd === "routine_run_now" && c.args?.id === "rt_1")).toBe(true),
    );
    const row = screen.getByTestId("routine-row");
    row.focus();
    fireEvent.keyDown(row, { key: "p" });
    await waitFor(() =>
      expect(
        calls.some((c) => c.cmd === "routine_pause" && c.args?.id === "rt_1" && c.args?.paused === true),
      ).toBe(true),
    );
  });

  it("F-RT-01.4 the editor's Full access needs the explicit choice before Save", async () => {
    pro();
    let saved: RoutineInput | null = null;
    backend([], {
      routine_save: (a) => {
        saved = a?.input as RoutineInput;
        return routine({ access: "full" });
      },
    });
    renderPage();
    fireEvent.click((await screen.findAllByRole("button", { name: "New routine" }))[0]!);
    const editor = await screen.findByTestId("routine-editor");
    fireEvent.change(within(editor).getByTestId("routine-name-input"), { target: { value: "Cleanup" } });
    fireEvent.change(within(editor).getByLabelText("Instruction"), {
      target: { value: "Delete merged branches older than a week and report which." },
    });
    fireEvent.click(within(editor).getByRole("combobox", { name: "Access" }));
    fireEvent.click(await screen.findByRole("option", { name: "Full access" }));
    expect(await screen.findByText("This routine will act without asking.")).toBeTruthy();
    const save = screen.getByTestId("routine-save") as HTMLButtonElement;
    expect(save.disabled).toBe(true);
    fireEvent.click(screen.getByRole("checkbox", { name: /Let this routine run any command/ }));
    await waitFor(() =>
      expect((screen.getByTestId("routine-save") as HTMLButtonElement).disabled).toBe(false),
    );
    fireEvent.click(screen.getByTestId("routine-save"));
    await waitFor(() => expect(saved).not.toBeNull());
    expect(saved!.access).toBe("full");
    expect(saved!.fullConfirmed).toBe(true);
  }, 15_000); // two menus and a confirm: over 5 s under the full suite's load

  it("F-RT-01 a too-short instruction can't be saved, and Esc on a changed form asks first", async () => {
    pro();
    const calls = backend([]);
    renderPage();
    fireEvent.click((await screen.findAllByRole("button", { name: "New routine" }))[0]!);
    const editor = await screen.findByTestId("routine-editor");
    fireEvent.change(within(editor).getByTestId("routine-name-input"), { target: { value: "Short" } });
    fireEvent.change(within(editor).getByLabelText("Instruction"), { target: { value: "Do it." } });
    expect((screen.getByTestId("routine-save") as HTMLButtonElement).disabled).toBe(true);
    expect(calls.some((c) => c.cmd === "routine_save")).toBe(false);
    fireEvent.keyDown(editor, { key: "Escape" });
    expect(await screen.findByText("Discard changes?")).toBeTruthy();
  });

  it("F-RT-01.3 the schedule preview shows the words, the zone and the next 3 runs", async () => {
    pro();
    backend([]);
    renderPage();
    fireEvent.click((await screen.findAllByRole("button", { name: "New routine" }))[0]!);
    const words = await screen.findByTestId("cron-words");
    expect(words.textContent).toMatch(/^Weekdays at 9:00 AM \(/);
    expect(screen.getByTestId("cron-next").textContent!.split("·")).toHaveLength(3);
  });

  it("F-RT-01 a schedule the core refuses shows its reason instead of a preview", async () => {
    pro();
    backend([], {
      routine_preview_schedule: () => {
        throw {
          code: "invalid",
          message: "Cron needs 5 fields; this has 4.",
          stage: null,
          hint: null,
          retryable: false,
        };
      },
    });
    renderPage();
    fireEvent.click((await screen.findAllByRole("button", { name: "New routine" }))[0]!);
    expect((await screen.findByTestId("cron-error")).textContent).toBe("Cron needs 5 fields; this has 4.");
    expect(screen.queryByTestId("cron-words")).toBeNull();
  });
});

describe("routines a teammate made (F-RT-05)", () => {
  it("F-RT-05 a routine made with the schedule tool says created by {name}", async () => {
    pro();
    backend([view(routine({ createdByTeammateId: "tm_lead" })), view(routine({ id: "rt_2", name: "Mine" }))]);
    renderPage();
    const rows = await screen.findAllByTestId("routine-row");
    const made = rows.find((r) => r.dataset.id === "rt_1")!;
    expect(within(made).getByTestId("routine-created-by").textContent).toBe("created by Lead");
    const mine = rows.find((r) => r.dataset.id === "rt_2")!;
    expect(within(mine).queryByTestId("routine-created-by")).toBeNull();
  });
});

describe("May create routines (F-RT-05)", () => {
  it("F-RT-05 turning the setting on saves maySchedule with the teammate's settings", async () => {
    const { patchOf } = await import("../agents/TeammateSettings");
    const base = {
      name: "Lead",
      avatarHue: null,
      brief: "",
      engineId: "claude",
      model: null,
      preferredAccountId: null,
      places: [],
      defaultAccess: "edits" as const,
      memoryCap: 4096,
      learnMode: "ask" as const,
      mayMessage: true,
      maySchedule: false,
      plugins: [],
    };
    const p = patchOf(LEAD, base, { ...base, maySchedule: true });
    expect(p.settings?.maySchedule).toBe(true);
    expect(p.settings?.mayMessage).toBe(true);
  });
});
