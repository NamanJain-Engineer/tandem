import { $, browser } from "@wdio/globals";
import { addFolder, freshUser, invoke, mod, relaunch, terminalText, typeInTerminal } from "../support/app";

// E2E-M2-5 Settings and theme (MILESTONES M2, F-SH-04 AC1–5, F-SH-05 AC2).
describe("E2E-M2-5 settings and theme", () => {
  it("Mod+, opens Settings; the filter finds scrollback; Esc restores mode and focus", async () => {
    await freshUser("settings");
    await addFolder(`/tmp/tandem-e2e-settings-${Date.now()}`);
    await browser.execute(() => (document.querySelector('nav[aria-label="Workspaces"] button[aria-label="Add folder…"]') as HTMLElement | null)?.focus());
    await browser.keys([mod(), ","]);
    await $('nav[aria-label="Settings pages"]').waitForExist({ timeout: 5_000 });
    await browser.keys(["s", "c", "r", "o", "l", "l"].flatMap((c) => [c]));
    await $('[data-testid="settings-page-terminal"]').waitForExist({ timeout: 5_000 });
    await browser.waitUntil(async () => (await $('[data-setting="term_scrollback"]').getAttribute("data-highlighted")) === "true", { timeout: 3_000 });
    await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
    await browser.keys("Escape");
    await $('nav[aria-label="Workspaces"]').waitForDisplayed({ timeout: 5_000 });
    const focused = await browser.execute(() => document.activeElement?.getAttribute("aria-label"));
    expect(focused).toBe("Add folder…");
  });

  it("Light theme repaints the terminal within 200 ms with the same process", async () => {
    await browser.waitUntil(async () => (await terminalText()).length > 0, { timeout: 15_000 });
    const spawnsBefore = (await invoke<unknown[]>("test_spawn_log")).length;
    await browser.keys([mod(), ","]);
    await $("button=Appearance").click();
    const elapsed = (await browser.executeAsync((done: (ms: number) => void) => {
      const term = document.querySelector('[data-testid="terminal"]')?.parentElement as HTMLElement | null;
      const card = Array.from(document.querySelectorAll('[role="radio"]')).find((el) => el.textContent?.trim() === "Light") as HTMLElement | undefined;
      if (!term || !card) return done(-1);
      const t0 = performance.now();
      card.click();
      const tick = () => {
        if (getComputedStyle(term).backgroundColor === "rgb(255, 255, 255)") done(performance.now() - t0);
        else if (performance.now() - t0 > 2000) done(9999);
        else requestAnimationFrame(tick);
      };
      tick();
    })) as number;
    expect(elapsed).toBeGreaterThanOrEqual(0);
    expect(elapsed).toBeLessThan(200);
    expect((await invoke<unknown[]>("test_spawn_log")).length).toBe(spawnsBefore);
    await browser.keys("Escape");
  });

  it("OSC 11 answers with the theme background", async () => {
    await typeInTerminal("stty raw -echo; printf '\\033]11;?\\033\\\\'; head -c 25 | tr '\\033' E; stty sane\n");
    await browser.waitUntil(async () => (await terminalText()).includes("E]11;rgb:ffff/ffff/ffff"), { timeout: 10_000, timeoutMsg: await terminalText() });
  });

  it("settings persist across a relaunch", async () => {
    const values: Record<string, unknown> = { theme: "light", term_scrollback: 5000, term_cursor_style: "block", density: "comfortable", reduce_motion: "on", launch_at_login: true, telemetry_crash: false };
    for (const [key, value] of Object.entries(values)) await invoke("settings_set", { key, value });
    await relaunch();
    const all = await invoke<Record<string, unknown>>("settings_get_all");
    for (const [key, value] of Object.entries(values)) expect(all[key]).toEqual(value);
    expect(await browser.execute(() => document.documentElement.dataset.theme)).toBe("light");
    await invoke("settings_set", { key: "theme", value: "dark" });
    await invoke("settings_set", { key: "launch_at_login", value: false });
  });
});
