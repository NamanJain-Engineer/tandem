import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { UiProvider } from "../theme/ThemeProvider";
import { MARKDOWN_MAX } from "./markdown";
import { LARGE_PREVIEW, MarkdownView } from "./MarkdownView";

describe("MarkdownView (F-TH-12)", () => {
  it("F-TH-12 a message over the limit shows a plain-text preview, then the rest a chunk per frame", async () => {
    const big = "x".repeat(2 * 1024 * 1024);
    expect(big.length).toBeGreaterThan(MARKDOWN_MAX);
    render(
      <UiProvider theme="dark">
        <MarkdownView markdown={big} streaming />
      </UiProvider>,
    );
    const shown = () => document.querySelector('[data-testid="large-text"]')?.textContent?.length ?? 0;
    expect(shown()).toBe(LARGE_PREVIEW);
    const more = screen.getByRole("button", { name: "Show all (2.0 MB)" });
    fireEvent.click(more);
    // Each frame adds a chunk until it's all there.
    for (let i = 0; i < 200 && shown() < big.length; i++) {
      await act(() => new Promise<void>((r) => requestAnimationFrame(() => r())));
    }
    expect(shown()).toBe(big.length);
    expect(screen.queryByRole("button", { name: /Show all/ })).toBeNull();
  });

  it("F-TH-12 the preview of a large message ends at a line end, in the transcript's scroll", () => {
    const line = "let value = compute(input, options);\n";
    const big = line.repeat(Math.ceil((MARKDOWN_MAX + 1) / line.length));
    render(
      <UiProvider theme="dark">
        <MarkdownView markdown={big} />
      </UiProvider>,
    );
    const box = document.querySelector('[data-testid="large-text"]');
    const preview = box?.textContent ?? "";
    expect(preview.length).toBeLessThanOrEqual(LARGE_PREVIEW);
    expect(preview.length).toBeGreaterThan(LARGE_PREVIEW - line.length);
    expect(preview.endsWith("\n")).toBe(true);
    expect(big.startsWith(preview)).toBe(true);
    expect(box?.className).not.toMatch(/overflow-(auto|scroll)/);
  });
});
