import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  clickAllow,
  freshOffline,
  home,
  invoke,
  paneIds,
  relaunch,
  sendToPane,
  settled,
  userHome,
  waitForApp,
} from "../support/app";
import { openFromPlus } from "../support/files";
import { newWorktree, pick } from "../support/worktrees";

// E2E-M18-WT06-1 Worktree runtime (F-WT-06): two worktrees get disjoint
// port blocks and their own variables; the file's env waits for trust; a
// server in a worktree is grouped under it in the ports panel; the blocks
// survive a relaunch; the teardown sees the worktree's Compose project.

const GIT_ENV = {
  ...process.env,
  GIT_AUTHOR_NAME: "E2E",
  GIT_AUTHOR_EMAIL: "e2e@vibezeo.test",
  GIT_COMMITTER_NAME: "E2E",
  GIT_COMMITTER_EMAIL: "e2e@vibezeo.test",
};
const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", env: GIT_ENV }).trim();

const answerConfirm = (answer: "yes" | "no") => writeFileSync(join(home(), "confirm.txt"), answer);

type Ws = { id: string; laneBranch: string | null };
const laneId = async (branch: string): Promise<string> =>
  (await invoke<Ws[]>("workspace_list")).find((w) => w.laneBranch === branch)?.id ?? "";

async function waitForFile(path: string, timeout = 20_000): Promise<string> {
  await browser.waitUntil(() => existsSync(path) && readFileSync(path, "utf8").includes("\n"), {
    timeout,
    timeoutMsg: `${path} never appeared`,
  });
  return readFileSync(path, "utf8").trim();
}

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

/** The first pane of the worktree that's showing, once its shell is up. */
async function shownPane(): Promise<string> {
  await browser.waitUntil(async () => (await paneIds()).length >= 1, { timeout: 20_000 });
  await browser.pause(1_500);
  return (await paneIds())[0] ?? "";
}

describe("E2E-M18-WT06-1 worktree runtime", function () {
  this.timeout(300_000);
  const stamp = Date.now();
  const repo = `/tmp/tandem-e2e-wt06-${stamp}`;
  const out = (name: string) => `/tmp/tandem-e2e-wt06-${stamp}-${name}.txt`;
  const probe = (name: string) =>
    `printf '%s|%s|%s|%s\\n' "$VIBEZEO_PORT" "$VIBEZEO_PORTS" "$DB" "$COMPOSE_PROJECT_NAME" > ${out(name)}\r`;

  before(async () => {
    mkdirSync(join(repo, ".vibezeo"), { recursive: true });
    git(repo, "init", "-q", "-b", "main");
    writeFileSync(join(repo, "README.md"), "hello\n");
    git(repo, "add", ".");
    git(repo, "commit", "-q", "-m", "first");
    writeFileSync(
      join(repo, ".vibezeo/worktree.json"),
      JSON.stringify({
        ports: 10,
        env: { DB: "app_{{slug}}" },
        teardown: `printf '%s|%s\\n' "$COMPOSE_PROJECT_NAME" "$DB" > ${out("teardown")}`,
      }),
    );
    writeFileSync(join(userHome(), ".gitconfig"), "[user]\n\tname = E2E\n\temail = e2e@vibezeo.test\n");
    await freshOffline();
    await addFolder(repo);
  });

  it("F-WT-06.2 before trust a worktree's shell has Vibezeo's variables but not the file's env", async () => {
    await newWorktree(repo, "alpha");
    // The file has a teardown and env, neither trusted: the review opens.
    await $('[data-testid="worktree-review"]').waitForExist({ timeout: 10_000 });
    const pane = await shownPane();
    await sendToPane(pane, probe("alpha-before"));
    const [port, ports, db, compose] = (await waitForFile(out("alpha-before"))).split("|");
    expect(Number(port)).toBeGreaterThanOrEqual(41000);
    expect(ports).toBe(`${port}-${Number(port) + 9}`);
    expect(db).toBe("");
    expect(compose).toBe(`tandem-e2e-wt06-${stamp}-alpha`);
    await settled();
    await clickAllow('[data-testid="worktree-review-trust"]');
    await $('[data-testid="worktree-review"]').waitForExist({ reverse: true, timeout: 10_000 });
  });

  it("F-WT-06.1 a second worktree gets a disjoint block; F-WT-06.2 trusted, its shell has the env", async () => {
    await newWorktree(repo, "beta");
    const pane = await shownPane();
    await sendToPane(pane, probe("beta"));
    const [betaPort, , db, compose] = (await waitForFile(out("beta"))).split("|");
    const [alphaPort] = readFileSync(out("alpha-before"), "utf8").split("|");
    expect(Math.abs(Number(betaPort) - Number(alphaPort))).toBeGreaterThanOrEqual(10);
    expect(db).toBe("app_beta");
    expect(compose).toBe(`tandem-e2e-wt06-${stamp}-beta`);
    // F-WT-06.4: a server on its port is grouped under the worktree.
    await sendToPane(pane, `python3 -m http.server "$VIBEZEO_PORT" --bind 127.0.0.1\r`);
    await openFromPlus("Ports");
    const row = `[data-testid="port-row"][data-port="${betaPort}"]`;
    await $(row).waitForExist({ timeout: 20_000, timeoutMsg: "the worktree's server isn't listed" });
    expect(await $(row).getAttribute("data-branch")).toBe("tandem/beta");
    expect(await $(`[data-testid="port-group"][data-branch="tandem/beta"]`).isExisting()).toBe(true);
    await browser.execute(
      (sel: string) => document.querySelector<HTMLElement>(`${sel} [data-testid="port-open"]`)?.click(),
      row,
    );
    await browser.waitUntil(
      async () => browser.execute((p: string) => document.body.innerHTML.includes(`localhost:${p}`), betaPort ?? ""),
      { timeout: 15_000, timeoutMsg: "no browser tab for the worktree's port" },
    );
  });

  it("F-WT-06.1 the blocks survive a relaunch", async () => {
    const before = {
      alpha: await invoke<{ start: number } | null>("lane_runtime", { laneId: await laneId("tandem/alpha") }),
      beta: await invoke<{ start: number } | null>("lane_runtime", { laneId: await laneId("tandem/beta") }),
    };
    expect(before.alpha?.start).not.toBe(before.beta?.start);
    await relaunch();
    await waitForApp();
    const after = {
      alpha: await invoke<{ start: number } | null>("lane_runtime", { laneId: await laneId("tandem/alpha") }),
      beta: await invoke<{ start: number } | null>("lane_runtime", { laneId: await laneId("tandem/beta") }),
    };
    expect(after).toEqual(before);
  });

  it("F-WT-06.5 discarding runs the teardown with the worktree's variables", async () => {
    answerConfirm("yes");
    await worktreeAction("tandem/alpha", "Discard worktree…");
    await $('//label[normalize-space()="Type tandem/alpha to confirm"]/following::input[1]').setValue("tandem/alpha");
    await $("button=Discard worktree").click();
    expect(await waitForFile(out("teardown"))).toBe(`tandem-e2e-wt06-${stamp}-alpha|app_alpha`);
  });
});
