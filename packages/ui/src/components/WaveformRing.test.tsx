import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ThemeProvider } from "../theme/ThemeProvider";
import { insertText } from "./Composer";
import { WaveformRing } from "./WaveformRing";

describe("WaveformRing (UI_SPEC §2.30)", () => {
  it("F-DI-07 draws 48 bars, a gap at the top on error, and names its state", () => {
    const { rerender } = render(
      <ThemeProvider motion="off">
        <WaveformRing state="listening" level={0.5} label="Listening" />
      </ThemeProvider>,
    );
    const ring = screen.getByRole("img", { name: "Listening" });
    expect(ring.querySelectorAll("line").length).toBe(48);
    rerender(
      <ThemeProvider motion="off">
        <WaveformRing state="error" label="Your microphone isn't delivering audio" />
      </ThemeProvider>,
    );
    expect(screen.getByRole("img").querySelectorAll("line").length).toBeLessThan(48);
    expect(screen.getByRole("img").getAttribute("data-state")).toBe("error");
  });

  it("F-DI-07 reduced motion draws a static ring instead of moving bars", () => {
    render(
      <ThemeProvider motion="on">
        <WaveformRing state="thinking" label="Transcribing" />
      </ThemeProvider>,
    );
    const ring = screen.getByRole("img", { name: "Transcribing" });
    expect(ring.querySelectorAll("line").length).toBe(0);
    expect(ring.querySelector("circle")).toBeTruthy();
  });
});

describe("Composer insertion (F-DI-12)", () => {
  it("F-DI-12.1 inserts at the caret, spaced from its neighbours, over a selection", () => {
    expect(insertText("fix the test", 8, 8, "failing")).toEqual({ value: "fix the failing test", caret: 15 });
    expect(insertText("hello", 5, 5, "world")).toEqual({ value: "hello world", caret: 11 });
    expect(insertText("", 0, 0, "first")).toEqual({ value: "first", caret: 5 });
    expect(insertText("say X.", 4, 5, "hi")).toEqual({ value: "say hi.", caret: 6 });
  });
});
