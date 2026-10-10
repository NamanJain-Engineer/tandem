import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { UiProvider } from "@tandem/ui";
import { useUi, type UpgradeGate } from "../../stores/ui";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { UpgradeDialog } from "./UpgradeDialog";

function open(gate?: UpgradeGate) {
  useUi.getState().openUpgrade(gate);
  render(
    <UiProvider>
      <UpgradeDialog />
    </UiProvider>,
  );
}

describe("UI-120 plans dialog gates (UI-122)", () => {
  beforeEach(() => {
    resetStores();
    fakeBackend();
  });

  it.each([
    [{ kind: "workspaces", limit: 3 }, "You hit the Free limit of 3 workspaces."],
    [{ kind: "panes", limit: 4 }, "You hit the Free limit of 4 panes."],
    [{ kind: "thread_grid", limit: 1 }, "Free shows 1 thread at a time."],
    [{ kind: "chat_engines", limit: 2 }, "Free threads run Claude Code and Codex. Pro runs every chat engine."],
  ] as Array<[UpgradeGate, string]>)("F-TH-06 each gate names its own limit (%o)", (gate, text) => {
    open(gate);
    expect(screen.getByTestId("upgrade-gate").textContent).toBe(text);
  });

  it("F-TH-06 the Free plan lists its thread engines", () => {
    open();
    const free = screen.getByRole("region", { name: "Free" });
    expect(free.textContent).toContain("Threads with Claude Code and Codex");
  });
});
