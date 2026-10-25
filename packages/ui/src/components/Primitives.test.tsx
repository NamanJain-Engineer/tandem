// D-2600/D-2601 (M18 · Developer-first UI): the quiet status primitives.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Badge, Count } from "./Badge";
import { StatusDot, type Status } from "./StatusDot";
import { AGO_LABELS, TimeAgo, compactAgo } from "./TimeAgo";

const MIN = 60_000;

describe("D-2601 StatusDot", () => {
  it("pairs every status with its own shape and an accessible name", () => {
    const statuses: Status[] = ["working", "waiting", "done", "idle", "error", "offline"];
    const shapes = statuses.map((s) => {
      const { container, unmount } = render(<StatusDot status={s} />);
      const svg = container.querySelector("svg") as SVGSVGElement;
      const shape = svg.innerHTML;
      unmount();
      return shape;
    });
    expect(new Set(shapes).size).toBe(statuses.length);
    render(<StatusDot status="waiting" />);
    expect(screen.getByRole("img", { name: "Waiting for you" })).toBeTruthy();
  });

  it("working is a filled dot with a breathing halo", () => {
    const { container } = render(<StatusDot status="working" />);
    expect(container.querySelectorAll("circle")).toHaveLength(2);
    expect(container.querySelector(".tn-breathe")).not.toBeNull();
  });
});

describe("D-2601 quiet Badge and Count", () => {
  it("a quiet badge is sentence case without a box", () => {
    render(
      <Badge variant="waiting" quiet dot>
        Needs you
      </Badge>,
    );
    const b = screen.getByText("Needs you");
    expect(b.className).not.toMatch(/uppercase|border/);
    expect(b.className).toMatch(/text-waiting-text/);
  });

  it("T3 a boxed badge is sentence case too, on the 4px scale", () => {
    render(<Badge variant="accent">Beta</Badge>);
    const b = screen.getByText("Beta");
    expect(b.className).not.toMatch(/uppercase|tracking-caps|\[18px\]/);
    expect(b.className).toMatch(/h-4\.5/);
  });

  it("a count caps at 99+ and takes an accessible name", () => {
    render(<Count value={140} label="140 unread" />);
    expect(screen.getByRole("img", { name: "140 unread" }).textContent).toBe("99+");
  });
});

describe("D-2601 relative time", () => {
  const now = Date.UTC(2026, 9, 20, 12);
  it.each([
    [10_000, "now"],
    [4 * MIN, "4m"],
    [59 * MIN, "59m"],
    [2 * 60 * MIN, "2h"],
    [26 * 60 * MIN, "1d"],
    [-5 * MIN, "now"],
  ])("%d ms ago reads %s", (ago, text) => {
    expect(compactAgo(now - ago, now, AGO_LABELS)).toBe(text);
  });

  it("TimeAgo carries a machine-readable date", () => {
    render(<TimeAgo at={now - 3 * 60 * MIN} now={now} />);
    const t = screen.getByText("3h");
    expect(t.tagName).toBe("TIME");
    expect(t.getAttribute("datetime")).toBe(new Date(now - 3 * 60 * MIN).toISOString());
  });
});
