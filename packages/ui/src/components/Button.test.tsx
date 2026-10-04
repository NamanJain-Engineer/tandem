import { fireEvent, render, screen } from "@testing-library/react";
import { axe } from "vitest-axe";
import { describe, expect, it, vi } from "vitest";
import { Button, type ButtonVariant } from "./Button";

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
      <Button disabled disabledReason="Included with Pro">
        Connect
      </Button>,
    );
    expect(screen.getByRole("button").getAttribute("title")).toBe("Included with Pro");
  });

  it("has no axe violations", async () => {
    const { container } = render(<Button variant="primary">Continue</Button>);
    expect(await axe(container)).toHaveNoViolations();
  });
});
