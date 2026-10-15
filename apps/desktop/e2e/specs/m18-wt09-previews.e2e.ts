import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, freshOffline, invoke, settled, userHome } from "../support/app";
import { newWorktree } from "../support/worktrees";

// E2E-M18-WT09-1 Previews per worktree (F-WT-09): a repository whose file
// has a preview: the first worktree asks to trust it, then runs it in a
// Preview pane on the worktree's port; once it answers, the rail row's
// Preview button opens it in a sidebar browser tab.

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

describe("E2E-M18-WT09-1 previews", function () {
  this.timeout(240_000);
  const repo = `/tmp/tandem-e2e-wt09-${Date.now()}`;

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
  });

  it("F-WT-09.2 the untrusted preview waits for review; F-WT-09.1 trusted, it runs in a Preview pane", async () => {
    await newWorktree(repo, "alpha");
    const review = $('[data-testid="worktree-review"]');
    await review.waitForExist({ timeout: 10_000 });
    expect(String(await review.getProperty("textContent"))).toContain("python3 -m http.server {{port}}");
    // Nothing answers before trust.
    const lane = (await invoke<Ws[]>("workspace_list")).find((w) => w.laneBranch === "tandem/alpha")?.id ?? "";
    const before = await invoke<Array<{ status: string; port: number | null }>>("lane_previews", { laneIds: [lane] });
    expect(before[0]?.status).toBe("down");
    await settled();
    await $('[data-testid="worktree-review-trust"]').click();
    await review.waitForExist({ reverse: true, timeout: 10_000 });
    await browser.waitUntil(
      async () => (await invoke<Array<{ status: string }>>("lane_previews", { laneIds: [lane] }))[0]?.status === "ready",
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
    const lane = (await invoke<Ws[]>("workspace_list")).find((w) => w.laneBranch === "tandem/alpha")?.id ?? "";
    const [state] = await invoke<Array<{ url: string; port: number }>>("lane_previews", { laneIds: [lane] });
    const button = $('[data-testid="worktree-preview"]');
    await button.waitForExist({ timeout: 15_000, timeoutMsg: "no Preview button on the row" });
    await browser.execute(() => document.querySelector<HTMLElement>('[data-testid="worktree-preview"]')?.click());
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
});
