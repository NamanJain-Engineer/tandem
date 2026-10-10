import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Heatmap, heatLevel } from "./Heatmap";

describe("Heatmap (F-AC-07)", () => {
  it("draws one cell per day, Monday-aligned, with levels by the busiest day", () => {
    // 2026-10-01 is a Thursday: three blank cells first.
    const days = ["2026-10-01", "2026-10-02", "2026-10-03", "2026-10-04"].map((date, i) => ({ date, value: [0, 10, 40, 100][i] ?? 0 }));
    render(<Heatmap days={days} label="Daily tokens" describe={(d) => `${d.date}: ${d.value}`} />);
    const map = screen.getByRole("img", { name: "Daily tokens" });
    const cells = map.querySelectorAll("[data-date]");
    expect(cells).toHaveLength(4);
    expect(map.children).toHaveLength(7);
    expect([...cells].map((c) => (c as HTMLElement).dataset.level)).toEqual(["0", "1", "2", "4"]);
    expect(cells[3]?.getAttribute("title")).toBe("2026-10-04: 100");
  });

  it("levels are quartiles of the maximum", () => {
    expect(heatLevel(0, 100)).toBe(0);
    expect(heatLevel(25, 100)).toBe(1);
    expect(heatLevel(26, 100)).toBe(2);
    expect(heatLevel(76, 100)).toBe(4);
    expect(heatLevel(5, 0)).toBe(0);
  });
});
