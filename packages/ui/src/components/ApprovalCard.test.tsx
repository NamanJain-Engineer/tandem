import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { UiProvider } from "../theme/ThemeProvider";
import { ApprovalCard, type ApprovalCardProps } from "./ApprovalCard";

function card(props: Partial<ApprovalCardProps> = {}) {
  const handlers = { onAllow: vi.fn(), onAllowThread: vi.fn(), onDeny: vi.fn() };
  render(
    <UiProvider theme="dark">
      <ApprovalCard kind="command" title="Claude Code wants to run a command" allowThread {...handlers} {...props} />
    </UiProvider>,
  );
  return handlers;
}

describe("ApprovalCard (F-TH-11, UI-23)", () => {
  it("F-TH-11 command actions: Allow once, Allow for this thread, Deny", () => {
    const h = card();
    fireEvent.click(screen.getByRole("button", { name: "Allow once" }));
    fireEvent.click(screen.getByRole("button", { name: "Allow for this thread" }));
    fireEvent.click(screen.getByRole("button", { name: "Deny" }));
    expect(h.onAllow).toHaveBeenCalledTimes(1);
    expect(h.onAllowThread).toHaveBeenCalledTimes(1);
    expect(h.onDeny).toHaveBeenCalledWith(null);
  });

  it("F-TH-11 keys: Mod+Enter allows, Mod+Shift+Enter allows for the thread, D denies, Esc focuses Deny", () => {
    const h = card();
    const el = screen.getByTestId("approval-card");
    fireEvent.keyDown(el, { key: "Enter", ctrlKey: true });
    fireEvent.keyDown(el, { key: "Enter", ctrlKey: true, shiftKey: true });
    fireEvent.keyDown(el, { key: "a" });
    fireEvent.keyDown(el, { key: "d" });
    expect(h.onAllow).toHaveBeenCalledTimes(2);
    expect(h.onAllowThread).toHaveBeenCalledTimes(1);
    expect(h.onDeny).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(el, { key: "Escape" });
    expect(document.activeElement).toBe(screen.getByTestId("approval-deny"));
  });

  it("F-TH-11 Deny and explain sends the reason; typing in it never triggers the keys", () => {
    const h = card();
    fireEvent.keyDown(screen.getByTestId("approval-card"), { key: "e" });
    const reason = screen.getByLabelText("Tell the agent why");
    fireEvent.change(reason, { target: { value: "  use the staging db  " } });
    fireEvent.keyDown(reason, { key: "d" });
    expect(h.onDeny).not.toHaveBeenCalled();
    fireEvent.keyDown(reason, { key: "Enter", metaKey: true });
    expect(h.onDeny).toHaveBeenCalledWith("use the staging db");
  });

  it("F-TH-11 no thread option when the request doesn't offer it, or for spend", () => {
    card({ allowThread: false });
    expect(screen.queryByRole("button", { name: "Allow for this thread" })).toBeNull();
  });

  it("F-TH-11 file edits hide the diff behind View diff; resolved collapses to one line", () => {
    const { unmount } = render(
      <UiProvider theme="dark">
        <ApprovalCard
          kind="file_write"
          title="Claude Code wants to edit 2 files"
          detail={<pre>diff</pre>}
          labels={{ allow: "Allow", showDetail: "View diff" }}
          onAllow={() => undefined}
          onDeny={() => undefined}
        />
      </UiProvider>,
    );
    expect(screen.queryByText("diff")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "View diff" }));
    expect(screen.getByText("diff")).toBeTruthy();
    unmount();
    card({ resolved: { tone: "denied", text: "Denied · you · 10:42" } });
    expect(screen.getByTestId("approval-resolved").textContent).toContain("Denied · you · 10:42");
    expect(screen.queryByTestId("approval-card")).toBeNull();
  });
});
