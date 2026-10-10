import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { UiProvider } from "@tandem/ui";
import { useBoot } from "../shell/useBoot";
import { useSettings } from "../../stores/settings";
import { BOOT, fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { WhatsNew, notesForApp } from "./WhatsNew";

const CHANGELOG = `# Changelog

## 2.0.0

The big one.

### Added
- [lin] **Threads.** On Linux.
- [mac] A macOS thing.

### Fixed
- [all] A fix everywhere.

## 1.0.0

### Added
- [all] First.
`;

function mount() {
  return render(
    <UiProvider>
      <WhatsNew changelog={CHANGELOG} />
    </UiProvider>,
  );
}

describe("What's new after an update (F-WB-03)", () => {
  let calls: Call[] = [];
  beforeEach(() => {
    resetStores();
    calls = fakeBackend();
    useBoot.setState({ boot: { ...BOOT, version: "2.0.0", platform: "linux" } });
  });

  it("F-WB-03.2 after an update it shows this version's notes for this platform, once", async () => {
    useSettings.setState({ values: { whats_new_seen: "1.0.0" }, loaded: true });
    const first = mount();
    const dialog = await screen.findByRole("dialog");
    expect(dialog.textContent).toContain("What's new in Vibezeo 2.0.0");
    expect(dialog.textContent).toContain("Threads.");
    expect(dialog.textContent).toContain("A fix everywhere.");
    expect(dialog.textContent).not.toContain("A macOS thing.");
    fireEvent.click(screen.getByRole("button", { name: "Got it" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(calls.find((c) => c.cmd === "settings_set")?.args).toEqual({
      key: "whats_new_seen",
      value: "2.0.0",
    });
    first.unmount();
    // The next launch of the same version doesn't show it again.
    mount();
    await act(async () => {});
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("F-WB-03.2 a first install shows nothing and remembers the version", async () => {
    useSettings.setState({ values: {}, loaded: true });
    mount();
    await waitFor(() => expect(calls.some((c) => c.cmd === "settings_set")).toBe(true));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(useSettings.getState().values.whats_new_seen).toBe("2.0.0");
  });

  it("F-WB-03 a version without a CHANGELOG section (a dev build) shows nothing", async () => {
    useBoot.setState({ boot: { ...BOOT, version: "2.1.0-dev", platform: "linux" } });
    useSettings.setState({ values: { whats_new_seen: "2.0.0" }, loaded: true });
    mount();
    await act(async () => {});
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("F-WB-03 notes pick the platform: macOS gets its own bullets and [all]", () => {
    expect(notesForApp(CHANGELOG, "2.0.0", "macos")?.sections.flatMap((s) => s.items)).toEqual([
      "A macOS thing.",
      "A fix everywhere.",
    ]);
  });
});
