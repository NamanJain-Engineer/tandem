import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { OrphanInfo } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { OrphansDialog } from "./OrphansDialog";

const ORPHAN: OrphanInfo = {
  paneId: "pn_1",
  exe: "bash",
  cwd: "/code/shop",
  startedAt: Date.UTC(2026, 0, 1, 9, 30),
  pids: [101, 102],
};

function setup(list: OrphanInfo[]) {
  const calls = fakeBackend({ orphans_list: () => list, orphans_resolve: () => null });
  render(
    <UiProvider>
      <OrphansDialog />
    </UiProvider>,
  );
  return calls;
}

beforeEach(resetStores);
afterEach(() => setInvokeBackend(null));

describe("orphaned processes (F-CD-03 AC8, UI-132)", () => {
  it("F-CD-03.8 lists what's still running and Stop ends it", async () => {
    const calls = setup([ORPHAN, { ...ORPHAN, paneId: "pn_2", exe: "claude", pids: [200] }]);
    expect(
      await screen.findByText("2 agent processes from your last session are still running"),
    ).toBeTruthy();
    expect(screen.getByText("claude")).toBeTruthy();
    expect(screen.getAllByText("/code/shop")).toHaveLength(2);
    fireEvent.click(screen.getByTestId("orphans-stop"));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "orphans_resolve")?.args).toEqual({ action: "stop" }),
    );
    await waitFor(() => expect(screen.queryByTestId("orphans")).toBeNull());
    expect(await screen.findByText("Stopped 3 processes")).toBeTruthy();
  });

  it("F-CD-03.8 Keep them leaves the processes alone", async () => {
    const calls = setup([ORPHAN]);
    expect(await screen.findByText("1 agent process from your last session is still running")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Keep them" }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "orphans_resolve")?.args).toEqual({ action: "keep" }),
    );
  });

  it("shows nothing after a clean exit", async () => {
    const calls = setup([]);
    await waitFor(() => expect(calls.some((c) => c.cmd === "orphans_list")).toBe(true));
    expect(screen.queryByTestId("orphans")).toBeNull();
  });
});
