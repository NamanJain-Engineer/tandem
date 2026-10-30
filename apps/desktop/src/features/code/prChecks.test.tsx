import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  DashboardItem,
  FileDiff,
  ImportedComment,
  LaneChecks,
  PrCheck,
  Tab,
  WorkspaceSummary,
} from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useDashboard } from "../../stores/dashboard";
import { useLayout } from "../../stores/layout";
import { useStatus } from "../../stores/status";
import { useWorkspaces } from "../../stores/workspaces";
import {
  CHECKS_AFTER_PUSH_MS,
  CHECKS_IDLE_MS,
  checksDue,
  loadChecks,
  pollChecks,
  useWorktrees,
} from "../../stores/worktrees";
import { gitKey, useGit } from "../../stores/git";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { ReviewDiff, buildReviewPrompt, useReviews } from "./WorktreeReview";
import { checksState } from "./PullRequestChecks";
import { WorktreeDialogs, WorktreeRow, worktreeMenu } from "./Worktrees";

const tab = (ws: string): Tab => ({
  id: `tb_${ws}`,
  workspaceId: ws,
  name: "Tab 1",
  sortKey: "a0",
  createdAt: 0,
});
const REPO = {
  id: "shop",
  kind: "local",
  root: "/code/shop",
  name: "shop",
  parentId: null,
  laneBranch: null,
  missing: false,
  branch: "main",
  liveCount: 0,
  tabs: [tab("shop")],
} as unknown as WorkspaceSummary;
const WT = {
  ...REPO,
  id: "wt_1",
  parentId: "shop",
  laneBranch: "tandem/fix",
  name: "fix",
  tabs: [tab("wt_1")],
} as WorkspaceSummary;

const READY: LaneChecks = {
  laneId: "wt_1",
  state: "ready",
  pr: { number: 7, url: "https://github.com/o/r/pull/7" },
  checks: [
    {
      name: "test",
      bucket: "fail",
      link: "https://github.com/o/r/actions/runs/1/job/2",
      workflow: "CI",
      jobId: "2",
    },
    { name: "lint", bucket: "pass", link: null, workflow: "CI", jobId: null },
  ],
  error: null,
};
const PROMPT =
  "A CI check failed on this worktree's pull request #7.\n\n> Untrusted content from the CI check\n````text\nCheck: test\n````";

const COMMENTS: ImportedComment[] = [
  {
    id: "101",
    path: "a.ts",
    line: 2,
    side: "new",
    author: "ana",
    body: "Rename this",
    fenced:
      "> Untrusted content from a pull request review comment\n```text\nana on a.ts:2:\nRename this\n```",
  },
];

