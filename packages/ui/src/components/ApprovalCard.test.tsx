import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { UiProvider } from "../theme/ThemeProvider";
import { ApprovalCard, type ApprovalCardProps } from "./ApprovalCard";

function card(props: Partial<ApprovalCardProps> = {}) {
  const handlers = { onAllow: vi.fn(), onAllowThread: vi.fn(), onDeny: vi.fn() };
  render(
    <UiProvider theme="dark">
      <ApprovalCard
        kind="command"
        title="Claude Code wants to run a command"
        allowThread
        {...handlers}
        {...props}
      />
    </UiProvider>,
  );
  return handlers;
}

/** Waits until Allow has held still long enough to take a click. */
async function steady() {
  await waitFor(() =>
    expect((screen.getByTestId("approval-allow") as HTMLButtonElement).disabled).toBe(false),
  );
}

describe("ApprovalCard (F-TH-11, UI-23)", () => {
  it("F-TH-11 command actions: Allow once, Allow for this thread, Deny", async () => {
    const h = card();
    await steady();
    fireEvent.click(screen.getByRole("button", { name: "Allow once" }));
    fireEvent.click(screen.getByRole("button", { name: "Allow for this thread" }));
    fireEvent.click(screen.getByRole("button", { name: "Deny" }));
    expect(h.onAllow).toHaveBeenCalledTimes(1);
    expect(h.onAllowThread).toHaveBeenCalledTimes(1);
    expect(h.onDeny).toHaveBeenCalledWith(null);
  });

  it("F-TH-11 keys: Mod+Enter allows, Mod+Shift+Enter allows for the thread, D denies, Esc focuses Deny", async () => {
    const h = card();
    await steady();
    const el = screen.getByTestId("approval-card");
    el.focus();
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

describe("ApprovalCard holds still before it allows (F-TL-12.12, S4)", () => {
  // jsdom lays nothing out: give the buttons a row, 100 apart and 90 wide,
  // and a cover that sits over part of it.
  let covering: Element | null = null;
  let coverX: [number, number] | null = null;
  let shift = 0;
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
  function layout() {
    covering = null;
    coverX = null;
    shift = 0;
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      if (this.tagName !== "BUTTON") return rect(0, shift, 400, 30);
      const row = Array.from(this.parentElement?.children ?? []);
      return rect(100 * row.indexOf(this), shift, 90, 30);
    });
    Object.defineProperty(document, "elementFromPoint", {
      value: (x: number) =>
        covering !== null && (coverX === null || (x >= coverX[0] && x < coverX[1])) ? covering : null,
      configurable: true,
    });
  }
  afterEach(() => {
    vi.restoreAllMocks();
    covering?.remove();
    Object.defineProperty(document, "elementFromPoint", { value: undefined, configurable: true });
  });
  const allow = () => screen.getByTestId("approval-allow") as HTMLButtonElement;
  const thread = () => screen.getByTestId("approval-allow-thread") as HTMLButtonElement;

  it("S4: a card that just appeared takes no click or key on Allow until it has been still a moment", async () => {
    layout();
    const h = card();
    const el = screen.getByTestId("approval-card");
    el.focus();
    expect(allow().disabled).toBe(true);
    expect(thread().disabled).toBe(true);
    expect(allow().title).toBe("Allow turns on once this has been still on screen for a moment.");
    fireEvent.click(allow());
    fireEvent.keyDown(el, { key: "a" });
    fireEvent.keyDown(el, { key: "Enter", ctrlKey: true });
    fireEvent.keyDown(el, { key: "Enter", ctrlKey: true, shiftKey: true });
    expect(h.onAllow).not.toHaveBeenCalled();
    expect(h.onAllowThread).not.toHaveBeenCalled();
    // Deny never waits.
    fireEvent.keyDown(el, { key: "d" });
    expect(h.onDeny).toHaveBeenCalledWith(null);
    await steady();
    fireEvent.keyDown(el, { key: "a" });
    expect(h.onAllow).toHaveBeenCalledTimes(1);
  });

  it("S4: a card that moves waits again, and a key the moment it moved does nothing", async () => {
    layout();
    const h = card();
    const el = screen.getByTestId("approval-card");
    el.focus();
    await steady();
    shift = 40;
    // The key measures on the spot: the move is seen before any frame.
    fireEvent.keyDown(el, { key: "Enter", ctrlKey: true });
    fireEvent.keyDown(el, { key: "Enter", ctrlKey: true, shiftKey: true });
    expect(h.onAllow).not.toHaveBeenCalled();
    expect(h.onAllowThread).not.toHaveBeenCalled();
    await waitFor(() => expect(allow().disabled).toBe(true));
    await steady();
    fireEvent.click(thread());
    expect(h.onAllowThread).toHaveBeenCalledTimes(1);
  });

  it("S4: a cover over Allow for this thread alone turns both Allows and their keys off", async () => {
    layout();
    const h = card();
    const el = screen.getByTestId("approval-card");
    el.focus();
    await steady();
    covering = document.createElement("div");
    document.body.append(covering);
    coverX = [160, 200];
    await waitFor(() => expect(thread().disabled).toBe(true));
    expect(allow().disabled).toBe(true);
    fireEvent.click(thread());
    fireEvent.keyDown(el, { key: "a" });
    expect(h.onAllow).not.toHaveBeenCalled();
    expect(h.onAllowThread).not.toHaveBeenCalled();
    act(() => covering?.remove());
    covering = null;
    await steady();
    expect(thread().disabled).toBe(false);
  });

  it("S4: keys answer only the card that has focus", async () => {
    layout();
    const h = card();
    await steady();
    const el = screen.getByTestId("approval-card");
    // Focus elsewhere: a key event that reaches the card anyway is ignored.
    (document.activeElement as HTMLElement | null)?.blur();
    fireEvent.keyDown(el, { key: "a" });
    expect(h.onAllow).not.toHaveBeenCalled();
    el.focus();
    fireEvent.keyDown(el, { key: "a" });
    expect(h.onAllow).toHaveBeenCalledTimes(1);
  });
});
