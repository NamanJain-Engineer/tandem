import { execFileSync } from "node:child_process";
import { $, browser } from "@wdio/globals";
import { freshUser } from "../support/app";

// E2E-M2-8 Diagnostics (MILESTONES M2, F-SH-11 AC2).
describe("E2E-M2-8 diagnostics", () => {
  it("Help → Copy diagnostics puts version, OS and recent lines on the clipboard without home paths", async () => {
    await freshUser("diag");
    await $('button[aria-label="Vibezeo menu"]').click();
    await $('//*[@role="menuitem"][normalize-space()="Copy diagnostics"]').click();
    let clip = "";
    await browser.waitUntil(
      () => {
        clip = execFileSync("xclip", ["-o", "-selection", "clipboard"], { encoding: "utf8" });
        return clip.startsWith("Vibezeo ");
      },
      { timeout: 5_000, timeoutMsg: "clipboard has no diagnostics" },
    );
    expect(clip).toMatch(/^Vibezeo \S+ \(\S+\)\n/);
    expect(clip).toContain("OS: linux");
    expect(clip).toContain("Last 200 log lines:");
    expect(clip).toMatch(/auth\.signed_in/);
    expect(clip).not.toMatch(/\/home\/|\/root\//);
  });
});