const agent = (paneId: string, workspaceId: string): DashboardItem => ({
  key: `pane:${paneId}`,
  source: { type: "pane", paneId },
  status: "working",
  reason: null,
  since: 0,
  title: null,
  humanName: "Ada",
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

let calls: Call[] = [];
const named = (cmd: string) => calls.filter((c) => c.cmd === cmd);
let checks: LaneChecks = READY;
let pushFails = false;

beforeEach(() => {
  resetStores();
  checks = READY;
  pushFails = false;
  useWorktrees.setState({ checks: {}, ask: null });
  useDashboard.setState({ items: {} });
  useReviews.setState({ byLane: {}, next: 1, importedIds: {} });
  calls = fakeBackend({
    workspace_list: () => [REPO, WT],
    lane_checks: () => checks,
    lane_check_failure: () => PROMPT,
    pr_checks_allow: (a) => {
      checks = a?.allow ? READY : { ...READY, state: "not_allowed", pr: null, checks: [] };
      return null;
    },
    lane_review_comments: () => ({ comments: COMMENTS, outdated: 1, cut: 0 }),
    pty_type_prompt: () => null,
    lane_forge: () => "github",
    lane_runtime: () => null,
    git_push: () => {
      if (pushFails)
        throw { code: "io", message: "rejected: fetch first", stage: "git", hint: null, retryable: false };
      return null;
    },
  });
  useWorkspaces.setState({ list: [REPO, WT], activeId: "shop", activeTab: {} });
});
afterEach(() => setInvokeBackend(null));

function withDialogs(node: React.ReactNode) {
  return render(
    <UiProvider>
      {node}
      <WorktreeDialogs />
    </UiProvider>,
  );
}

describe("CI results and review comments (F-WT-08)", () => {
  it("F-WT-08.1 the row says CI failed (1 check failed: test as its name and title), and Send failure to agent types the core's fenced prompt on a click", async () => {
    useDashboard.setState({ items: { "pane:pn_1": agent("pn_1", "wt_1") } });
    withDialogs(<WorktreeRow wt={WT} base="main" />);
    await act(() => loadChecks(["wt_1"]));
    const badge = await screen.findByTestId("worktree-checks");
    expect(badge.dataset.state).toBe("failed");
    // D-2700: the status bar's words; its name starts with them (WCAG 2.5.3).
    expect(badge.textContent).toBe("CI failed");
    expect(badge.getAttribute("aria-label")).toBe("CI failed: 1 check failed: test");
    expect(badge.title).toBe("1 check failed: test");
    // F-WT-08.4: reading checks never sends anything.
    expect(named("pty_type_prompt")).toHaveLength(0);
    expect(named("lane_check_failure")).toHaveLength(0);
    fireEvent.click(badge);
    const rows = await screen.findAllByTestId("check-row");
    expect(rows.map((r) => [r.dataset.name, r.dataset.bucket])).toEqual([
      ["test", "fail"],
      ["lint", "pass"],
    ]);
    expect(within(rows[1]!).queryByTestId("check-send")).toBeNull();
    fireEvent.click(within(rows[0]!).getByTestId("check-send"));
    await waitFor(() => expect(named("pty_type_prompt")).toHaveLength(1));
    expect(named("lane_check_failure")[0]?.args).toEqual({ laneId: "wt_1", name: "test" });
    expect(named("pty_type_prompt")[0]?.args).toEqual({ paneId: "pn_1", text: PROMPT });
  });

  it("F-WT-08.1 D-2614 D-2700 beside the row's status the badge is its icon alone, its name and title kept", async () => {
    // The worktree's agent is working: its row says "● 1 working" (D-2603).
    useLayout.setState({
      tabs: { tb_wt_1: { tree: null, active: null, maximized: null, panes: { p1: {} as never }, keepEmpty: false } },
    });
    useStatus.setState({
      records: {
        "pane:p1": { key: "pane:p1", status: "working", reason: null, since: 1, ts: 1, tool: null, title: null, preview: null },
      },
    });
    withDialogs(<WorktreeRow wt={WT} base="main" />);
    await act(() => loadChecks(["wt_1"]));
    const badge = await screen.findByRole("button", { name: "CI failed: 1 check failed: test" });
    expect(badge.dataset.testid).toBe("worktree-checks");
    expect(badge.hasAttribute("data-compact")).toBe(true);
    expect(badge.textContent).toBe("");
    expect(badge.querySelector("svg")).not.toBeNull();
    expect(badge.title).toBe("1 check failed: test");
    // With nothing beside it, its words show whole, never cut short.
    act(() => useStatus.setState({ records: {} }));
    expect(badge.hasAttribute("data-compact")).toBe(false);
    expect(badge.textContent).toBe("CI failed");
    expect(badge.querySelector(".truncate")).toBeNull();
    fireEvent.click(badge);
    expect(useWorktrees.getState().ask).toEqual({ kind: "checks", worktreeId: "wt_1" });
  });

  it("F-WT-08.1 D-2700 the badge says the status bar's words in every state, its title the checks'", async () => {
    const states: Array<[PrCheck["bucket"], string, string]> = [
      ["pending", "CI running", "Checks running"],
      ["cancel", "CI cancelled", "Checks cancelled"],
      ["pass", "CI passed", "Checks passed"],
    ];
    for (const [bucket, words, title] of states) {
      checks = { ...READY, checks: [{ ...READY.checks[0]!, bucket }] };
      const { unmount } = withDialogs(<WorktreeRow wt={WT} base="main" />);
      await act(() => loadChecks(["wt_1"]));
      const badge = await screen.findByRole("button", { name: words });
      expect(badge.textContent).toBe(words);
      expect(badge.title).toBe(title);
      unmount();
    }
  });

  it("F-WT-08.1 D-2700 while a row's contents overflow it, its marks fold to their icons, the radar mark first, then the badge", async () => {
    useWorktrees.setState({ radar: { wt_1: ["applyCoupon"] } });
    // The row's contents: 120 px, and 80 more for each mark that shows its words.
    let room = 200;
    const row = (el: Element) => el instanceof HTMLElement && el.dataset.testid === "worktree-row";
    const scroll = vi
      .spyOn(Element.prototype, "scrollWidth", "get")
      .mockImplementation(function (this: Element) {
        return row(this) ? 120 + 80 * this.querySelectorAll("[data-words]").length : 0;
      });
    const client = vi
      .spyOn(Element.prototype, "clientWidth", "get")
      .mockImplementation(function (this: Element) {
        return row(this) ? room : 0;
      });
    try {
      withDialogs(<WorktreeRow wt={WT} base="main" />);
      await act(() => loadChecks(["wt_1"]));
      const badge = await screen.findByTestId("worktree-checks");
      const radar = screen.getByTestId("worktree-radar");
      // Room for one mark's words: the radar mark folds, the badge keeps "CI failed".
      expect(radar.hasAttribute("data-compact")).toBe(true);
      expect(radar.getAttribute("aria-label")).toBe("applyCoupon, shared with another worktree");
      expect(badge.textContent).toBe("CI failed");
      // New contents are measured again from nothing folded: room for both now.
      room = 400;
      act(() => useWorktrees.setState({ radar: { wt_1: ["applyCoupon", "priceOf"] } }));
      expect(radar.textContent).toBe("2 symbols");
      expect(badge.textContent).toBe("CI failed");
      // Room for neither: both are their icons, never their words cut short.
      room = 150;
      act(() => useWorktrees.setState({ radar: { wt_1: ["applyCoupon"] } }));
      expect(radar.textContent).toBe("");
      expect(badge.textContent).toBe("");
      expect(badge.getAttribute("aria-label")).toBe("CI failed: 1 check failed: test");
    } finally {
      scroll.mockRestore();
      client.mockRestore();
    }
  });

  it("F-WT-08.3 Check pull request asks first; Not now runs nothing, Allow allows the repository and shows the checks", async () => {
    checks = { ...READY, state: "not_allowed", pr: null, checks: [] };
    withDialogs(null);
    const menu = () => worktreeMenu(WT, "main", () => 0);
    const select = (id: string) => {
      const e = menu().find((x) => "id" in x && x.id === id);
      if (e && "onSelect" in e) e.onSelect?.();
    };
    act(() => select("wt-checks"));
    expect(await screen.findByTestId("checks-ask")).toBeTruthy();
    expect(screen.getByTestId("checks-ask").textContent).toContain(
      "gh signs itself in; Vibezeo never sees its token.",
    );
    fireEvent.click(screen.getByTestId("checks-not-now"));
    await waitFor(() => expect(screen.queryByTestId("checks-ask")).toBeNull());
    expect(named("pr_checks_allow")).toHaveLength(0);
    // Not allowed: no "Stop checking" entry.
    expect(menu().some((x) => "id" in x && x.id === "wt-checks-forget")).toBe(false);
    act(() => select("wt-checks"));
    fireEvent.click(await screen.findByTestId("checks-allow"));
    await waitFor(() =>
      expect(named("pr_checks_allow")[0]?.args).toEqual({ workspaceId: "wt_1", allow: true }),
    );
    expect((await screen.findByTestId("checks")).dataset.state).toBe("ready");
    // Allowed: the menu can refresh the checks, or forget the repository.
    expect(menu().some((x) => "id" in x && x.id === "wt-checks-refresh")).toBe(true);
    expect(menu().some((x) => "id" in x && x.id === "wt-checks-forget")).toBe(true);
    act(() => select("wt-checks-forget"));
    await waitFor(() =>
      expect(named("pr_checks_allow")[1]?.args).toEqual({ workspaceId: "wt_1", allow: false }),
    );
  });

  it("F-WT-08.2 Import review comments adds them to the review bar by file, line and author; the prompt carries the fenced copy", async () => {
    await loadChecks(["wt_1"]);
    const files: FileDiff[] = [
      {
        path: "a.ts",
        change: "modified",
        additions: 1,
        deletions: 1,
        binary: false,
        truncated: false,
        patch:
          "diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -1,2 +1,2 @@\n keep()\n-old()\n+fresh()\n",
      },
    ] as FileDiff[];
    withDialogs(<ReviewDiff wt={WT} files={files} labels={{}} />);
    const button = await screen.findByTestId("review-import");
    expect(button.textContent).toBe("Import review comments from #7");
    fireEvent.click(button);
    expect(await screen.findByTestId("review-bar")).toBeTruthy();
    const comment = await screen.findByTestId("review-comment");
    expect(comment.dataset.imported).toBe("true");
    expect(comment.dataset.line).toBe("a.ts:new:2");
    expect(comment.textContent).toContain("@ana:");
    const list = useReviews.getState().byLane["wt_1"] ?? [];
    expect(list).toHaveLength(1);
    expect(await screen.findByText("1 comment on a line that no longer exists was left out.")).toBeTruthy();
    const prompt = buildReviewPrompt("tandem/fix", list);
    // The comment's place is GitHub's text too: only inside the fence.
    expect(prompt).toContain(
      "1. Imported review comment:\n   > Untrusted content from a pull request review comment",
    );
    expect(prompt).toContain("   ```text\n   ana on a.ts:2:\n   Rename this\n   ```");
    expect(prompt).not.toContain("1. a.ts:2");
    // Importing again adds nothing new, even once it was removed (or sent).
    act(() => useReviews.getState().remove("wt_1", list[0]!.id));
    fireEvent.click(screen.getByTestId("review-import"));
    await waitFor(() => expect(named("lane_review_comments")).toHaveLength(2));
    expect(named("lane_review_comments")[0]?.args).toEqual({ laneId: "wt_1", seen: [] });
    expect(named("lane_review_comments")[1]?.args).toEqual({ laneId: "wt_1", seen: ["101"] });
    expect(useReviews.getState().byLane["wt_1"] ?? []).toHaveLength(0);
  });

  it("F-WT-08 polling reads only worktrees with running checks, rarely the rest, and never twice at once", async () => {
    const now = 10_000_000;
    expect(checksDue(undefined, undefined, now)).toBe(true);
    const running: LaneChecks = { ...READY, checks: [{ ...READY.checks[0]!, bucket: "pending" }] };
    expect(checksDue(running, now - 1_000, now)).toBe(true);
    expect(checksDue(READY, now - 1_000, now)).toBe(false);
    expect(checksDue({ ...READY, state: "no_pr", pr: null, checks: [] }, now - 1_000, now)).toBe(false);
    expect(checksDue(READY, now - CHECKS_IDLE_MS, now)).toBe(true);
    // Two polls at once make one round of calls.
    await Promise.all([pollChecks(["wt_1"]), pollChecks(["wt_1"])]);
    expect(named("lane_checks")).toHaveLength(1);
    // Finished: the next poll skips it.
    await pollChecks(["wt_1"]);
    expect(named("lane_checks")).toHaveLength(1);
  });

  it("F-WT-08.5 D-2619 a push reads the worktree's checks again at once, and each poll reads them until CI's new run shows", async () => {
    // The rail has read wt_1's checks: CI failed, every check finished, so
    // polls leave it alone for 15 minutes.
    await act(() => loadChecks(["wt_1"]));
    await pollChecks(["wt_1"]);
    expect(named("lane_checks")).toHaveLength(1);
    // A push starts CI again: the checks are read right after it.
    const context = { type: "workspace" as const, workspaceId: "wt_1" };
    expect(await act(() => useGit.getState().push(context))).toBeNull();
    await waitFor(() => expect(named("lane_checks")).toHaveLength(2));
    const order = calls.map((c) => c.cmd).filter((c) => c === "git_push" || c === "lane_checks");
    expect(order).toEqual(["lane_checks", "git_push", "lane_checks"]);
    // gh may list the new run a minute later: until then each poll reads
    // the checks, though the last answer had finished.
    await pollChecks(["wt_1"]);
    expect(named("lane_checks")).toHaveLength(3);
    const now = 10_000_000;
    expect(checksDue(READY, now - 1_000, now, now - 1_000)).toBe(true);
    expect(checksDue(READY, now - 1_000, now, now - CHECKS_AFTER_PUSH_MS)).toBe(false);
    expect(checksDue(READY, now - 1_000, now, undefined)).toBe(false);
  });

  it("F-WT-08.5 F-WT-08.3 D-2619 a push that failed reads no checks, nor does one in a repository not allowed", async () => {
    const context = { type: "workspace" as const, workspaceId: "wt_1" };
    await act(() => loadChecks(["wt_1"]));
    pushFails = true;
    expect((await act(() => useGit.getState().push(context)))?.message).toBe("rejected: fetch first");
    await act(() => new Promise((r) => setTimeout(r, 20)));
    expect(named("lane_checks")).toHaveLength(1);
    pushFails = false;
    // Not allowed: the core would run no gh, and the rail asks it nothing.
    checks = { ...READY, state: "not_allowed", pr: null, checks: [] };
    await act(() => loadChecks(["wt_1"]));
    const before = named("lane_checks").length;
    expect(await act(() => useGit.getState().push(context))).toBeNull();
    await act(() => new Promise((r) => setTimeout(r, 20)));
    expect(named("lane_checks")).toHaveLength(before);
    expect(named("git_push")).toHaveLength(2);
  });

  it("F-WT-08.5 D-2619 a push while another git action runs pushes nothing and reads no checks", async () => {
    const context = { type: "workspace" as const, workspaceId: "wt_1" };
    await act(() => loadChecks(["wt_1"]));
    const key = gitKey(context);
    act(() =>
      useGit.setState((s) => ({
        byKey: { ...s.byKey, [key]: { ...(s.byKey[key] ?? ({} as never)), context, busy: "pull" } },
      })),
    );
    expect(await act(() => useGit.getState().push(context))).toBeNull();
    await act(() => new Promise((r) => setTimeout(r, 20)));
    expect(named("git_push")).toHaveLength(0);
    expect(named("lane_checks")).toHaveLength(1);
  });

  it("F-WT-08 the dialog says when there's no pull request, no gh, or GitLab", async () => {
    for (const [state, text] of [
      ["no_pr", "This worktree's branch has no open pull request."],
      ["no_gh", "Install the GitHub CLI (gh) and sign in with gh auth login to see checks."],
      ["gitlab", "Checks on GitLab merge requests aren't read yet."],
    ] as const) {
      checks = { ...READY, state, pr: null, checks: [] };
      const view = withDialogs(null);
      act(() => useWorktrees.getState().open({ kind: "checks", worktreeId: "wt_1" }));
      expect(await screen.findByText(text)).toBeTruthy();
      act(() => useWorktrees.getState().open(null));
      view.unmount();
    }
  });

  it("F-WT-08 no badge without a pull request, and with no agent a failure can be copied", async () => {
    checks = { ...READY, state: "no_pr", pr: null, checks: [] };
    const view = withDialogs(<WorktreeRow wt={WT} base="main" />);
    await act(() => loadChecks(["wt_1"]));
    expect(screen.queryByTestId("worktree-checks")).toBeNull();
    view.unmount();
    checks = READY;
    withDialogs(null);
    act(() => useWorktrees.getState().open({ kind: "checks", worktreeId: "wt_1" }));
    expect(await screen.findByTestId("check-copy")).toBeTruthy();
  });
});

describe("F-WT-08 checks in one word (D-2603)", () => {
  const c = (bucket: "pass" | "fail" | "pending" | "skipping" | "cancel") =>
    ({ name: bucket, bucket, link: null, workflow: null }) as unknown as Parameters<typeof checksState>[0][number];
  it("a cancelled run never reads as passed; skipped checks don't count against it", () => {
    expect(checksState([c("pass"), c("cancel")])).toBe("cancelled");
    expect(checksState([c("pass"), c("skipping")])).toBe("passed");
    expect(checksState([c("cancel"), c("pending")])).toBe("running");
    expect(checksState([c("pending"), c("fail")])).toBe("failed");
  });
});
