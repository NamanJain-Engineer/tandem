import { $, browser } from "@wdio/globals";
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, rmSync, statSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  checkAgain,
  crash,
  freshOffline,
  paneIds,
  profileWithoutEngines,
  relaunch,
  setProfile,
  settled,
  split,
  waitForApp,
  waitForPaneText,
} from "../support/app";

// The public site's demo video (apps/web/public/screens/demo.{webm,mp4}): a real,
// unedited screen recording of the app. Not a test: run it on its own, under
// a display, with ffmpeg and the real `claude` CLI on the machine (detected
// only, never launched or signed in):
//
//   dbus-run-session -- xvfb-run -a -s "-screen 0 1920x1080x24" \
//     pnpm --filter desktop e2e -- --spec e2e/screens/demo.screens.ts
//
// Storyboard: a new tab finds Claude Code; four terminals over a clone of
// this repository; kill -9; relaunch, and the layout comes back.

const repo = new URL("../../../..", import.meta.url).pathname.replace(/\/$/, "");
const out = process.env.TANDEM_SCREENS_DIR ?? join(repo, "apps/web/public/screens");
const enginePath = process.env.TANDEM_SCREENS_PATH ?? "/opt/node22/bin";
const W = 1440;
const H = 900;

const pause = (ms: number) => browser.pause(ms);

/** Types into a pane like a person would, so the recording is followable. */
async function typeIn(paneId: string, text: string): Promise<void> {
  await $(`[data-testid="terminal"][data-pane-id="${paneId}"]`).click();
  // One action per key: WebKitWebDriver merges repeated keys in one action.
  for (const ch of text) {
    await browser.keys(ch);
    await pause(18);
  }
  await browser.keys("Enter");
}

async function placeWindow(): Promise<void> {
  await browser.setWindowRect(0, 0, W, H);
  await settled();
}

describe("demo video", () => {
  const work = `/tmp/tandem-demo-${Date.now()}`;
  const raw = join(work, "raw.mp4");
  let ffmpeg: ChildProcess | null = null;

  before(() => {
    mkdirSync(out, { recursive: true });
    execFileSync("git", ["clone", "-q", "--depth", "80", `file://${repo}`, join(work, "tandem")]);
    const cargoHome = process.env.CARGO_HOME ?? join(process.env.TANDEM_REAL_HOME ?? "/root", ".cargo");
    const rustupHome = process.env.RUSTUP_HOME ?? join(process.env.TANDEM_REAL_HOME ?? "/root", ".rustup");
    setProfile([
      ...profileWithoutEngines(),
      `export PATH=${enginePath}:${cargoHome}/bin:/usr/local/bin:/usr/bin:/bin`,
      `export CARGO_HOME=${cargoHome} RUSTUP_HOME=${rustupHome} CARGO_TARGET_DIR=${join(repo, "target")}`,
    ]);
  });

  after(() => {
    ffmpeg?.kill("SIGINT");
    setProfile(null);
    rmSync(work, { recursive: true, force: true });
  });

  it("records launch, four terminals, a crash and the restore", async function () {
    this.timeout(600_000);
    await freshOffline();
    await addFolder(join(work, "tandem"));
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    // Detect again with this machine's PATH: the real `claude`, not the
    // harness's fake engine, must be what the video shows. The launch grid
    // reads the result when it opens, so detect before emptying the tab.
    const report = await checkAgain();
    expect(report.found.map((f) => f.engineId)).toContain("claude");
    // The UI reads detection results when it loads.
    await browser.refresh();
    await waitForApp();
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    // Start from an empty tab: the launch grid with the engines found.
    const [shell] = await paneIds();
    await $(`[data-testid="pane"][data-pane-id="${shell}"] button[aria-label^="Close "]`).click();
    await $('[data-testid="tile-claude"]').waitForExist({ timeout: 10_000 });
    const real =
      execFileSync(join(enginePath, "claude"), ["--version"], { encoding: "utf8" }).split(" ")[0] ?? "";
    const tileText = () =>
      browser.execute(() => document.querySelector('[data-testid="tile-claude"]')?.textContent ?? "");
    await browser.waitUntil(async () => (await tileText()).includes(real), {
      timeout: 10_000,
      timeoutMsg: `the launch grid doesn't show claude ${real}`,
    });
    await placeWindow();
    await pause(800);

    ffmpeg = spawn(
      "ffmpeg",
      ["-y", "-loglevel", "error", "-f", "x11grab", "-framerate", "30", "-video_size", `${W}x${H}`]
        .concat(["-i", `${process.env.DISPLAY ?? ":0"}+0,0`, "-c:v", "libx264", "-preset", "veryfast"])
        .concat(["-crf", "18", "-pix_fmt", "yuv420p", raw]),
      { stdio: ["pipe", "ignore", "inherit"] },
    );
    // x11grab takes a moment to start: hold the opening shot once frames
    // are actually being written.
    await browser.waitUntil(async () => existsSync(raw) && statSync(raw).size > 0, {
      timeout: 15_000,
      interval: 100,
    });
    await pause(3_000);

    // 1. Open a terminal from the launch grid, look at the history.
    await $('[data-testid="tile-shell"]').click();
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 10_000 });
    const [first] = await paneIds();
    await waitForPaneText(first ?? "", "#", 20_000);
    await typeIn(first ?? "", "git --no-pager log --oneline --decorate --color=always -n 14");
    await pause(1_500);
    // 2. Three more terminals: htop, the Claude Code hook handler, the tests.
    const right = await split("right");
    await typeIn(right, "htop");
    await pause(1_200);
    const belowRight = await split("down");
    await typeIn(belowRight, "cargo test -q -p tandem-core --lib 2>&1 | tail -n 12");
    await $(`[data-testid="pane"][data-pane-id="${first}"]`).click();
    const belowLeft = await split("down");
    await typeIn(belowLeft, "vim +/PreToolUse crates/tandem-engines/src/hook.rs");
    await waitForPaneText(belowRight, "test result", 240_000);
    await pause(3_500);

    // 3. kill -9, then 4. relaunch: the layout and the shells come back.
    crash();
    await pause(1_500);
    await relaunch();
    await placeWindow();
    await browser.waitUntil(async () => (await paneIds()).length === 4, { timeout: 30_000 });
    await waitForApp();
    await pause(5_000);

    // Stop cleanly so the file is finalized.
    ffmpeg.stdin?.write("q");
    await new Promise<void>((done) => ffmpeg?.once("exit", () => done()));
    ffmpeg = null;
    // Web-sized copy: 1280 px wide, no audio, streamable.
    execFileSync("ffmpeg", [
      "-y",
      "-loglevel",
      "error",
      "-i",
      raw,
      "-vf",
      "scale=1280:-2",
      "-c:v",
      "libx264",
      "-preset",
      "slow",
      "-crf",
      "27",
      "-pix_fmt",
      "yuv420p",
      "-movflags",
      "+faststart",
      "-an",
      join(out, "demo.mp4"),
    ]);
    // VP9 too: open-source Chromium builds can't play H.264.
    execFileSync("ffmpeg", [
      "-y",
      "-loglevel",
      "error",
      "-i",
      raw,
      "-vf",
      "scale=1280:-2",
      "-c:v",
      "libvpx-vp9",
      "-crf",
      "36",
      "-b:v",
      "0",
      "-row-mt",
      "1",
      "-an",
      join(out, "demo.webm"),
    ]);
  });
});
