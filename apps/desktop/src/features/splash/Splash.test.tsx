import { fireEvent, render, screen } from "@testing-library/react";
import { axe } from "vitest-axe";
import { describe, expect, it, vi } from "vitest";
import { Splash } from "./Splash";

describe("UI-02 splash", () => {
  it("renders labels from the i18n catalog while starting", async () => {
    const { container } = render(<Splash />);
    expect(screen.getByRole("heading", { name: "Tandem" })).toBeTruthy();
    expect(screen.getByRole("status").textContent).toBe("Starting…");
    expect(await axe(container)).toHaveNoViolations();
  });

  it("shows restore progress and skip", () => {
    const onSkip = vi.fn();
    render(<Splash progress={{ done: 6, total: 9 }} onSkip={onSkip} />);
    expect(screen.getByText("6 of 9 sessions")).toBeTruthy();
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("6");
    fireEvent.click(screen.getByRole("button", { name: "Skip restore" }));
    expect(onSkip).toHaveBeenCalled();
  });

  it("F-SH-07 says what launch is doing, and any key or click skips the wait", () => {
    const onAnyInput = vi.fn();
    const { rerender } = render(<Splash phase="restoring" onAnyInput={onAnyInput} />);
    expect(screen.getByRole("status").textContent).toBe("Restoring your workspace…");
    rerender(<Splash phase="plan" onAnyInput={onAnyInput} />);
    expect(screen.getByRole("status").textContent).toBe("Checking your plan…");
    fireEvent.keyDown(screen.getByTestId("splash"), { key: "a" });
    fireEvent.pointerDown(screen.getByTestId("splash"));
    expect(onAnyInput).toHaveBeenCalledTimes(2);
  });
});
