import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, clickAllow, freshOffline, invoke, settled, userHome } from "../support/app";
import { newWorktree, pick } from "../support/worktrees";

// E2E-M18-WT09-1 Previews per worktree (F-WT-09): a repository whose file
// has a preview: the first worktree asks to trust it, then runs it in a
// Preview pane on the worktree's port; once it answers, the rail row's
// Preview button opens it in a sidebar browser tab. A second worktree's
// preview starts on its own (already trusted), and "Compare previews…"
// opens both side by side in a new tab of the repository.

const GIT_ENV = {
  ...process.env,
  GIT_AUTHOR_NAME: "E2E",
  GIT_AUTHOR_EMAIL: "e2e@vibezeo.test",
  GIT_COMMITTER_NAME: "E2E",
  GIT_COMMITTER_EMAIL: "e2e@vibezeo.test",
};
const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", env: GIT_ENV }).trim();

type Ws = { id: string; laneBranch: string | null };
type Preview = { status: string; port: number | null; url: string | null };
const laneId = async (branch: string): Promise<string> =>
  (await invoke<Ws[]>("workspace_list")).find((w) => w.laneBranch === branch)?.id ?? "";

async function worktreeAction(branch: string, label: string): Promise<void> {
  await browser.waitUntil(
    async () => {
      const opened = await browser.execute((b: string) => {
        const button = document.querySelector<HTMLElement>(`button[aria-label="Actions for ${b}"]`);
        button?.focus();
        return button !== null && document.activeElement === button;
      }, branch);
      if (!opened) return false;
      await browser.keys("Enter");
      await browser.pause(300);
      return browser.execute(() => document.querySelectorAll("[role=menuitem]").length > 0);
    },
    { timeout: 15_000, timeoutMsg: `no menu for ${branch}` },
  );
  await pick(label);
}

