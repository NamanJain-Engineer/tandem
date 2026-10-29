import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { UiProvider } from "../theme/ThemeProvider";
import { Checkbox } from "./Choice";

describe("Checkbox waiting (UI_SPEC §2.5)", () => {
  it("S3: an unchecked waiting box looks waiting: a fainter box, not the enabled one, and it ignores clicks", () => {
    const onChange = vi.fn();
    render(
      <UiProvider theme="dark">
        <Checkbox
          label="Pick"
          checked={false}
          waiting
          waitingReason="Wait a moment"
          onCheckedChange={onChange}
        />
      </UiProvider>,
    );
    const box = screen.getByRole("checkbox", { name: "Pick" });
    const classes = box.className.split(/\s+/);
    expect(classes).toContain("border-border");
    expect(classes).not.toContain("border-border-strong");
    expect(classes).toContain("bg-surface-2");
    expect(classes).not.toContain("bg-surface-4");
    // The focus ring stays at full strength: the box itself isn't dimmed.
    expect(classes).not.toContain("opacity-40");
    expect(box.getAttribute("aria-disabled")).toBe("true");
    expect((box as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(box);
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByText("Pick").className).toContain("opacity-40");
  });

  it("S3: once it's on, it looks and works like any checkbox", () => {
    const onChange = vi.fn();
    render(
      <UiProvider theme="dark">
        <Checkbox label="Pick" checked={false} onCheckedChange={onChange} />
      </UiProvider>,
    );
    const box = screen.getByRole("checkbox", { name: "Pick" });
    expect(box.className.split(/\s+/)).toContain("border-border-strong");
    fireEvent.click(box);
    expect(onChange).toHaveBeenCalledWith(true);
  });
});
