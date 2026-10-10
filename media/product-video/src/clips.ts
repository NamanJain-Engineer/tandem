/**
 * Where each scene's footage comes from: one continuous screen recording of
 * the released Vibezeo 0.1.0 Linux build (public/clips/app.mp4, recorded by
 * capture/record.sh). Times are seconds in that recording; a `rate` above 1
 * is shown on screen as a speed tag.
 */
export const APP = "clips/app.mp4";
export const APP_ASPECT = 1.6;

export const CLIPS = {
  // Launch grid: hover Claude Code (11.1–13), then Terminal (13–15.3).
  launchGrid: { from: 9.5, rate: 1 },
  // Terminal, git recent, three splits through the pane picker, htop, tests, vim.
  terminals: { from: 15.0, rate: 2.3 },
  // Four panes, just before the kill -9 at 52.9.
  crashBefore: { from: 44.5, rate: 1 },
  // Relaunch from nothing at 55.1: the window at 56.5, the shells back by 61.
  restore: { from: 55.4, rate: 1 },
};
