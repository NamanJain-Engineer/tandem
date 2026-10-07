import { render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { TerminalColorsSync } from "../../App";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { useSettings } from "../../stores/settings";

beforeEach(resetStores);
afterEach(() => setInvokeBackend(null));

const lastColors = (calls: ReturnType<typeof fakeBackend>) =>
  calls.filter((c) => c.cmd === "term_set_theme_colors").at(-1)?.args?.colors;

describe("theme (F-SH-05)", () => {
  it("F-SH-05.2 the core answers OSC 10/11/12 with the current theme", async () => {
    const calls = fakeBackend();
    const { rerender } = render(
      <UiProvider theme="dark">
        <TerminalColorsSync />
      </UiProvider>,
    );
    await waitFor(() =>
      expect(lastColors(calls)).toEqual({ fg: "#dbe4ef", bg: "#0b111c", cursor: "#2dd4bf" }),
    );
    rerender(
      <UiProvider theme="light">
        <TerminalColorsSync />
      </UiProvider>,
    );
    await waitFor(() =>
      expect(lastColors(calls)).toEqual({ fg: "#1e293b", bg: "#ffffff", cursor: "#0d9488" }),
    );
  });

  it("F-SH-05 a fixed terminal theme ignores the app theme", async () => {
    const calls = fakeBackend();
    useSettings.setState({ values: { terminal_theme: "night" }, loaded: true });
    render(
      <UiProvider theme="light">
        <TerminalColorsSync />
      </UiProvider>,
    );
    await waitFor(() => expect(lastColors(calls)).toMatchObject({ bg: "#0b111c" }));
  });
});
