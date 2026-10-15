import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  freshOffline,
  home,
  invoke,
  paneIds,
  paneText,
  sendToPane,
  split,
  waitForPaneText,
} from "../support/app";

type Status = { available: boolean; tool: string | null; reason: string | null };

const GIT_ENV = {
  ...process.env,
  GIT_AUTHOR_NAME: "E2E",
  GIT_AUTHOR_EMAIL: "e2e@vibezeo.test",
  GIT_COMMITTER_NAME: "E2E",
  GIT_COMMITTER_EMAIL: "e2e@vibezeo.test",
};
const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", env: GIT_ENV }).trim();

/** Sends `/sh <cmd>` to the fake engine and returns its exit code. */
async function sh(pane: string, cmd: string, mark: string): Promise<number> {
  await sendToPane(pane, `/sh echo ${mark}; ${cmd}\r`);
  let code = -2;
  await browser.waitUntil(
    async () => {
      const text = await paneText(pane);
      const at = text.lastIndexOf(mark);
      const m = at >= 0 ? /sh exit: (-?\d+)/.exec(text.slice(at)) : null;
      if (m) code = Number(m[1]);
      return m !== null;
    },
    { timeout: 15_000, timeoutMsg: `no exit code for ${cmd}` },
  );
  return code;
}

// E2E-M18-TL11 Agent sandbox (F-TL-11, Linux): with "Workspace" on, an agent
// pane's engine runs under bubblewrap; it writes in its folder, not outside;
// it can't read ~/.ssh; .git/hooks stays read-only. Needs bwrap (the test
// machine's, never installed by Vibezeo).
describe("E2E-M18-TL11 agent sandbox", function () {
  this.timeout(180_000);
  // The panes' HOME (run.ts points it inside TANDEM_HOME, at home/).
  const userHome = process.env.HOME ?? join(home(), "home");
  const repo = join(userHome, "m18-tl11-repo");

  it("F-TL-11.1 F-TL-11.2 F-TL-11.3 F-TL-11.6 a sandboxed agent writes in its folder only and can't read keys or hooks", async () => {
    await freshOffline();
    const status = await invoke<Status>("sandbox_status");
    if (!status.available) throw new Error(`the test machine needs bubblewrap: ${status.reason}`);
    mkdirSync(join(userHome, ".ssh"), { recursive: true });
    writeFileSync(join(userHome, ".ssh", "id_ed25519"), "SECRET KEY\n");
    mkdirSync(repo, { recursive: true });
    git(repo, "init", "-q", "-b", "main");
    writeFileSync(join(repo, "README.md"), "hello\n");
    git(repo, "add", "README.md");
    git(repo, "commit", "-q", "-m", "first");
    await addFolder(repo);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });

    // Off: exactly as before (no chip, and a write outside works).
    const plain = await split("right", "claude");
    await waitForPaneText(plain, "Fake Claude Code");
    expect(
      await $(`[data-testid="pane"][data-pane-id="${plain}"] [data-testid="sandbox-chip"]`).isExisting(),
    ).toBe(false);

    await invoke("settings_set", { key: "agent_sandbox", value: "workspace" });
    const pane = await split("down", "claude");
    await waitForPaneText(pane, "Fake Claude Code");
    const chip = `[data-testid="pane"][data-pane-id="${pane}"] [data-testid="sandbox-chip"]`;
    await $(chip).waitForExist({ timeout: 10_000 });
    expect(await $(chip).getAttribute("data-sandboxed")).toBe("true");

    expect(await sh(pane, `echo hi > ${repo}/inside.txt`, "M1")).toBe(0);
    expect(existsSync(join(repo, "inside.txt"))).toBe(true);
    const outside = `/var/tmp/tandem-e2e-tl11-${Date.now()}`;
    expect(await sh(pane, `echo x > ${outside}`, "M2")).not.toBe(0);
    expect(existsSync(outside)).toBe(false);
    expect(await sh(pane, `echo x > ${userHome}/outside.txt`, "M6")).not.toBe(0);
    expect(existsSync(join(userHome, "outside.txt"))).toBe(false);
    expect(await sh(pane, `cat ${userHome}/.ssh/id_ed25519`, "M3")).not.toBe(0);
    expect(await paneText(pane)).not.toContain("SECRET KEY");
    expect(await sh(pane, `echo x > ${repo}/.git/hooks/pre-commit`, "M4")).not.toBe(0);
    expect(existsSync(join(repo, ".git", "hooks", "pre-commit"))).toBe(false);
    // A commit inside the folder still works.
    expect(
      await sh(
        pane,
        `cd ${repo} && git -c user.name=E -c user.email=e@x add inside.txt && git -c user.name=E -c user.email=e@x commit -qm sandboxed`,
        "M5",
      ),
    ).toBe(0);
    expect(git(repo, "log", "-1", "--format=%s")).toBe("sandboxed");
    await invoke("settings_set", { key: "agent_sandbox", value: "off" });
  });
});
