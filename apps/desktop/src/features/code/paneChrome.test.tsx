// D-2603 (M18 · Developer-first UI): the pane header shows how long an agent
// has been at it, and its actions keep their shortcuts in their tooltips.
import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { UiProvider } from "@tandem/ui";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { PaneChrome } from "./PaneChrome";

const NOW = Date.UTC(2026, 9, 20, 12);

function show(over: Partial<Parameters<typeof PaneChrome>[0]> = {}) {
  return render(
    <UiProvider>
      <PaneChrome
        title="Fix the redirect loop"
        status="working"
        focused
        solo={false}
        maximized={false}
        menuItems={[]}
        onMaximize={() => undefined}
        onClose={() => undefined}
        {...over}
      />
    </UiProvider>,
  );
}

describe("§2.20 pane header (D-2603)", () => {
  beforeEach(() => {
    resetStores();
    fakeBackend();
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(NOW);
  });
  afterEach(() => vi.useRealTimers());

  it("shows the time an agent has been working, and waiting in amber", () => {
    const { unmount } = show({ since: NOW - 4 * 60_000 });
    const el = screen.getByTestId("pane-elapsed");
    expect(el.textContent).toBe("4m");
    expect(el.getAttribute("title")).toBe("Working · 4m");
    unmount();
    show({ status: "waiting", since: NOW - 2 * 3_600_000 });
    expect(screen.getByTestId("pane-elapsed").className).toMatch(/text-waiting-text/);
  });

  it("shows no time for a pane that isn't working or waiting", () => {
    show({ status: "idle", since: null });
    expect(screen.queryByTestId("pane-elapsed")).toBeNull();
  });

  it("keeps its actions reachable and named", () => {
    show();
    expect(screen.getByRole("button", { name: "Maximize pane" })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Close/ })).toBeTruthy();
  });
});
