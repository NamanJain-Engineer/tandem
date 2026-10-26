import { $, browser } from "@wdio/globals";
import { expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";
import {
  addFolder,
  freshOffline,
  invoke,
  mod,
  relaunch,
  terminalText,
  sendToTerminal,
  typeKeys,
} from "../support/app";

// E2E-M2-5 Settings and theme (MILESTONES M2, F-SH-04 AC1–5, F-SH-05 AC2).
describe("E2E-M2-5 settings and theme", () => {
  it("F-SH-04.1 Mod+, opens Settings; F-SH-04.4 the filter finds scrollback; Esc restores mode and focus", async () => {
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-settings-${Date.now()}`);
    // The new shell takes focus once when it starts; let that happen first.
    await browser.waitUntil(async () => (await terminalText()).length > 0, { timeout: 15_000 });
    await browser.waitUntil(
      async () => (await browser.execute(() => !!document.activeElement?.closest(".xterm"))) === true,
      { timeout: 5_000 },
    );
    await browser.execute(() =>
      (
        document.querySelector(
          'nav[aria-label="Workspaces"] button[aria-label="Add folder…"]',
        ) as HTMLElement | null
      )?.focus(),
    );
    await browser.keys([mod(), ","]);
    await $('nav[aria-label="Settings pages"]').waitForExist({ timeout: 5_000 });
    await typeKeys("scroll");
    await $('[data-testid="settings-page-terminal"]').waitForExist({ timeout: 5_000 });
    await browser.waitUntil(
      async () => (await $('[data-setting="term_scrollback"]').getAttribute("data-highlighted")) === "true",
      { timeout: 3_000 },
    );
    await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
    await browser.keys("Escape");
    await $('nav[aria-label="Workspaces"]').waitForDisplayed({ timeout: 5_000 });
    const focused = await browser.execute(() => document.activeElement?.getAttribute("aria-label"));
    expect(focused).toBe("Add folder…");
  });

  it("F-SH-04.2 Light theme repaints the terminal within 200 ms with the same process", async () => {
    await browser.waitUntil(async () => (await terminalText()).length > 0, { timeout: 15_000 });
    const spawnsBefore = (await invoke<unknown[]>("test_spawn_log")).length;
    await browser.keys([mod(), ","]);
    await $("button=Appearance").click();
    const elapsed = (await browser.executeAsync((done: (ms: number) => void) => {
      const term = document.querySelector('[data-testid="terminal"]')?.parentElement as HTMLElement | null;
      const card = Array.from(document.querySelectorAll('[role="radio"]')).find(
        (el) => el.textContent?.trim() === "Light",
      ) as HTMLElement | undefined;
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

  it("F-SH-05.2 OSC 11 answers with the theme background", async () => {
    await sendToTerminal(
      "stty raw -echo; printf '\\033]11;?\\033\\\\'; head -c 25 | tr '\\033' E; stty sane\n",
    );
    await browser.waitUntil(async () => (await terminalText()).includes("E]11;rgb:ffff/ffff/ffff"), {
      timeout: 10_000,
      timeoutMsg: await terminalText(),
    });
  });

  it("every Settings page passes axe in both themes", async () => {
    await browser.keys([mod(), ","]);
    for (const page of [
      "General",
      "Appearance",
      "Terminal",
      "Privacy & Telemetry",
      "Account",
      "Advanced",
    ]) {
      await $(`button=${page}`).click();
      await $(`h1=${page}`).waitForExist({ timeout: 5_000 });
      await expectNoSeriousA11yViolationsInBothThemes();
    }
    await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
    await browser.keys("Escape");
  });

  it("F-SH-05.3 with Reduce Motion on, animations resolve to 0.01 ms or less", async () => {
    await invoke("settings_set", { key: "reduce_motion", value: "on" });
    await browser.waitUntil(
      async () => (await browser.execute(() => document.documentElement.dataset.motion)) === "reduced",
      { timeout: 5_000 },
    );
    const durations = (await browser.execute(() => {
      const probe = document.createElement("div");
      probe.innerHTML =
        '<span class="tn-breathe"></span><span class="tn-shimmer"></span><span class="tn-indeterminate"></span><span class="animate-spin"></span><div class="tn-dialog"></div>';
      document.body.appendChild(probe);
      const out = Array.from(probe.children).map((el) => getComputedStyle(el).animationDuration);
      probe.remove();
      return out;
    })) as string[];
    for (const d of durations)
      expect(parseFloat(d) * (d.endsWith("ms") ? 1 : 1000)).toBeLessThanOrEqual(0.01);
    await invoke("settings_set", { key: "reduce_motion", value: "system" });
  });

  it("F-SH-04.5 settings persist across a relaunch", async () => {
    const values: Record<string, unknown> = {
      theme: "light",
      term_scrollback: 5000,
      term_cursor_style: "block",
      density: "comfortable",
      reduce_motion: "on",
      launch_at_login: true,
      telemetry_crash: false,
    };
    for (const [key, value] of Object.entries(values)) await invoke("settings_set", { key, value });
    await relaunch();
    const all = await invoke<Record<string, unknown>>("settings_get_all");
    for (const [key, value] of Object.entries(values)) expect(all[key]).toEqual(value);
    expect(await browser.execute(() => document.documentElement.dataset.theme)).toBe("light");
    await invoke("settings_set", { key: "theme", value: "dark" });
    await invoke("settings_set", { key: "launch_at_login", value: false });
  });
});
