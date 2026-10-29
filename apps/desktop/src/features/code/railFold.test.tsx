// D-2701: a rail row narrower than its contents folds them, never cuts them
// off. A worktree row folds its preview to its icon, then its radar mark,
// its checks badge, and last its status words to their dot; whatever folds
// keeps its words as its accessible name and tooltip.
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentStatus, LaneChecks, Tab, WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useLayout } from "../../stores/layout";
import { useStatus } from "../../stores/status";
import { useWorkspaces } from "../../stores/workspaces";
import { useWorktrees } from "../../stores/worktrees";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { RAIL_MIN } from "./CodeRail";
import { WorktreeRow } from "./Worktrees";

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
  liveCount: 2,
  tabs: [tab("wt_1")],
} as WorkspaceSummary;

const FAILED: LaneChecks = {
  laneId: "wt_1",
  state: "ready",
  pr: { number: 7, url: "https://github.com/o/r/pull/7" },
  checks: [{ name: "test", bucket: "fail", link: null, workflow: "CI", jobId: null }],
  error: null,
};

/** The worktree's panes, each with a status; `unread` finished unseen. */
function panes(statuses: AgentStatus[], unread: string[] = []) {
  const ids = statuses.map((_, i) => `p${i + 1}`);
  useLayout.setState({
    tabs: {
      tb_wt_1: {
        tree: null,
        active: null,
        maximized: null,
        panes: Object.fromEntries(ids.map((id) => [id, {} as never])),
        keepEmpty: false,
      },
    },
  });
  useStatus.setState({
    records: Object.fromEntries(
      statuses.map((status, i) => [
        `pane:p${i + 1}`,
        {
          key: `pane:p${i + 1}`,
          status,
          reason: null,
          since: 1,
          ts: 1,
          tool: null,
          title: null,
          preview: null,
        },
      ]),
    ),
    unread: Object.fromEntries(unread.map((id) => [`pane:${id}`, true])),
  });
}

/** Every mark a worktree row can show: a preview, the checks and the radar. */
function marks() {
  useWorktrees.setState({
    checks: { wt_1: FAILED },
    radar: { wt_1: ["applyCoupon"] },
    previews: {
      wt_1: {
        laneId: "wt_1",
        status: "ready",
        url: "http://fix.localhost:41000/",
        port: 41000,
        trusted: true,
      },
    },
  });
}

/** The row's contents: 120 px, and 80 more for each part showing its words. */
let room = 1000;
/** How many times a row's width was read. */
let measures = 0;
function measureRows() {
  const row = (el: Element) => el instanceof HTMLElement && el.dataset.testid === "worktree-row";
  const scroll = vi.spyOn(Element.prototype, "scrollWidth", "get").mockImplementation(function (
    this: Element,
  ) {
    if (!row(this)) return 0;
    measures += 1;
    return 120 + 80 * this.querySelectorAll("[data-words]").length;
  });
  const client = vi.spyOn(Element.prototype, "clientWidth", "get").mockImplementation(function (
    this: Element,
  ) {
    return row(this) ? room : 0;
  });
  return () => {
    scroll.mockRestore();
    client.mockRestore();
  };
}

const show = (width: number) => {
  room = width;
  return render(
    <UiProvider>
      <WorktreeRow wt={WT} base="main" />
    </UiProvider>,
  );
};
const words = (testId: string) =>
  screen.getByTestId(testId).querySelector("[data-words]")?.textContent ?? null;

let restore: () => void = () => undefined;
beforeEach(() => {
  resetStores();
  useWorktrees.setState({ checks: {}, radar: {}, previews: {} });
  useLayout.setState({ tabs: {} });
  useStatus.setState({ records: {}, unread: {} });
  fakeBackend({ workspace_list: () => [REPO, WT], lane_forge: () => "github", lane_runtime: () => null });
  useWorkspaces.setState({ list: [REPO, WT], activeId: "shop", activeTab: {} });
  restore = measureRows();
});
afterEach(() => {
  restore();
  setInvokeBackend(null);
});

