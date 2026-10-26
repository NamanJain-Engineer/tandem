import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Coachmark, placeCoachmark } from "./Coachmark";

const rect = (left: number, top: number, width: number, height: number) =>
  ({
    left,
    top,
    width,
    height,
    right: left + width,
    bottom: top + height,
    x: left,
    y: top,
    toJSON: () => ({}),
  }) as DOMRect;

describe("Coachmark (DESIGN §5.2.2)", () => {
  it("DESIGN-5.2.2 sits beside its anchor and inside the window", () => {
    // A rail row: to its right, centred on it, never over the rail.
    expect(placeCoachmark(rect(0, 100, 240, 32), 200, 80, "right", 1280, 800)).toEqual({
      left: 248,
      top: 76,
    });
    // Under the mode switch, centred.
    expect(placeCoachmark(rect(600, 4, 80, 24), 200, 80, "bottom", 1280, 800)).toEqual({
      left: 540,
      top: 36,
    });
    // From its left edge, clear of what's to the left.
    expect(placeCoachmark(rect(280, 46, 60, 28), 200, 80, "bottom", 1280, 800, "start")).toEqual({
      left: 280,
      top: 82,
    });
    // Above the composer, right-aligned.
    expect(placeCoachmark(rect(300, 700, 600, 80), 200, 80, "top", 1280, 800)).toEqual({
      left: 700,
      top: 612,
    });
    // Near an edge it stays 8 px inside the window.
    expect(placeCoachmark(rect(1200, 780, 60, 20), 200, 80, "right", 1280, 800)).toEqual({
      left: 1072,
      top: 712,
    });
  });

  it("DESIGN-5.2.2 shows only with an anchor, announces itself, and never takes focus", () => {
    const { rerender } = render(
      <Coachmark anchor={null} label="Tip">
        Press Enter
      </Coachmark>,
    );
    expect(screen.queryByRole("status", { name: "Tip" })).toBeNull();
    const anchor = document.createElement("button");
    document.body.append(anchor);
    anchor.focus();
    rerender(
      <Coachmark anchor={anchor} label="Tip" side="right" footer={<span>1 of 3</span>}>
        Press Enter
      </Coachmark>,
    );
    const hint = screen.getByRole("status", { name: "Tip" });
    expect(hint.textContent).toBe("Press Enter1 of 3");
    expect(hint.dataset.side).toBe("right");
    expect(document.activeElement).toBe(anchor);
    anchor.remove();
  });
});
