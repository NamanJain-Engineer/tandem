import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { UiProvider } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { followHealth, resetHealth } from "../../stores/health";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { Banners } from "./Banners";

describe("watchdogs in the UI (§5.2)", () => {
  let calls: Call[];
  beforeEach(() => {
    resetStores();
    resetHealth();
    calls = fakeBackend({ app_copy_diagnostics: () => "ok" });
  });

  it("§5.2 a subsystem the watchdog gave up on shows an error banner that names it, with Copy diagnostics", async () => {
    followHealth();
    render(
      <UiProvider>
        <Banners />
      </UiProvider>,
    );
    expect(screen.queryByTestId("banner-failed:gateway")).toBeNull();
    act(() => emitLocal("app:subsystem-failed", { name: "gateway", restarts: 3, lastError: "listener closed" }));
    const banner = await screen.findByTestId("banner-failed:gateway");
    expect(banner.textContent).toBe(
      "Part of Vibezeo stopped working (gateway) and didn't recover after 3 restarts. Restart Vibezeo to bring it back.",
    );
    fireEvent.click(screen.getByRole("button", { name: "Copy diagnostics" }));
    await act(async () => undefined);
    expect(calls.some((c) => c.cmd === "app_copy_diagnostics")).toBe(true);
    // Dismissing it hides it; another failure shows again.
    // The banner's own Dismiss, not the "copied" toast's: the nearest
    // ancestor that holds the banner text.
    const dismiss = screen.getAllByRole("button", { name: "Dismiss" }).find((b) => {
      let n: HTMLElement | null = b.parentElement;
      for (let i = 0; i < 3 && n; i++, n = n.parentElement)
        if (n.querySelector('[data-testid="banner-failed:gateway"]')) return true;
      return false;
    });
    if (!dismiss) throw new Error("no Dismiss on the banner");
    fireEvent.click(dismiss);
    expect(screen.queryByTestId("banner-failed:gateway")).toBeNull();
    act(() => emitLocal("app:subsystem-failed", { name: "status", restarts: 3, lastError: "panicked" }));
    expect(await screen.findByTestId("banner-failed:status")).toBeTruthy();
  });
});
