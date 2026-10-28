import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { ReviewItem } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { setInvokeBackend } from "../../ipc/invoke";
import { followReview, sortItems, useReview } from "../../stores/review";
import { useWorktrees } from "../../stores/worktrees";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { DashboardPanel, ReviewQueue } from "./ReviewQueue";

const item = (over: Partial<ReviewItem>): ReviewItem => ({
  key: "pane:p1",
  source: { type: "pane", paneId: "p1" },
  title: "Ada",
  engineId: "claude",
  workspaceId: "wt_1",
  workspace: "fix-auth",
  branch: "tandem/fix-auth",
  laneId: "wt_1",
  finishedAt: 1_000,
  closing: "Done: handled expired tokens.",
  files: 2,
  additions: 10,
  deletions: 3,
  risks: [],
  check: null,
  diffHash: "h1",
  state: "review",
  snoozedUntil: null,
  turnCheckpointId: "cp_1",
  ...over,
});

const risky = item({
  key: "thread:t1",
  source: { type: "thread", threadId: "t1" },
  title: "Grace",
  laneId: null,
  finishedAt: 2_000,
  risks: [
    { rule: "migrations", files: ["db/migrations/0042.sql"] },
    { rule: "deleted_tests", files: ["tests/auth.test.ts"] },
  ],
});

const wrap = (ui: React.ReactNode) => render(<UiProvider>{ui}</UiProvider>);
const titles = () => screen.getAllByTestId("review-item-title").map((e) => e.textContent);

beforeEach(() => {
  resetStores();
  useReview.setState({ items: [], overlaps: [], loaded: false, error: null, view: "agents", sort: "risk" });
  useWorktrees.setState({ ask: null });
});
afterEach(() => setInvokeBackend(null));

