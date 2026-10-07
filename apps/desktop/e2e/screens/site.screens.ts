import { $, $$, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  checkAgain,
  freshOffline,
  paneIds,
  profileWithoutEngines,
  sendToPane,
  setProfile,
  settled,
  split,
  waitForPaneText,
} from "../support/app";

// Screenshots of the real app for the public site (apps/web/public/screens).
// Not a test: run it on its own, under a display, with the real `claude`
// CLI on the machine (it's only detected, never launched or signed in):
//
//   dbus-run-session -- xvfb-run -a -s "-screen 0 1920x1080x24" \
//     pnpm --filter desktop e2e -- --spec e2e/screens/site.screens.ts
//
// The workspace is a fresh clone of this repository, so the terminals show
// its real history, tests and code.

const repo = new URL("../../../..", import.meta.url).pathname.replace(/\/$/, "");
const out = process.env.TANDEM_SCREENS_DIR ?? join(repo, "apps/web/public/screens");
const enginePath = process.env.TANDEM_SCREENS_PATH ?? "/opt/node22/bin";

async function shot(name: string): Promise<void> {
  await settled();
  await browser.pause(600);
  await browser.saveScreenshot(join(out, `${name}.png`));
}

/** A window screenshot cropped to `selector`'s parent plus `pad` px, cropped
 * in the webview itself (WebKit's element screenshots clip this layout). */
async function shotOf(selector: string, name: string, pad: number): Promise<void> {
  const rect = await browser.execute(
    (sel: string, p: number) => {
      const r = document.querySelector(sel)?.parentElement?.getBoundingClientRect();
      if (!r) return null;
      return {
        x: r.x - p,
        y: r.y - p,
        w: r.width + 2 * p,
        h: r.height + 2 * p,
        dpr: window.devicePixelRatio,
      };
    },
    selector,
    pad,
  );
  if (!rect) throw new Error(`${selector} isn't on screen`);
  const full = await browser.takeScreenshot();
  const png = await browser.executeAsync(
    (
      b64: string,
      r: { x: number; y: number; w: number; h: number; dpr: number },
      done: (out: string) => void,
    ) => {
      const img = new Image();
      img.onload = () => {
        const c = document.createElement("canvas");
        c.width = Math.round(r.w * r.dpr);
        c.height = Math.round(r.h * r.dpr);
        c.getContext("2d")?.drawImage(
          img,
          r.x * r.dpr,
          r.y * r.dpr,
          c.width,
          c.height,
          0,
          0,
          c.width,
          c.height,
        );
        done(c.toDataURL("image/png").split(",")[1] ?? "");
      };
      img.src = `data:image/png;base64,${b64}`;
    },
    full,
    rect,
  );
  writeFileSync(join(out, `${name}.png`), Buffer.from(png, "base64"));
}

describe("site screenshots", () => {
  const work = `/tmp/tandem-screens-${Date.now()}`;

  before(() => {
    mkdirSync(out, { recursive: true });
    execFileSync("git", ["clone", "-q", "--depth", "80", `file://${repo}`, join(work, "tandem")]);
    // The run's HOME is a temp dir: point the shells at this machine's Rust
    // toolchain and build cache so `cargo test` in a pane runs as usual.
    const cargoHome = process.env.CARGO_HOME ?? join(process.env.TANDEM_REAL_HOME ?? "/root", ".cargo");
    const rustupHome = process.env.RUSTUP_HOME ?? join(process.env.TANDEM_REAL_HOME ?? "/root", ".rustup");
    setProfile([
      ...profileWithoutEngines(),
      `export PATH=${enginePath}:${cargoHome}/bin:/usr/local/bin:/usr/bin:/bin`,
      `export CARGO_HOME=${cargoHome} RUSTUP_HOME=${rustupHome} CARGO_TARGET_DIR=${join(repo, "target")}`,
    ]);
  });

  after(() => {
    setProfile(null);
    rmSync(work, { recursive: true, force: true });
  });

  it("code mode: four real terminals over this repository", async () => {
    await freshOffline();
    const ws = join(work, "tandem");
    await addFolder(ws);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const [first] = await paneIds();
    const right = await split("right");
    const belowRight = await split("down");
    await $(`[data-testid="pane"][data-pane-id="${first}"]`).click();
    const belowLeft = await split("down");
    await sendToPane(first ?? "", "git --no-pager log --oneline --decorate --color=always -n 14\r");
    await sendToPane(right, "htop\r");
    // How Tandem reads Claude Code's hooks.
    await sendToPane(belowLeft, "vim +/PreToolUse crates/tandem-engines/src/hook.rs\r");
    await sendToPane(belowRight, "cargo test -q -p tandem-core --lib 2>&1 | tail -n 12\r");
    await waitForPaneText(first ?? "", "M5:", 20_000);
    await waitForPaneText(belowRight, "test result", 240_000);
    await shot("code-mode");
  });

  it("the launch grid finds the engines installed on this machine", async () => {
    const report = await checkAgain();
    expect(report.found.map((f) => f.engineId)).toContain("claude");
    for (const pane of await paneIds()) {
      await $(`[data-testid="pane"][data-pane-id="${pane}"] button[aria-label^="Close "]`).click();
      await browser.pause(200);
    }
    await $('[data-testid="launch-grid"]').waitForExist({ timeout: 10_000 });
    await browser.waitUntil(
      async () => (await $$('[data-testid="launch-grid"] [data-testid^="tile-"]').length) >= 2,
      { timeout: 10_000 },
    );
    // Show the engines that aren't installed as well.
    await $('[data-testid="more-engines"]').click();
    await $('[data-testid^="missing-"]').waitForExist({ timeout: 5_000 });
    await settled();
    await browser.pause(400);
    await shotOf('[data-testid="launch-grid"]', "launch-grid", 32);
  });
});
