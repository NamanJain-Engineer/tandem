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
//   GDK_SCALE=2 TANDEM_SCREENS_DIR=/tmp/screens dbus-run-session -- \
//     xvfb-run -a -s "-screen 0 3840x2160x24" \
//     pnpm --filter desktop e2e -- --spec e2e/screens/site.screens.ts
//
// Then copy code-mode.png to apps/web/media/code-mode@2x.png and run
// apps/web/scripts/optimize-screens.py (OPERATING §6).
//
// The workspace is a fresh clone of this repository, so the terminals show
// its real history, tests and code.

const repo = new URL("../../../..", import.meta.url).pathname.replace(/\/$/, "");
const out = process.env.TANDEM_SCREENS_DIR ?? join(repo, "apps/web/public/screens");
const enginePath = process.env.TANDEM_SCREENS_PATH ?? "/opt/node22/bin";

/** What this harness's build shows and the download never does: the debug
 * build's DEV badge (D-1838), and the status bar's connection state, which
 * only a hosted build has; the early-access download shows none (D-816). */
async function hideNotInDownload(): Promise<void> {
  await browser.execute(() => {
    for (const el of Array.from(document.querySelectorAll("header span, [data-tauri-drag-region] span"))) {
      if (el.textContent === "DEV" && el.children.length === 0) (el as HTMLElement).style.display = "none";
    }
    const connection = document.querySelector<HTMLElement>('[data-testid="connection-state"]');
    if (connection) connection.style.display = "none";
  });
}

async function shot(name: string): Promise<void> {
  await settled();
  await hideNotInDownload();
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
    execFileSync("git", ["clone", "-q", "--depth", "80", `file://${repo}`, join(work, "vibezeo")]);
    // On `main`, so no working branch's name shows in the rail.
    execFileSync("git", ["-C", join(work, "vibezeo"), "checkout", "-q", "-B", "main"]);
    // `git recent`: the product's own history (the app and its crates), one
    // line per commit, without merges, internal code names, release-channel
    // or dated subjects.
    execFileSync("git", [
      "-C",
      join(work, "vibezeo"),
      "config",
      "alias.recent",
      "!git --no-pager log --oneline --no-merges --color=always -n 14 -i --invert-grep --grep=tandem --grep=early --grep=chain --grep='20[0-9][0-9]' -- crates apps/desktop",
    ]);
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
    // 1440×900 points; under GDK_SCALE=2 the capture is 2880×1800.
    await browser.setWindowRect(0, 0, 1440, 900);
    const ws = join(work, "vibezeo");
    await addFolder(ws);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const [first] = await paneIds();
    const right = await split("right");
    const belowRight = await split("down");
    await $(`[data-testid="pane"][data-pane-id="${first}"]`).click();
    const belowLeft = await split("down");
    await sendToPane(first ?? "", "git recent\r");
    await sendToPane(right, "htop\r");
    // How Vibezeo reads Claude Code's hooks.
    await sendToPane(belowLeft, "(cd crates/*engines/src && vim +/PreToolUse hook.rs)\r");
    await sendToPane(belowRight, "(cd crates/*core && cargo test -q --lib 2>&1 | tail -n 12)\r");
    await waitForPaneText(first ?? "", "M18:", 20_000);
    await waitForPaneText(belowRight, "test result", 240_000);
    await shot("code-mode");
  });

  // The site no longer shows this capture (D-1885); it needs the real
  // `claude` CLI, so it runs only when asked.
  (process.env.TANDEM_SCREENS_LAUNCH_GRID ? it : it.skip)("the launch grid finds the engines installed on this machine", async () => {
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
