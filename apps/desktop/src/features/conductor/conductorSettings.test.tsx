import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ConductorMemory, IpcError, WakeStatus } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { emitLocal } from "../../ipc/events";
import { FREE_VIEW, fakeBackend, resetStores } from "../../test/fakeBackend";
import { waiting } from "../../test/waiting";
import { useSession } from "../../stores/session";
import { useSettings } from "../../stores/settings";
import { SettingsView } from "../settings/SettingsView";

// Settings › Conductor shows its rows only with an account that carries
// Conductor (hosted, D-2913); without one it only says so (download.test.tsx).
beforeEach(() => {
  resetStores();
  useSession.setState({ entitlement: { ...FREE_VIEW, limits: { ...FREE_VIEW.limits, conductor: true } } });
});
afterEach(() => setInvokeBackend(null));

const memory = (over: Partial<ConductorMemory> = {}): ConductorMemory => ({
  enabled: false,
  text: "",
  facts: [],
  proposals: [],
  path: "/home/u/.tandem/conductor/memory.md",
  ...over,
});
const listening: WakeStatus = { state: { state: "listening" }, unavailable: null };

const open = (values: Record<string, unknown> = {}) => {
  useSettings.setState({ values: { settings_last_page: "conductor", ...values }, loaded: true });
  return render(
    <UiProvider>
      <SettingsView />
    </UiProvider>,
  );
};

