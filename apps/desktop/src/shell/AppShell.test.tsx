import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AppShell } from "./AppShell";

describe("AppShell", () => {
  it("switches mode from the rail and from the command palette", () => {
    render(<AppShell />);
    expect(screen.getByRole("heading", { level: 1, name: "Home" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Code" }));
    expect(screen.getByRole("heading", { level: 1, name: "Code" })).toBeTruthy();

    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    const filter = screen.getByRole("textbox", { name: "Filter commands" });
    fireEvent.change(filter, { target: { value: "thread" } });
    fireEvent.click(screen.getByRole("option", { name: "Open Threads" }));
    expect(screen.getByRole("heading", { level: 1, name: "Threads" })).toBeTruthy();
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
