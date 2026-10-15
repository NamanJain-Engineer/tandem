import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { EngineVersionView } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { useInstall } from "../code/InstallSheet";
import { EnginesPage } from "./EnginesPage";

const codexView = {
  spec: { id: "codex", label: "Codex", vendor: "OpenAI", logo: "X", commands: ["codex"], newlineSequence: "\u001b\r", note: null },
  install: { path: "/usr/local/bin/codex", version: "1.3.0", source: "shell" },
  chatCapable: true,
  unsupportedReason: null,
  extraArgs: [],
};

function version(over: Partial<EngineVersionView> = {}): EngineVersionView {
  return {
    engineId: "codex",
    version: "1.3.0",
    method: "npm",
    selfUpdates: false,
    pinnable: true,
    pinned: false,
    previous: "1.2.0",
    lastGood: "1.2.0",
    broken: null,
    ...over,
  };
}

let calls: Call[];
function open(handlers: Record<string, (a?: Record<string, unknown>) => unknown>) {
  calls = fakeBackend({
    engines_list: () => [codexView],
    engines_report: () => null,
    engine_background: () => [],
    ...handlers,
  });
  render(
    <UiProvider>
      <EnginesPage />
    </UiProvider>,
  );
}

const details = async () => fireEvent.click(await screen.findByRole("button", { name: /Codex/ }));

describe("UI-161 engine versions (F-EN-08)", () => {
  beforeEach(() => {
    resetStores();
    useInstall.getState().close();
  });

  it("F-EN-08.1 shows the version and install method, and asks npm only when Check for updates is clicked", async () => {
    open({ engine_versions: () => [version()], engine_check_update: () => ({ latest: "1.4.0", how: null }) });
    await waitFor(() => expect(screen.getByTestId("engine-version-codex").textContent).toBe("1.3.0 · npm"));
    await details();
    expect(calls.some((c) => c.cmd === "engine_check_update")).toBe(false);
    await act(async () => fireEvent.click(screen.getByTestId("engine-check-codex")));
    expect(calls.find((c) => c.cmd === "engine_check_update")?.args).toEqual({ engineId: "codex" });
    expect((await screen.findByTestId("engine-latest-codex")).textContent).toBe("1.4.0 is available");
  });

  it("F-EN-08.2 Pin turns the engine's switch on for new panes", async () => {
    let pinned = false;
    open({
      engine_versions: () => [version({ pinned })],
      engine_pin: (a) => {
        pinned = Boolean(a?.on);
        return null;
      },
    });
    await waitFor(() => expect(screen.getByTestId("engine-version-codex").textContent).toContain("npm"));
    await details();
    const pin = await screen.findByRole("switch", { name: "Pin this version" });
    await act(async () => fireEvent.click(pin));
    expect(calls.find((c) => c.cmd === "engine_pin")?.args).toEqual({ engineId: "codex", on: true });
  });

  it("F-EN-08.3 an update that broke threads names both versions and Roll back shows the command first", async () => {
    open({ engine_versions: () => [version({ broken: { from: "1.2.0", to: "1.3.0" } })] });
    expect((await screen.findByTestId("engine-broken-codex")).textContent).toBe(
      "Codex updated from 1.2.0 to 1.3.0, and threads have failed since.",
    );
    fireEvent.click(screen.getByRole("button", { name: "Roll back to 1.2.0…" }));
    expect(useInstall.getState().rollback).toBe("1.2.0");
    expect(useInstall.getState().engine?.spec.id).toBe("codex");
    // Nothing ran: the sheet shows the command and waits for Run.
    expect(calls.some((c) => c.cmd === "pty_write")).toBe(false);
  });

  it("F-EN-08.4 a background engine process is listed, and Stop asks first", async () => {
    open({
      engine_versions: () => [version()],
      engine_background: () => [{ pid: 4242, engineId: "codex", command: "node codex.js daemon" }],
      engine_background_stop: () => null,
    });
    const row = await screen.findByTestId("engine-background-row");
    expect(row.textContent).toContain("node codex.js daemon");
    fireEvent.click(within(row).getByRole("button", { name: "Stop" }));
    expect(calls.some((c) => c.cmd === "engine_background_stop")).toBe(false);
    const dialog = await screen.findByRole("alertdialog");
    await act(async () => fireEvent.click(within(dialog).getByRole("button", { name: "Stop" })));
    await waitFor(() => expect(calls.find((c) => c.cmd === "engine_background_stop")?.args).toEqual({ pid: 4242 }));
  });

  it("F-EN-08.4 the empty and unavailable states say so", async () => {
    open({
      engine_versions: () => [],
      engine_background: () => {
        throw { code: "unavailable", message: "Listing background processes isn't available on this system yet" };
      },
    });
    expect(await screen.findByText("Listing background processes isn't available on this system yet")).toBeTruthy();
  });
});