describe("E2E-M18-WT09-1 previews", function () {
  this.timeout(240_000);
  const repo = `/tmp/tandem-e2e-wt09-${Date.now()}`;
  /** The rail's minimum before any row shows a Preview (D-2703). */
  let baseMin = 0;

  before(async () => {
    mkdirSync(join(repo, ".vibezeo"), { recursive: true });
    git(repo, "init", "-q", "-b", "main");
    writeFileSync(join(repo, "index.html"), "<h1>alpha preview</h1>\n");
    git(repo, "add", ".");
    git(repo, "commit", "-q", "-m", "first");
    writeFileSync(
      join(repo, ".vibezeo/worktree.json"),
      JSON.stringify({
        ports: 5,
        preview: { command: "python3 -m http.server {{port}} --bind 127.0.0.1" },
      }),
    );
    writeFileSync(join(userHome(), ".gitconfig"), "[user]\n\tname = E2E\n\temail = e2e@vibezeo.test\n");
    await freshOffline();
    await addFolder(repo);
    // Measured once the rail's fonts are in (D-2701, D-2626).
    await browser.pause(1_000);
    baseMin = Number(await $('[data-testid="rail-resizer"]').getAttribute("aria-valuemin"));
  });

  it("F-WT-09.2 the untrusted preview waits for review; F-WT-09.1 trusted, it runs in a Preview pane", async () => {
    await newWorktree(repo, "alpha");
    const review = $('[data-testid="worktree-review"]');
    await review.waitForExist({ timeout: 10_000 });
    expect(String(await review.getProperty("textContent"))).toContain("python3 -m http.server {{port}}");
    // Nothing answers before trust.
    const lane =
      (await invoke<Ws[]>("workspace_list")).find((w) => w.laneBranch === "tandem/alpha")?.id ?? "";
    const before = await invoke<Array<{ status: string; port: number | null }>>("lane_previews", {
      laneIds: [lane],
    });
    expect(before[0]?.status).toBe("down");
    await settled();
    await clickAllow('[data-testid="worktree-review-trust"]');
    await review.waitForExist({ reverse: true, timeout: 10_000 });
    await browser.waitUntil(
      async () =>
        (await invoke<Array<{ status: string }>>("lane_previews", { laneIds: [lane] }))[0]?.status ===
        "ready",
      { timeout: 45_000, timeoutMsg: "the preview never answered" },
    );
    const pane = await browser.execute(() =>
      Array.from(document.querySelectorAll<HTMLElement>('[data-testid="pane"]')).some((p) =>
        (p.textContent ?? "").includes("Preview"),
      ),
    );
    expect(pane).toBe(true);
  });

  it("F-WT-09.1 the rail row's Preview button opens the preview in a browser tab", async () => {
    const lane =
      (await invoke<Ws[]>("workspace_list")).find((w) => w.laneBranch === "tandem/alpha")?.id ?? "";
    const [state] = await invoke<Array<{ url: string; port: number }>>("lane_previews", { laneIds: [lane] });
    const button = $('[data-testid="worktree-preview"]');
    await button.waitForExist({ timeout: 15_000, timeoutMsg: "no Preview button on the row" });
    await browser.execute(() =>
      document.querySelector<HTMLElement>('[data-testid="worktree-preview"]')?.click(),
    );
    await browser.waitUntil(
      async () =>
        browser.execute(
          (port: number) =>
            document.querySelectorAll('[data-testid="sidebar-pill-browser"]').length > 0 &&
            document.body.innerHTML.includes(`localhost:${port}`),
          state?.port ?? 0,
        ),
      { timeout: 15_000, timeoutMsg: "no browser tab for the preview" },
    );
    expect(state?.url).toMatch(new RegExp(`^http://(alpha\\.)?localhost:${state?.port}/$`));
  });

  it("F-SH-03 D-2703 while a row shows its Preview, the rail's minimum adds the globe and its gap, and the rail widens to it", async () => {
    expect(baseMin).toBeGreaterThanOrEqual(246);
    const edge = $('[data-testid="rail-resizer"]');
    // The globe (20 px) and its gap (6) have no text: every engine draws them alike.
    await browser.waitUntil(async () => Number(await edge.getAttribute("aria-valuemin")) === baseMin + 26, {
      timeout: 10_000,
      timeoutMsg: `the minimum stayed ${await edge.getAttribute("aria-valuemin")}, from ${baseMin}`,
    });
    // Eased over --dur-base, then saved.
    await browser.pause(500);
    const width = await browser.execute(
      () => document.querySelector('nav[aria-label="Workspaces"]')?.getBoundingClientRect().width ?? 0,
    );
    expect(width).toBeGreaterThanOrEqual(baseMin + 26 - 0.5);
    expect(Number(await edge.getAttribute("aria-valuenow"))).toBeGreaterThanOrEqual(baseMin + 26);
  });

  it("F-WT-09.5 Compare previews opens two worktrees' previews side by side in a new tab", async () => {
    await newWorktree(repo, "beta");
    const beta = await laneId("tandem/beta");
    // Trusted already: the preview starts with the worktree, no review.
    await browser.waitUntil(
      async () => (await invoke<Preview[]>("lane_previews", { laneIds: [beta] }))[0]?.status === "ready",
      { timeout: 45_000, timeoutMsg: "beta's preview never answered" },
    );
    // The preview is the new worktree's only pane: no shell started beside it
    // to take the last of Free's 4 live panes (D-863).
    const betaTab = (await invoke<Array<{ id: string; tabs: Array<{ id: string }> }>>("workspace_list")).find(
      (w) => w.id === beta,
    )?.tabs[0]?.id;
    const betaPanes = await invoke<Array<{ humanName: string | null }>>("pane_list", { tabId: betaTab });
    expect(betaPanes.map((p) => p.humanName)).toEqual(["Preview"]);
    const [alphaState, betaState] = await invoke<Preview[]>("lane_previews", {
      laneIds: [await laneId("tandem/alpha"), beta],
    });
    expect(alphaState?.port).not.toBe(betaState?.port);
    await worktreeAction("tandem/alpha", "Compare previews…");
    const row = $('[data-testid="compare-previews-row"][data-branch="tandem/beta"] [role="checkbox"]');
    await row.waitForExist({ timeout: 15_000 });
    await settled();
    await row.click();
    await $('[data-testid="compare-previews-open"]').click();
    await $('[data-testid="compare-previews"]').waitForExist({ reverse: true, timeout: 30_000 });
    // Two browser panes in the shown tab, one per branch, each on its port.
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          // Other tabs stay mounted, hidden: only the shown tab's panes.
          const panes = Array.from(document.querySelectorAll<HTMLElement>('[data-testid="pane"]')).filter(
            (p) => p.closest<HTMLElement>('[id^="panel-"]')?.style.display !== "none",
          );
          const text = panes.map((p) => p.textContent ?? "");
          return (
            panes.length === 2 &&
            text.some((x) => x.includes("tandem/alpha")) &&
            text.some((x) => x.includes("tandem/beta"))
          );
        }),
      { timeout: 15_000, timeoutMsg: "no side-by-side tab" },
    );
    // The repository has a tab named for both, holding one pane per preview.
    const parent = (
      await invoke<Array<Ws & { root: string; tabs: Array<{ id: string; name: string }> }>>("workspace_list")
    ).find((w) => w.root === repo);
    const compareTab = parent?.tabs.find((x) => x.name === "tandem/alpha | tandem/beta");
    expect(compareTab).toBeDefined();
    const panes = await invoke<Array<{ kind: string; browserUrl: string | null; humanName: string | null }>>(
      "pane_list",
      {
        tabId: compareTab?.id,
      },
    );
    expect(
      panes
        .map((p) => [p.kind, p.humanName, p.browserUrl])
        .sort((a, b) => String(a[1]).localeCompare(String(b[1]))),
    ).toEqual([
      ["browser", "tandem/alpha", alphaState?.url],
      ["browser", "tandem/beta", betaState?.url],
    ]);
  });
});