describe("F-SH-03 D-2701 a rail row never cuts its contents off", () => {
  it("F-WT-09.1 F-WT-08.1 F-WT-10.1 D-2701 the marks fold in order: the preview, the radar mark, then the checks badge", () => {
    marks();
    // 120 + 3 × 80: room for all three marks' words.
    let view = show(360);
    expect(words("worktree-preview")).toBe("Preview");
    expect(words("worktree-radar")).toBe("applyCoupon");
    expect(words("worktree-checks")).toBe("CI failed");
    view.unmount();
    // Room for two: the preview's word goes first; it's its icon.
    view = show(280);
    const preview = screen.getByTestId("worktree-preview");
    expect(words("worktree-preview")).toBeNull();
    expect(preview.textContent).toBe("");
    expect(preview.querySelector("svg")).not.toBeNull();
    expect(preview.getAttribute("aria-label")).toBe("Open the preview of tandem/fix");
    expect(preview.title).toBe("Open the preview of tandem/fix");
    expect(words("worktree-radar")).toBe("applyCoupon");
    expect(words("worktree-checks")).toBe("CI failed");
    view.unmount();
    // Room for one: the radar mark folds next, and the badge keeps its words.
    view = show(200);
    expect(words("worktree-radar")).toBeNull();
    expect(words("worktree-checks")).toBe("CI failed");
    view.unmount();
    // Room for none: every mark is its icon, with its name and tooltip.
    show(120);
    expect(words("worktree-checks")).toBeNull();
    expect(screen.getByTestId("worktree-checks").getAttribute("aria-label")).toBe(
      "CI failed: 1 check failed: test",
    );
    expect(screen.getByTestId("worktree-radar").getAttribute("aria-label")).toBe(
      "applyCoupon, shared with another worktree",
    );
  });

  it("F-SH-03 D-2603 D-2701 the status words fold last, to their dot; the words stay its name and tooltip, and the unread dot sits on it", () => {
    marks();
    panes(["waiting", "idle"], ["p2"]);
    // Beside the status the checks and radar marks are icons (D-2700): the
    // preview's word and the status's are all that can fold.
    let view = show(280);
    const status = () => screen.getByTestId("workspace-status");
    expect(words("worktree-checks")).toBeNull();
    expect(words("worktree-radar")).toBeNull();
    expect(words("worktree-preview")).toBe("Preview");
    expect(status().textContent).toBe("Needs you");
    expect(status().hasAttribute("data-folded")).toBe(false);
    // Unfolded, the unread dot stands beside the status.
    expect(within(status()).queryByTestId("workspace-unread")).toBeNull();
    expect(screen.getByTestId("workspace-unread")).toBeTruthy();
    view.unmount();
    view = show(200);
    expect(words("worktree-preview")).toBeNull();
    expect(status().textContent).toBe("Needs you");
    view.unmount();
    // No room for any words: the status is its dot.
    show(120);
    expect(status().hasAttribute("data-folded")).toBe(true);
    expect(status().textContent).toBe("");
    expect(status().title).toBe("Needs you");
    const dot = within(status()).getByRole("img", { name: "Needs you" });
    expect(dot.getAttribute("data-status")).toBe("waiting");
    // The unread dot sits on the status dot, as on a pane's row, taking no room.
    const unread = within(status()).getByTestId("workspace-unread");
    expect(unread.getAttribute("aria-label")).toBe("Finished while you were away");
    expect(unread.className).toMatch(/\babsolute\b/);
    expect(screen.getAllByTestId("workspace-unread")).toHaveLength(1);
  });

  it("F-SH-03 D-2603 D-2701 each status folds to its own dot: n working, Failed", () => {
    panes(["working", "working"]);
    let view = show(120);
    expect(
      within(screen.getByTestId("workspace-status"))
        .getByRole("img", { name: "2 working" })
        .getAttribute("data-status"),
    ).toBe("working");
    expect(screen.getByTestId("workspace-status").title).toBe("2 working");
    view.unmount();
    panes(["failed"]);
    view = show(120);
    expect(
      within(screen.getByTestId("workspace-status"))
        .getByRole("img", { name: "Failed" })
        .getAttribute("data-status"),
    ).toBe("error");
    view.unmount();
    // With room, the words show and nothing folds.
    show(400);
    expect(screen.getByTestId("workspace-status").textContent).toBe("Failed");
  });

  it("F-SH-03 D-2701 RAIL_MIN adds up a fully folded row's parts at the deepest indent", () => {
    // CodeRail.tsx measured these parts once, in the app's own CSS; when one
    // of them changes here, RAIL_MIN has to be measured again.
    marks();
    useWorktrees.setState({ previews: {} });
    panes(["waiting"]);
    show(120);
    const row = screen.getByTestId("worktree-row");
    expect(row.className.split(" ")).toEqual(expect.arrayContaining(["px-2", "gap-1.5", "text-sm"]));
    // Its parts, in order (⋯ covers the row's end and takes no room).
    const parts = [...row.children].filter((c) => !/\babsolute\b/.test(c.className));
    expect(parts.map((c) => c.getAttribute("data-testid") ?? c.tagName.toLowerCase())).toEqual([
      "svg",
      "worktree-row-name",
      "workspace-status",
      "worktree-checks",
      "worktree-radar",
      "worktree-live",
    ]);
    expect(parts[0]?.getAttribute("width")).toBe("12");
    expect(screen.getByTestId("worktree-row-name").className).toContain("min-w-[min(10ch,100%)]");
    expect(screen.getByTestId("workspace-status").querySelector("svg")?.getAttribute("width")).toBe("6");
    const checks = screen.getByTestId("worktree-checks");
    expect(checks.className.split(" ")).toContain("px-1");
    expect(checks.querySelector("svg")?.getAttribute("width")).toBe("12");
    const radar = screen.getByTestId("worktree-radar").firstElementChild;
    expect(radar?.className.split(" ")).toEqual(expect.arrayContaining(["px-1.5", "border"]));
    expect(radar?.querySelector("svg")?.getAttribute("width")).toBe("10");
    expect(screen.getByTestId("worktree-live").firstElementChild?.className.split(" ")).toContain("px-1.5");
    expect(RAIL_MIN).toBe(246);
  });

  it("F-SH-03 D-2701 a render that changes nothing the row shows measures nothing", () => {
    marks();
    panes(["working"]);
    show(200);
    const before = measures;
    expect(before).toBeGreaterThan(0);
    // Selecting the worktree draws the row again, with the same contents.
    act(() => useWorkspaces.setState({ activeId: "wt_1" }));
    expect(screen.getByTestId("worktree-row").getAttribute("aria-selected")).toBe("true");
    expect(measures).toBe(before);
    // New contents are measured.
    act(() => useWorktrees.setState({ radar: { wt_1: ["applyCoupon", "priceOf"] } }));
    expect(measures).toBeGreaterThan(before);
  });

  it("F-SH-03 D-2700 ⋯ takes the place of the marks under it, whole, never the end of one", () => {
    marks();
    panes(["waiting"]);
    // Where the row's parts are: the radar mark's end is under ⋯'s button.
    const at: Record<string, [number, number]> = {
      "worktree-row-name": [20, 120],
      "workspace-status": [126, 132],
      "worktree-checks": [138, 158],
      "worktree-radar": [164, 226],
      "worktree-live": [232, 244],
      menu: [224, 252],
    };
    const rect = vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (
      this: Element,
    ) {
      const key = this.closest('[data-testid="worktree-row-menu"]')
        ? "menu"
        : this.getAttribute("data-testid");
      const [left, right] = (key && at[key]) || [0, 0];
      return { left, right, width: right - left, top: 0, bottom: 28, height: 28, x: left, y: 0 } as DOMRect;
    });
    try {
      show(1000);
      const row = screen.getByTestId("worktree-row");
      const menu = screen.getByTestId("worktree-row-menu");
      const button = within(menu).getByRole("button", { name: "Actions for tandem/fix" });
      // As ⋯ shows, it covers the radar mark from its start, and the count.
      fireEvent.pointerEnter(row);
      expect(menu.style.paddingLeft).toBe("60px");
      // Whenever ⋯ shows, on hover or focus, the cover is the row's colour.
      expect(row.className.split(" ")).toEqual(
        expect.arrayContaining(["hover:bg-surface-4", "focus-within:bg-surface-4"]),
      );
      expect(menu.className.split(" ")).toContain("bg-surface-4");
      // A click on the cover is the row's, never a mark's under it; ⋯'s is
      // its menu's.
      fireEvent.click(button);
      expect(useWorkspaces.getState().activeId).toBe("shop");
      fireEvent.click(menu);
      expect(useWorkspaces.getState().activeId).toBe("wt_1");
      expect(useWorktrees.getState().ask).toBeNull();
      // With the radar mark clear of ⋯, the cover is ⋯'s own.
      at["worktree-radar"] = [164, 218];
      act(() => button.focus());
      expect(menu.style.paddingLeft).toBe("0px");
      // The name isn't a mark: ⋯ covers its end, as any text cut short.
      at["worktree-row-name"] = [20, 250];
      fireEvent.pointerEnter(row);
      expect(menu.style.paddingLeft).toBe("0px");
    } finally {
      rect.mockRestore();
    }
  });

  it("F-SH-03 D-2700 with no count, the row keeps ⋯'s room at its end, so ⋯ never covers a mark", () => {
    marks();
    useWorkspaces.setState({ list: [REPO, { ...WT, liveCount: 0 }] });
    render(
      <UiProvider>
        <WorktreeRow wt={{ ...WT, liveCount: 0 }} base="main" />
      </UiProvider>,
    );
    const row = screen.getByTestId("worktree-row");
    const parts = [...row.children].filter((c) => !/\babsolute\b/.test(c.className));
    expect(screen.queryByTestId("worktree-live")).toBeNull();
    // As wide as a one-digit count: ⋯ covers 20 px of the row's contents
    // (its 28 px at the row's edge, less the row's 8 px padding), and the
    // room and the gap before it are 22.
    const room = parts.at(-1);
    expect(room?.getAttribute("data-testid")).toBe("worktree-menu-room");
    expect(room?.getAttribute("aria-hidden")).toBe("true");
    expect(room?.className.split(" ")).toEqual(expect.arrayContaining(["w-4", "shrink-0"]));
    const menu = screen.getByTestId("worktree-row-menu");
    expect(menu.className.split(" ")).toEqual(expect.arrayContaining(["absolute", "right-0"]));
    expect(within(menu).getByRole("button").className.split(" ")).toContain("size-7");
    expect(row.className.split(" ")).toEqual(expect.arrayContaining(["px-2", "gap-1.5"]));
  });

  it("F-SH-03 D-2701 new contents are measured again from nothing folded", () => {
    marks();
    show(200);
    expect(words("worktree-radar")).toBeNull();
    // Room again, and the row's contents change: everything unfolds that fits.
    room = 1000;
    act(() => useWorktrees.setState({ radar: { wt_1: ["applyCoupon", "priceOf"] } }));
    expect(words("worktree-preview")).toBe("Preview");
    expect(words("worktree-radar")).toBe("2 symbols");
    expect(words("worktree-checks")).toBe("CI failed");
  });
});
