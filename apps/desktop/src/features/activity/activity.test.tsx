import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { ActivityEvent } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useActivity, useCheckpointFocus } from "../../stores/activity";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { ActivityDialog, decidedLabel } from "./ActivityDialog";

beforeEach(() => {
  resetStores();
  useActivity.setState({ open: { source: "pane", id: "pn_1", title: "Claude Code" } });
});
afterEach(() => {
  setInvokeBackend(null);
  useActivity.setState({ open: null });
});

const ev = (over: Partial<ActivityEvent>): ActivityEvent => ({
  kind: "turn",
  at: 1_760_000_000_000,
  summary: "Turn sent",
  checkpointId: null,
  checkpointWorkspace: null,
  commandId: null,
  exitCode: null,
  durationMs: null,
  secrets: [],
  decided: null,
  engine: null,
  ...over,
});

const EVENTS: ActivityEvent[] = [
  ev({
    kind: "files",
    summary: "Folder saved as it was",
    checkpointId: "cp_1",
    checkpointWorkspace: "ws_shop",
  }),
  ev({ kind: "turn", checkpointId: "cp_1" }),
  ev({
    kind: "command",
    summary: "pnpm test",
    exitCode: 0,
    durationMs: 840,
    commandId: 7,
    checkpointId: "cp_1",
  }),
  ev({ kind: "approval", summary: "run `pnpm deploy` with stripe-test", decided: "allowed:inline" }),
  ev({
    kind: "run",
    summary: "pnpm deploy",
    secrets: ["stripe-test"],
    decided: "once",
    exitCode: 0,
    durationMs: 1200,
  }),
];

const dialog = () =>
  render(
    <UiProvider>
      <ActivityDialog />
    </UiProvider>,
  );

describe("the activity ledger (F-EX-07)", () => {
  it("F-EX-07.1 shows the session's events in order with their facts and links", async () => {
    const calls = fakeBackend({ activity_list: () => ({ events: EVENTS, more: false }) });
    dialog();
    await waitFor(() => expect(screen.getAllByTestId("activity-row")).toHaveLength(5));
    expect(screen.getAllByTestId("activity-row").map((r) => r.dataset.kind)).toEqual([
      "files",
      "turn",
      "command",
      "approval",
      "run",
    ]);
    expect(screen.getAllByTestId("activity-facts").map((f) => f.textContent)).toEqual([
      "exit 0 · 840 ms",
      "Allowed in the pane",
      "exit 0 · 1,200 ms · uses stripe-test · You, once",
    ]);
    expect(screen.getAllByTestId("activity-checkpoint")).toHaveLength(3);
    expect(calls.find((c) => c.cmd === "activity_list")?.args).toEqual({
      source: "pane",
      id: "pn_1",
      before: null,
    });
  });

  it("Show earlier pages back, and a checkpoint link opens its own folder on it", async () => {
    const older = ev({ kind: "tool", summary: "Read", at: 1_759_000_000_000 });
    const calls = fakeBackend({
      activity_list: (a) =>
        a?.before == null ? { events: EVENTS, more: true } : { events: [older], more: false },
    });
    dialog();
    fireEvent.click(await screen.findByTestId("activity-earlier"));
    await waitFor(() => expect(screen.getAllByTestId("activity-row")).toHaveLength(6));
    expect(calls.filter((c) => c.cmd === "activity_list").pop()?.args).toMatchObject({
      before: EVENTS[0]?.at,
    });
    expect(screen.queryByTestId("activity-earlier")).toBeNull();
    expect(screen.getAllByTestId("activity-day").length).toBeGreaterThanOrEqual(2);
    fireEvent.click(screen.getAllByTestId("activity-checkpoint")[0] as HTMLElement);
    expect(useCheckpointFocus.getState().focus).toEqual({ workspaceId: "ws_shop", checkpointId: "cp_1" });
    expect(useActivity.getState().open).toBeNull();
  });

  it("F-EX-07.3 Export writes the file and says where", async () => {
    const calls = fakeBackend({
      activity_list: () => ({ events: EVENTS, more: false }),
      activity_export: () => "/home/me/a.jsonl",
    });
    dialog();
    await screen.findAllByTestId("activity-row");
    fireEvent.click(screen.getByTestId("activity-export"));
    expect(await screen.findByText("/home/me/a.jsonl")).toBeTruthy();
    expect(calls.find((c) => c.cmd === "activity_export")?.args).toEqual({ source: "pane", id: "pn_1" });
  });

  it("empty, error with Retry, and Export disabled with nothing to export", async () => {
    let fail = true;
    fakeBackend({
      activity_list: () => {
        if (fail) throw { code: "io", message: "disk" };
        return { events: [], more: false };
      },
    });
    dialog();
    expect((await screen.findByTestId("activity-error")).textContent).toBe("Couldn't read the activity.");
    fail = false;
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("Nothing recorded for this session yet.")).toBeTruthy();
    expect((screen.getByTestId("activity-export") as HTMLButtonElement).disabled).toBe(true);
  });

  it("names who decided", () => {
    expect(decidedLabel("agent")).toBe("You, for this agent");
    expect(decidedLabel("rule")).toBe("A repository rule");
    expect(decidedLabel("allowed_before")).toBe("Allowed earlier for this agent");
    expect(decidedLabel("declined")).toBe("You declined");
    expect(decidedLabel("denied:dashboard")).toBe("Denied from the Dashboard");
    expect(decidedLabel("expired")).toBe("Expired unanswered");
    expect(decidedLabel(null)).toBeNull();
  });
});
