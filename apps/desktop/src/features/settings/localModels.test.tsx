import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { LocalServer } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { LocalModelChip } from "../code/LocalModelChip";
import { LocalModels, modelLine } from "./LocalModels";

const OLLAMA: LocalServer = {
  kind: "ollama",
  port: 11434,
  added: false,
  running: true,
  startable: true,
  startReason: null,
  models: [{ id: "qwen3-coder:30b", size: 18_556_701_696, parameters: "30.5B" }],
};
const LLAMA: LocalServer = {
  kind: "llama_cpp",
  port: 8080,
  added: false,
  running: true,
  startable: false,
  startReason: "Codex can't use llama.cpp servers yet",
  models: [{ id: "gpt-oss-20b", size: null, parameters: null }],
};
const ADDED: LocalServer = {
  kind: "lm_studio",
  port: 1235,
  added: true,
  running: false,
  startable: true,
  startReason: null,
  models: [],
};

let calls: Call[];
function open(handlers: Record<string, (a?: Record<string, unknown>) => unknown>) {
  calls = fakeBackend({ engines_list: () => [], engines_report: () => null, ...handlers });
  render(
    <UiProvider>
      <LocalModels />
    </UiProvider>,
  );
}

describe("UI-164 Local models (F-EN-11)", () => {
  beforeEach(() => resetStores());

  it("F-EN-11.1 a running server lists its models with the size and parameters it reports", async () => {
    open({ local_models_list: () => [OLLAMA, ADDED] });
    const row = (await screen.findAllByTestId("local-server")).find((r) => r.dataset.port === "11434")!;
    expect(within(row).getByTestId("local-model").textContent).toContain("qwen3-coder:30b · 18.6 GB · 30.5B");
    expect(within(row).getByTestId("local-state").textContent).toBe("Running · 1 model");
    const added = screen.getAllByTestId("local-server").find((r) => r.dataset.port === "1235")!;
    expect(within(added).getByTestId("local-state").textContent).toBe("Not running");
    expect(modelLine({ id: "m", size: null, parameters: null })).toBe("m");
  });

  it("F-EN-11.2 Start is offered only where Codex can run, and says why not", async () => {
    open({ local_models_list: () => [LLAMA, OLLAMA] });
    const rows = await screen.findAllByTestId("local-server");
    const llama = rows.find((r) => r.dataset.port === "8080")!;
    const start = within(llama).getByTestId("local-start") as HTMLButtonElement;
    expect(start.disabled).toBe(true);
    expect(start.title).toBe("Codex can't use llama.cpp servers yet");
    // No Codex installed in this fake: the Ollama model says so too.
    const ollama = rows.find((r) => r.dataset.port === "11434")!;
    expect((within(ollama).getByTestId("local-start") as HTMLButtonElement).title).toBe(
      "Install Codex to use a local model.",
    );
  });

  it("F-EN-11.2 on Windows Start says local models run on Linux and macOS for now", async () => {
    const windows = {
      ...OLLAMA,
      startable: false,
      startReason: "Local models run on Linux and macOS for now",
    };
    open({ local_models_list: () => [windows] });
    const row = (await screen.findAllByTestId("local-server"))[0]!;
    const start = within(row).getByTestId("local-start") as HTMLButtonElement;
    expect(start.disabled).toBe(true);
    expect(start.title).toBe("Local models run on Linux and macOS for now");
  });

  it("F-EN-11.3 Add a port sends the host as typed; a refused host shows the core's reason", async () => {
    open({
      local_models_list: () => [],
      local_models_add_port: (a) => {
        if (a?.host !== "127.0.0.1" && a?.host !== "localhost")
          throw {
            code: "invalid",
            message: "Vibezeo only looks for model servers on this computer (127.0.0.1 or localhost)",
          };
        return null;
      },
    });
    expect((await screen.findByTestId("local-empty")).textContent).toBe(
      "No model servers are running on this computer. Start Ollama or LM Studio, then check again.",
    );
    fireEvent.click(screen.getByTestId("local-add-open"));
    fireEvent.change(await screen.findByTestId("local-host"), { target: { value: "192.168.1.5" } });
    fireEvent.change(screen.getByTestId("local-port"), { target: { value: "11500" } });
    await act(async () => fireEvent.click(screen.getByTestId("local-add-confirm")));
    expect((await screen.findByTestId("local-error")).textContent).toContain(
      "only looks for model servers on this computer",
    );
    fireEvent.change(screen.getByTestId("local-host"), { target: { value: "127.0.0.1" } });
    await act(async () => fireEvent.click(screen.getByTestId("local-add-confirm")));
    await waitFor(() => expect(screen.queryByTestId("local-add")).toBeNull());
    expect(calls.filter((c) => c.cmd === "local_models_add_port").map((c) => c.args)).toEqual([
      { host: "192.168.1.5", port: 11500, kind: "ollama" },
      { host: "127.0.0.1", port: 11500, kind: "ollama" },
    ]);
    // The list is asked again after the add.
    expect(calls.filter((c) => c.cmd === "local_models_list").length).toBeGreaterThanOrEqual(2);
  });

  it("F-EN-11.3 a port out of range can't be added", async () => {
    open({ local_models_list: () => [] });
    fireEvent.click(await screen.findByTestId("local-add-open"));
    fireEvent.change(await screen.findByTestId("local-port"), { target: { value: "70000" } });
    expect((screen.getByTestId("local-add-confirm") as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText("Ports run from 1 to 65535")).toBeTruthy();
  });

  it("F-EN-11.1 a failed check shows the error with Try again", async () => {
    let fail = true;
    open({
      local_models_list: () => {
        if (fail) throw { code: "internal", message: "Couldn't set up the local client" };
        return [OLLAMA];
      },
    });
    expect(await screen.findByText("Couldn't set up the local client")).toBeTruthy();
    fail = false;
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Try again" })));
    expect(await screen.findAllByTestId("local-server")).toHaveLength(1);
  });

  it('F-EN-11.2 a pane on a local model says "On this computer"', () => {
    render(
      <UiProvider>
        <LocalModelChip label="qwen3-coder:30b · Ollama" />
      </UiProvider>,
    );
    expect(screen.getByTestId("local-model-chip").textContent).toBe("On this computer");
  });
});
