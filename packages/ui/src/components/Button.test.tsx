import { fireEvent, render, screen } from "@testing-library/react";
import { axe } from "vitest-axe";
import { describe, expect, it, vi } from "vitest";
import { Button, buttonClass, type ButtonVariant } from "./Button";

const variants: ButtonVariant[] = ["primary", "secondary", "ghost", "danger", "link"];
const states = ["default", "disabled", "loading"] as const;

describe("Button (UI_SPEC §2.1)", () => {
  for (const variant of variants) {
    for (const state of states) {
      it(`renders ${variant} / ${state}`, () => {
        const { container } = render(
          <Button variant={variant} disabled={state === "disabled"} loading={state === "loading"} kbd="⌘↩">
            Save
          </Button>,
        );
        expect(container.innerHTML).toMatchSnapshot();
      });
    }
  }

  it("blocks clicks while loading and marks aria-busy", () => {
    const onClick = vi.fn();
    render(
      <Button loading onClick={onClick}>
        Save
      </Button>,
    );
    const button = screen.getByRole("button", { name: /save/i });
    expect(button.getAttribute("aria-busy")).toBe("true");
    fireEvent.click(button);
    expect(onClick).not.toHaveBeenCalled();
  });

  it("explains why it is disabled", () => {
    render(
      <Button disabled disabledReason="Comes with Vibezeo accounts">
        Connect
      </Button>,
    );
    expect(screen.getByRole("button").getAttribute("title")).toBe("Comes with Vibezeo accounts");
  });

  it("has no axe violations", async () => {
    const { container } = render(<Button variant="primary">Continue</Button>);
    expect(await axe(container)).toHaveNoViolations();
  });

  it("D-2701 a mark is a rail row's: 28 px tall with 4 px sides, and with no words it's its icon alone", () => {
    expect(buttonClass("ghost", "mark").split(" ")).toEqual(expect.arrayContaining(["h-7", "px-1", "gap-1"]));
    render(<Button variant="ghost" size="mark" aria-label="Open" iconLeft={<svg aria-hidden="true" />} />);
    const button = screen.getByRole("button", { name: "Open" });
    expect(button.textContent).toBe("");
    expect([...button.children].map((c) => c.tagName.toLowerCase())).toEqual(["svg"]);
  });
});

describe("buttonClass (the public site's link-buttons)", () => {
  it("F-WB-01 gives a link the same classes as the button it imitates", () => {
    const { container } = render(<Button variant="primary" size="lg">Go</Button>);
    const button = container.querySelector("button")?.className ?? "";
    expect(button.startsWith(buttonClass("primary", "lg"))).toBe(true);
  });

  it("F-WB-01 xl is 44 px tall and outline is transparent with a strong border", () => {
    expect(buttonClass("primary", "xl")).toContain("h-11");
    expect(buttonClass("outline", "xl")).toContain("border-border-strong");
    expect(buttonClass("outline", "xl")).toContain("bg-transparent");
  });
});
