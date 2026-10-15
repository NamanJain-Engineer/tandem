import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { UiProvider } from "@tandem/ui";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { EnginesPage } from "./EnginesPage";

const NOTE =
  "Google serves Gemini CLI to paid API keys and Gemini Code Assist Standard or Enterprise. Personal Google accounts use Antigravity CLI.";

const view = (id: string, label: string, logo: string, installed: boolean, note: string | null) => ({
  spec: { id, label, vendor: "Google", logo, commands: [id], newlineSequence: "\u001b\r", note },
  install: installed ? { path: `/bin/${id}`, version: "1.0.0", source: "shell" } : null,
  chatCapable: false,
  unsupportedReason: null,
  extraArgs: [],
});

describe("Settings › Engines (F-EN-09)", () => {
  beforeEach(() => resetStores());

  it("F-EN-09.2 Gemini CLI stays listed with its note; an engine without one shows none", async () => {
    fakeBackend({
      engines_list: () => [view("gemini", "Gemini CLI", "G", true, NOTE), view("antigravity", "Antigravity CLI", "Ag", false, null)],
      engines_report: () => null,
    });
    render(
      <UiProvider>
        <EnginesPage />
      </UiProvider>,
    );
    expect((await screen.findByTestId("engine-note-gemini")).textContent).toBe(NOTE);
    expect(screen.getByTestId("engine-row-antigravity")).toBeTruthy();
    expect(screen.queryByTestId("engine-note-antigravity")).toBeNull();
  });
});