describe("Review queue (F-DB-04)", () => {
  it("F-DB-04.6 empty and error states", async () => {
    fakeBackend({ review_queue: () => ({ items: [] }) });
    const { unmount } = wrap(<ReviewQueue />);
    await waitFor(() =>
      expect(screen.getByTestId("review-empty").textContent).toContain("Nothing to review"),
    );
    unmount();
    useReview.setState({ loaded: false });
    fakeBackend({
      review_queue: () => {
        throw { code: "unavailable", message: "The review queue is starting" };
      },
    });
    wrap(<ReviewQueue />);
    await waitFor(() => expect(screen.getByTestId("review-error").textContent).toContain("starting"));
  });

  it("F-DB-04.6 F-DB-04.2 sorting by risk puts the item with more rules first, and labels name their files", async () => {
    const calls = fakeBackend({ review_queue: () => ({ items: [item({}), risky] }) });
    wrap(<ReviewQueue />);
    await waitFor(() => expect(screen.getAllByTestId("review-item")).toHaveLength(2));
    expect(calls.find((c) => c.cmd === "review_queue")?.args).toEqual({ refresh: true });
    expect(titles()).toEqual(["Grace", "Ada"]);
    const rules = screen
      .getAllByTestId("review-risk")
      .map((e) => [e.getAttribute("data-rule"), e.getAttribute("title")]);
    expect(rules).toEqual([
      ["migrations", "db/migrations/0042.sql"],
      ["deleted_tests", "tests/auth.test.ts"],
    ]);
    expect(screen.getByText("Touches migrations")).toBeTruthy();
    act(() => useReview.getState().setSort("time"));
    expect(titles()).toEqual(["Ada", "Grace"]);
    expect(sortItems([risky, item({})], "time").map((i) => i.title)).toEqual(["Ada", "Grace"]);
  });

  it("F-TK-05.1 a claim overlap shows above the queue, naming both worktrees and the paths", async () => {
    const overlap = {
      repo: "/r",
      folders: ["/r/.tandem/worktrees/add-oauth", "/r/.tandem/worktrees/fix-auth"],
      agents: ["add-oauth", "fix-auth"],
      paths: ["src/auth/session.ts", "a", "b", "c", "d", "e"],
    };
    fakeBackend({ review_queue: () => ({ items: [], overlaps: [overlap] }) });
    const stop = followReview();
    wrap(<ReviewQueue />);
    await waitFor(() => expect(screen.getByTestId("review-overlaps")).toBeTruthy());
    const box = screen.getByTestId("review-overlaps").textContent ?? "";
    expect(box).toContain("add-oauth and fix-auth both claim");
    expect(box).toContain("src/auth/session.ts");
    expect(box).toContain("and 1 more");
    expect(screen.getByTestId("review-empty").textContent).toContain("Nothing to review");
    act(() => emitLocal("review:queue", { items: [item({})], overlaps: [] }));
    expect(screen.queryByTestId("review-overlaps")).toBeNull();
    stop();
  });

  it("F-DB-04.3 approve sends the hash the user saw; a changed item says so", async () => {
    const calls = fakeBackend({
      review_queue: () => ({ items: [item({ state: "changed" })] }),
      review_approve: () => item({ state: "approved" }),
    });
    const stop = followReview();
    wrap(<ReviewQueue />);
    await waitFor(() =>
      expect(screen.getByTestId("review-changed").textContent).toBe("Changed since you approved"),
    );
    // S4: Approve waits for the item to have held still a moment.
    const approve = screen.getByTestId("review-approve") as HTMLButtonElement;
    expect(approve.disabled).toBe(true);
    fireEvent.click(approve);
    expect(calls.some((c) => c.cmd === "review_approve")).toBe(false);
    await waitFor(() => expect(approve.disabled).toBe(false));
    fireEvent.click(approve);
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "review_approve")?.args).toEqual({ key: "pane:p1", diffHash: "h1" }),
    );
    act(() => emitLocal("review:queue", { items: [item({ state: "approved" })] }));
    expect(screen.getByTestId("review-item").getAttribute("data-state")).toBe("approved");
    expect(screen.getByTestId("review-approve").textContent).toBe("Unapprove");
    expect(screen.getByRole("region", { name: "Approved" })).toBeTruthy();
    stop();
  });

  it("F-DB-04.4 a worktree item's Send back opens its diff and review bar, Merge its confirm", async () => {
    fakeBackend({ review_queue: () => ({ items: [item({}), risky] }) });
    wrap(<ReviewQueue />);
    await waitFor(() => expect(screen.getAllByTestId("review-item")).toHaveLength(2));
    // A plain folder has no worktree actions.
    expect(screen.getAllByTestId("review-send-back")).toHaveLength(1);
    fireEvent.click(screen.getByTestId("review-send-back"));
    expect(useWorktrees.getState().ask).toEqual({ kind: "diff", worktreeId: "wt_1" });
    fireEvent.click(screen.getByTestId("review-merge"));
    expect(useWorktrees.getState().ask).toEqual({ kind: "merge", worktreeId: "wt_1" });
    fireEvent.click(screen.getByTestId("review-opinion"));
    expect(useWorktrees.getState().ask).toEqual({ kind: "opinion", worktreeId: "wt_1" });
  });

  it("F-DB-04.5 the check shows Passed, or Failed with its exit code and output", async () => {
    fakeBackend({
      review_queue: () => ({
        items: [
          item({ check: { state: "passed", exit: 0, output: "" } }),
          { ...risky, check: { state: "failed", exit: 1, output: "1 test failed" } },
        ],
      }),
    });
    wrap(<ReviewQueue />);
    await waitFor(() => expect(screen.getAllByTestId("review-check")).toHaveLength(2));
    const checks = screen.getAllByTestId("review-check").map((e) => e.textContent);
    expect(checks.some((c) => c?.includes("Check passed"))).toBe(true);
    expect(checks.some((c) => c?.includes("Check failed (exit 1)") && c.includes("1 test failed"))).toBe(
      true,
    );
  });

  it("F-DB-04 the Last turn and Open diff read the item's diff", async () => {
    const calls = fakeBackend({
      review_queue: () => ({ items: [item({})] }),
      review_item_diff: () => [],
    });
    wrap(<ReviewQueue />);
    await waitFor(() => expect(screen.getByTestId("review-turn")).toBeTruthy());
    fireEvent.click(screen.getByTestId("review-turn"));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "review_item_diff")?.args).toEqual({
        key: "pane:p1",
        scope: "turn",
      }),
    );
  });

  it("F-DB-04 the Dashboard panel switches to Review with its count", async () => {
    fakeBackend({
      review_queue: () => ({ items: [item({}), item({ key: "pane:p2", state: "approved" })] }),
      status_snapshot: () => [],
    });
    wrap(<DashboardPanel />);
    await waitFor(() => expect(screen.getByText("Review · 1")).toBeTruthy());
    fireEvent.click(screen.getByText("Review · 1"));
    await waitFor(() => expect(screen.getByTestId("review-queue")).toBeTruthy());
  });
});
