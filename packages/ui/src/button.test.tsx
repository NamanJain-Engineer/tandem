import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Button } from "./button";

describe("Button", () => {
  it("renders a primary button", () => {
    render(<Button>Save</Button>);
    const button = screen.getByRole("button", { name: "Save" });
    expect(button.getAttribute("data-variant")).toBe("primary");
    expect(button.hasAttribute("disabled")).toBe(false);
  });

  it("can be disabled", () => {
    render(<Button disabled variant="danger">Delete</Button>);
    const button = screen.getByRole("button", { name: "Delete" });
    expect(button.getAttribute("data-variant")).toBe("danger");
    expect((button as HTMLButtonElement).disabled).toBe(true);
  });
});
