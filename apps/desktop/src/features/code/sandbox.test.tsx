import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { UiProvider } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { setInvokeBackend } from "../../ipc/invoke";
import { followSandbox, useSandbox } from "../../stores/sandbox";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { SandboxChip } from "./SandboxChip";
import { SandboxSetting } from "../settings/SandboxSetting";

beforeEach(() => resetStores());
afterEach(() => setInvokeBackend(null));

describe("agent sandbox (F-TL-11)", () => {
  it("F-TL-11.1 a sandboxed pane says so; one that couldn't be says why", async () => {
    fakeBackend({ sandbox_panes: () => [] });
    const stop = followSandbox();
    emitLocal("sandbox:pane", { paneId: "pn_1", sandboxed: true, reason: null });
    const { rerender } = render(
      <UiProvider>
        <SandboxChip paneId="pn_1" />
      </UiProvider>,
    );
    const chip = await screen.findByTestId("sandbox-chip");
    expect(chip.dataset.sandboxed).toBe("true");
    expect(chip.textContent).toBe("Sandboxed");
    emitLocal("sandbox:pane", {
      paneId: "pn_1",
      sandboxed: false,
      reason: "Install bubblewrap (the bwrap command) to sandbox agents.",
    });
    rerender(
      <UiProvider>
        <SandboxChip paneId="pn_1" />
      </UiProvider>,
    );
    expect((await screen.findByTestId("sandbox-chip")).dataset.sandboxed).toBe("false");
    stop();
  });

  it("F-TL-11.6 a pane with no note (the setting off) shows no chip", () => {
    render(
      <UiProvider>
        <SandboxChip paneId="pn_9" />
      </UiProvider>,
    );
    expect(screen.queryByTestId("sandbox-chip")).toBeNull();
  });

  it("F-TL-11.4 the setting says why it isn't available", async () => {
    fakeBackend({
      sandbox_status: () => ({ available: false, tool: null, reason: "Not available on this system yet." }),
      settings_get_all: () => ({}),
    });
    render(
      <UiProvider>
        <SandboxSetting ids={{ id: "agent_sandbox" }} />
      </UiProvider>,
    );
    const line = await screen.findByTestId("sandbox-status");
    expect(line.dataset.available).toBe("false");
    expect(line.textContent).toBe("Not available on this system yet.");
    expect(useSandbox.getState().statusState).toBe("ready");
  });
});
