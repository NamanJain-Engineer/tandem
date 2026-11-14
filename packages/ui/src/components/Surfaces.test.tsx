// DESIGN §4.2 (the active state of a control), §4.3 (sentence case) and
// §5.4.7 (keyboard first, always discoverable): the daily-use pass of
// 13 November (D-2637 to D-2644).
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { CommandPalette } from "./CommandPalette";
import { Menu } from "./Menu";
import { SegmentedControl } from "./SegmentedControl";
import { TooltipProvider } from "./Tooltip";

describe("DESIGN §4.2 the current mode shows", () => {
  it("DESIGN-4.2 a segment inside a tooltip still draws as selected (D-2638)", () => {
    render(
      <TooltipProvider>
        <SegmentedControl
          label="Work mode"
          value="code"
          modeSwitch
          items={[
            { id: "agent", label: "Agent", tooltip: "Agent mode" },
            { id: "code", label: "Code", tooltip: "Code mode" },
          ]}
        />
      </TooltipProvider>,
    );
    const code = screen.getByRole("radio", { name: "Code" });
    expect(code.getAttribute("aria-checked")).toBe("true");
    // The tooltip owns data-state ("closed"), so the selection can't hang on it.
    expect(code.getAttribute("data-state")).not.toBe("checked");
    expect(code.className).toContain("aria-checked:bg-surface-3");
    expect(code.className).not.toContain("data-[state=checked]");
    expect(code.querySelector("span[aria-hidden]")?.className).toContain("group-aria-checked:block");
  });
});

describe("DESIGN §4.3 menu labels", () => {
  it("DESIGN-4.3 a menu's label is sentence case, never caps, and cut to the menu (D-2640)", () => {
    render(
      <Menu
        open
        trigger={<button type="button">More</button>}
        entries={[
          { type: "label", id: "l", label: "Stack coupons by priority" },
          { id: "r", label: "Rename" },
        ]}
      />,
    );
    const label = screen.getByText("Stack coupons by priority");
    expect(label.className).not.toContain("uppercase");
    expect(label.className).toContain("truncate");
  });
});

describe("DESIGN §5.4.7 the palette says its keys", () => {
  it("DESIGN-5.4.7 F-SH-09 the palette's footer shows each key it answers to as a Kbd (D-2642)", () => {
    render(
      <CommandPalette
        open
        onOpenChange={() => undefined}
        groups={[
          {
            heading: "Go to",
            items: [{ id: "a", label: "Code mode", kbd: ["Mod", "2"], onSelect: () => undefined }],
          },
        ]}
        hints={[
          { keys: "↑↓", label: "Move" },
          { keys: "↵", label: "Run" },
          { keys: "Esc", label: "Close" },
        ]}
      />,
    );
    const footer = screen.getByTestId("palette-hints");
    expect(footer.querySelectorAll("kbd")).toHaveLength(3);
    expect(footer.textContent).toBe("↑↓Move↵RunEscClose");
    // Every command with a shortcut shows it.
    const row = screen.getByRole("option", { name: /Code mode/ });
    expect(row.querySelectorAll("kbd").length).toBeGreaterThan(0);
  });

  it("DESIGN-5.4.7 without hints there's no footer", () => {
    render(<CommandPalette open onOpenChange={() => undefined} groups={[]} />);
    expect(screen.queryByTestId("palette-hints")).toBeNull();
  });
});