describe("Settings › Conductor (UI-67, F-CN-06, F-CN-07)", () => {
  it("F-CN-06 shows the persona rows with their defaults", async () => {
    fakeBackend({ conductor_memory_get: () => memory(), conductor_wake_status: () => listening });
    open();
    expect(screen.getByRole("heading", { name: "Conductor" })).toBeTruthy();
    expect((screen.getByLabelText("Name") as HTMLInputElement).value).toBe("Conductor");
    expect(screen.getByRole("radio", { name: "Neutral" }).getAttribute("aria-checked")).toBe("true");
    // "How much it says" and "Speech speed" both default to Normal.
    const normals = screen.getAllByRole("radio", { name: "Normal" });
    expect(normals.map((r) => r.getAttribute("aria-checked"))).toEqual(["true", "true"]);
    expect(screen.getByLabelText("Tell it about you")).toBeTruthy();
    await screen.findByText("Memory is off");
  });

  it("F-CN-06 an empty name goes back to Conductor and speed saves a number", async () => {
    const calls = fakeBackend({ conductor_memory_get: () => memory(), conductor_wake_status: () => listening });
    open({ conductor_name: "Ada" });
    const name = screen.getByLabelText("Name") as HTMLInputElement;
    fireEvent.change(name, { target: { value: "  " } });
    fireEvent.blur(name);
    await waitFor(() =>
      expect(calls.some((c) => c.cmd === "settings_set" && c.args?.key === "conductor_name" && c.args.value === "Conductor")).toBe(true),
    );
    fireEvent.click(screen.getByRole("radio", { name: "Fastest" }));
    await waitFor(() =>
      expect(calls.some((c) => c.cmd === "settings_set" && c.args?.key === "conductor_speed" && c.args.value === 5)).toBe(true),
    );
  });

  it("F-CN-06 memory is viewable, editable and clearable", async () => {
    let current = memory({ enabled: true, text: "# Conductor memory\n\n- Works on billing\n", facts: ["Works on billing"] });
    const calls = fakeBackend({
      conductor_memory_get: () => current,
      conductor_memory_save: (a) => {
        current = memory({ enabled: true, text: String(a?.text), facts: ["Works on billing", "Likes tea"] });
        return current;
      },
      conductor_memory_clear: () => {
        current = memory({ enabled: true });
        return current;
      },
      conductor_wake_status: () => listening,
    });
    open({ conductor_memory: true });
    const editor = (await screen.findByLabelText("memory.md")) as HTMLTextAreaElement;
    expect(editor.value).toContain("- Works on billing");
    fireEvent.change(editor, { target: { value: editor.value + "- Likes tea\n" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "conductor_memory_save")).toBe(true));
    fireEvent.click(screen.getByRole("button", { name: "Forget everything…" }));
    fireEvent.click(await screen.findByRole("button", { name: "Forget everything" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "conductor_memory_clear")).toBe(true));
    await waitFor(() => expect((screen.getByLabelText("memory.md") as HTMLTextAreaElement).value).toBe(""));
  });

  it("F-CN-06.2 a credential is refused with the UI-67 copy", async () => {
    const refused: IpcError = {
      code: "invalid",
      message: "That looks like a secret, so Conductor won't remember it.",
      stage: null,
      hint: null,
      retryable: false,
    };
    fakeBackend({
      conductor_memory_get: () => memory({ enabled: true }),
      conductor_memory_save: () => {
        throw refused;
      },
      conductor_wake_status: () => listening,
    });
    open({ conductor_memory: true });
    const editor = await screen.findByLabelText("memory.md");
    fireEvent.change(editor, { target: { value: "- my token is abc" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("That looks like a secret, so Conductor won't remember it.");
  });

  it("F-CN-06 proposed memories can be kept", async () => {
    const calls = fakeBackend({
      conductor_memory_get: () => memory({ enabled: true, proposals: [{ id: "mem_1", text: "Prefers Rust" }] }),
      conductor_memory_resolve: () => memory({ enabled: true, text: "- Prefers Rust\n", facts: ["Prefers Rust"] }),
      conductor_wake_status: () => listening,
    });
    open({ conductor_memory: true });
    await screen.findByText("Prefers Rust");
    const keep = screen.getByTestId("conductor-proposal-keep");
    await waitFor(() => expect(waiting(keep)).toBe(false));
    // A double-click keeps it once (the review of ce0d5815).
    fireEvent.click(keep);
    fireEvent.click(keep);
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "conductor_memory_resolve")?.args).toEqual({ proposalIds: ["mem_1"], keep: true }),
    );
    expect(calls.filter((c) => c.cmd === "conductor_memory_resolve")).toHaveLength(1);
  });

  it("F-TL-12.16 Keep waits until its row has held still: the next proposal slides into the one resolved", async () => {
    const two = [
      { id: "mem_1", text: "Prefers Rust" },
      { id: "mem_2", text: "Deploys on Fridays" },
    ];
    const calls = fakeBackend({
      conductor_memory_get: () => memory({ enabled: true, proposals: two }),
      conductor_memory_resolve: () => memory({ enabled: true, proposals: [two[1]!] }),
      conductor_wake_status: () => listening,
    });
    // jsdom lays nothing out: each row is 40 px below the one before it,
    // and what's in a row sits where the row does.
    const rects = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
      this: HTMLElement,
    ) {
      const row = this.closest<HTMLElement>('[data-testid="conductor-proposal"]');
      const rows = Array.from(document.querySelectorAll<HTMLElement>('[data-testid="conductor-proposal"]'));
      const top = row ? 40 * rows.indexOf(row) : 0;
      return { left: 0, top, width: 300, height: 32, right: 300, bottom: top + 32, x: 0, y: top, toJSON: () => ({}) } as DOMRect;
    });
    open({ conductor_memory: true });
    await screen.findByText("Prefers Rust");
    expect(
      screen.getAllByTestId("conductor-proposal-keep").every((b) => b.getAttribute("data-grant") !== null),
    ).toBe(true);
    // Both rows have held still: the second's Keep is on.
    const second = () => screen.getAllByTestId("conductor-proposal-keep").at(-1)!;
    await waitFor(() => expect(waiting(second())).toBe(false));
    // Discard the first: the second slides into its place, and its Keep
    // waits again (the review of ce0d5815: it was still in its first wait).
    fireEvent.click(screen.getAllByRole("button", { name: "Discard" })[0]!);
    await waitFor(() => expect(screen.getAllByTestId("conductor-proposal")).toHaveLength(1));
    const keep = screen.getByTestId("conductor-proposal-keep");
    await waitFor(() => expect(waiting(keep)).toBe(true));
    expect(keep.getAttribute("title")).toBe("Keep turns on once this has been still on screen for a moment.");
    // A double-click's second click lands on it: nothing is kept.
    fireEvent.click(keep);
    expect(calls.filter((c) => c.cmd === "conductor_memory_resolve")).toHaveLength(1);
    await waitFor(() => expect(waiting(screen.getByTestId("conductor-proposal-keep"))).toBe(false));
    rects.mockRestore();
  });

  it("F-CN-06 loading and error states", async () => {
    let fail = true;
    fakeBackend({
      conductor_memory_get: () => {
        if (fail) throw { code: "internal", message: "disk", stage: null, hint: null, retryable: false };
        return memory();
      },
      conductor_wake_status: () => listening,
    });
    open();
    expect(screen.getByTestId("conductor-memory-loading")).toBeTruthy();
    await screen.findByText("Couldn't read Conductor's memory");
    fail = false;
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await screen.findByText("Memory is off");
  });

  it("F-CN-07 the wake switch shows live state and pauses", async () => {
    fakeBackend({ conductor_memory_get: () => memory(), conductor_wake_status: () => listening });
    open({ conductor_wake: true });
    expect(await screen.findByText("Listening")).toBeTruthy();
    act(() => emitLocal<WakeStatus>("conductor:wake-state", { state: { state: "paused", reason: "call" }, unavailable: null }));
    expect(await screen.findByText("Paused during a call")).toBeTruthy();
  });

  it("F-CN-07 ships disabled: an unavailable build can't turn it on", async () => {
    fakeBackend({
      conductor_memory_get: () => memory(),
      conductor_wake_status: (): WakeStatus => ({
        state: { state: "unavailable", reason: "benchmark_failed" },
        unavailable: "benchmark_failed",
      }),
    });
    open({ conductor_wake: true });
    await screen.findByText("Turned off: it didn't pass its false-trigger test");
    const sw = screen.getByRole("switch", { name: 'Wake phrase "Hey Conductor"' });
    expect(sw.hasAttribute("disabled")).toBe(true);
    expect(sw.getAttribute("aria-checked")).toBe("false");
  });
});
