// UI-10 F-CD-13 (D-2603): the launch grid's keys and copyable install commands.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { EngineView } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useEngines } from "../../stores/engines";
import { BOOT, fakeBackend, resetStores } from "../../test/fakeBackend";
import { useBoot } from "../shell/useBoot";
import { LaunchGrid } from "./LaunchGrid";

const copied: string[] = [];
vi.mock("../../ipc/clipboard", () => ({
  copyText: async (text: string) => {
    copied.push(text);
    return true;
  },
}));

const engine = (id: string, label: string, logo: string, installed: boolean): EngineView =>
  ({
    spec: {
      id,
      label,
      vendor: "",
      logo,
      commands: [id],
      install: { lin: `npm install -g ${id}-cli`, mac: `brew install ${id}`, win: null },
    },
    install: installed ? { path: `/bin/${id}`, version: "1.0.0", source: "shell" } : null,
    chatCapable: true,
    unsupportedReason: null,
    extraArgs: [],
  }) as unknown as EngineView;

function show() {
  const picks: string[] = [];
  render(
    <UiProvider>
      <input aria-label="elsewhere" />
      <LaunchGrid tabId="tb_1" busy={null} onPick={(id) => picks.push(id)} />
    </UiProvider>,
  );
  return picks;
}

describe("UI-10 F-CD-13 launch grid keys and install commands (D-2603)", () => {
  beforeEach(() => {
    resetStores();
    fakeBackend();
    copied.length = 0;
    useBoot.setState({ boot: { ...BOOT, platform: "linux" } });
    useEngines.setState({
      loaded: true,
      list: [engine("claude", "Claude Code", "C", true), engine("codex", "Codex", "X", true), engine("kimi", "Kimi", "K", false)],
    });
  });

  it("a digit with focus in the grid launches the tile with that number", () => {
    const picks = show();
    const first = screen.getByTestId("tile-claude");
    first.focus();
    fireEvent.keyDown(first, { key: "2" });
    fireEvent.keyDown(first, { key: "3" });
    expect(picks).toEqual(["codex", "shell"]);
    expect(first.getAttribute("aria-keyshortcuts")).toBe("1");
  });

  it("Ctrl or Meta plus a digit doesn't launch, and neither does a digit outside the grid", () => {
    const picks = show();
    const first = screen.getByTestId("tile-claude");
    fireEvent.keyDown(first, { key: "1", ctrlKey: true });
    fireEvent.keyDown(first, { key: "1", metaKey: true });
    fireEvent.keyDown(screen.getByLabelText("elsewhere"), { key: "1" });
    expect(picks).toEqual([]);
  });

  it("the copy button copies exactly the command shown, and says so", async () => {
    show();
    fireEvent.click(screen.getByTestId("more-engines"));
    const shown = screen.getByTestId("install-command-kimi").querySelector("code")?.textContent;
    expect(shown).toBe("npm install -g kimi-cli");
    fireEvent.click(screen.getByRole("button", { name: "Copy the install command for Kimi" }));
    await waitFor(() => expect(copied).toEqual(["npm install -g kimi-cli"]));
    await waitFor(() => expect(screen.getByRole("status").textContent).toBe("Copied"));
  });
});
