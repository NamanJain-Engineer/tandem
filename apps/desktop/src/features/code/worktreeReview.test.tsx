import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { DashboardItem, FileDiff, Tab, WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useDashboard } from "../../stores/dashboard";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { ReviewDiff, buildReviewPrompt, useReviews, worktreeAgents, type ReviewComment } from "./WorktreeReview";

const tab: Tab = { id: "tb_wt", workspaceId: "wt_1", name: "Tab 1", sortKey: "a0", createdAt: 0 };
const WT: WorkspaceSummary = {
  id: "wt_1",
  kind: "local",
  hostId: "local",
  root: "/code/shop/.tandem/worktrees/fix-auth",
  wslDistro: null,
  name: "fix-auth",
  parentId: "shop",
  laneBranch: "tandem/fix-auth",
  laneBase: "abc",
  sortKey: "a0",
  pinned: false,
  collapsed: false,
  sessionsCollapsed: false,
  createdAt: 0,
  missing: false,
  branch: "tandem/fix-auth",
  liveCount: 1,
  tabs: [tab],
};

const FILES: FileDiff[] = [
  {
    path: "a.ts",
    oldPath: undefined,
    change: "modified",
    additions: 1,
    deletions: 1,
    binary: false,
    truncated: false,
    patch: "diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -1,2 +1,2 @@\n keep()\n-old()\n+fresh()\n",
  },
];

const agent = (key: string, workspaceId: string, name: string, paneId = key.slice(5)): DashboardItem => ({
  key,
  source: { type: "pane", paneId },
  status: "working",
  reason: null,
  since: 0,
  title: null,
  humanName: name,
  engineId: "claude",
  engineLabel: "Claude Code",
  engineLogo: null,
  accountLabel: null,
  workspaceId,
  workspace: null,
  branch: null,
  preview: null,
  pendingApproval: null,
});

const comment = (over: Partial<ReviewComment>): ReviewComment => ({
  id: 1,
  path: "a.ts",
  side: "new",
  line: 1,
  text: "",
  comment: "",
  ...over,
});

let calls: Call[] = [];
beforeEach(() => {
  resetStores();
  useReviews.setState({ byLane: {}, next: 1 });
  useDashboard.setState({ items: {} });
  calls = fakeBackend({});
});
afterEach(() => setInvokeBackend(null));

function view() {
  return render(
    <UiProvider>
      <ReviewDiff wt={WT} files={FILES} labels={{}} />
    </UiProvider>,
  );
}

describe("review comments to the agent (F-WT-02)", () => {
  it("F-WT-02.1 the prompt lists comments in file order with their quoted lines, branch and count", () => {
    const prompt = buildReviewPrompt("tandem/fix-auth", [
      comment({ id: 1, path: "b.ts", line: 3, text: "  return x;", comment: "Name this." }),
      comment({ id: 2, path: "a.ts", line: 10, text: "if (token == null) return;", comment: "Handle expiry.\nAnd log it." }),
    ]);
    expect(prompt).toBe(
      [
        "Review of your changes on tandem/fix-auth (2 comments):",
        "",
        "1. a.ts:10",
        "   > if (token == null) return;",
        "   Handle expiry.",
        "   And log it.",
        "",
        "2. b.ts:3",
        "   > return x;",
        "   Name this.",
        "",
        "Address each comment, then say what you changed.",
      ].join("\n"),
    );
    const one = buildReviewPrompt("tandem/x", [
      comment({ side: "old", line: 4, text: "x".repeat(300), comment: "Why remove this?" }),
    ]);
    expect(one).toContain("(1 comment):");
    expect(one).toContain("1. a.ts (removed line 4)");
    expect(one).toContain(`   > ${"x".repeat(199)}…`);
  });

  it("F-WT-02.2 comments added on lines are typed into the worktree's agent as one prompt, then cleared", async () => {
    useDashboard.setState({
      items: {
        "pane:pn_1": agent("pane:pn_1", "wt_1", "Ada"),
        "pane:pn_9": agent("pane:pn_9", "elsewhere", "Bo"),
      },
    });
    view();
    const buttons = screen.getAllByTestId("diff-comment-line");
    // Lines: keep() (new 1), old() (old 2), fresh() (new 2).
    expect(buttons.map((b) => b.getAttribute("aria-label"))).toEqual([
      "Comment on a.ts line 1",
      "Comment on a.ts removed line 2",
      "Comment on a.ts line 2",
    ]);
    fireEvent.click(buttons[2] as HTMLElement);
    fireEvent.change(screen.getByTestId("review-comment-input"), { target: { value: "Rename fresh." } });
    fireEvent.click(screen.getByTestId("review-comment-save"));
    fireEvent.click(screen.getAllByTestId("diff-comment-line")[1] as HTMLElement);
    fireEvent.change(screen.getByTestId("review-comment-input"), { target: { value: "Keep old." } });
    fireEvent.click(screen.getByTestId("review-comment-save"));
    expect(screen.getAllByTestId("review-comment")).toHaveLength(2);
    expect(screen.getByTestId("review-bar").textContent).toContain("2 comments");
    fireEvent.click(screen.getByTestId("review-send"));
    await waitFor(() => expect(calls.filter((c) => c.cmd === "pty_type_prompt")).toHaveLength(1));
    const sent = calls.find((c) => c.cmd === "pty_type_prompt")?.args as { paneId: string; text: string };
    expect(sent.paneId).toBe("pn_1");
    expect(sent.text).toContain("Review of your changes on tandem/fix-auth (2 comments):");
    expect(sent.text.indexOf("a.ts:2")).toBeLessThan(sent.text.indexOf("a.ts (removed line 2)"));
    await waitFor(() => expect(screen.queryByTestId("review-bar")).toBeNull());
    expect(useReviews.getState().byLane.wt_1).toBeUndefined();
  });

  it("F-WT-02.3 with no agent running, Send is disabled with the reason and Copy review still works", async () => {
    let copied = "";
    Object.assign(navigator, { clipboard: { writeText: (s: string) => ((copied = s), Promise.resolve()) } });
    view();
    fireEvent.click(screen.getAllByTestId("diff-comment-line")[0] as HTMLElement);
    fireEvent.change(screen.getByTestId("review-comment-input"), { target: { value: "Why keep?" } });
    fireEvent.click(screen.getByTestId("review-comment-save"));
    expect((screen.getByTestId("review-send") as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByTestId("review-no-agent").textContent).toContain("No agent is running in this worktree");
    fireEvent.click(screen.getByTestId("review-copy"));
    expect(copied).toContain("1. a.ts:1");
    expect(calls.some((c) => c.cmd === "pty_type_prompt")).toBe(false);
  });

  it("F-WT-02.1 a comment can be edited and removed before sending", () => {
    view();
    fireEvent.click(screen.getAllByTestId("diff-comment-line")[0] as HTMLElement);
    fireEvent.change(screen.getByTestId("review-comment-input"), { target: { value: "first" } });
    fireEvent.click(screen.getByTestId("review-comment-save"));
    fireEvent.click(screen.getByText("Edit"));
    fireEvent.change(screen.getByTestId("review-comment-input"), { target: { value: "second" } });
    fireEvent.click(screen.getByTestId("review-comment-save"));
    expect(useReviews.getState().byLane.wt_1?.map((c) => c.comment)).toEqual(["second"]);
    fireEvent.click(screen.getByText("Remove"));
    expect(screen.queryByTestId("review-bar")).toBeNull();
    expect(worktreeAgents({}, "wt_1")).toEqual([]);
  });
});
