import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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

/** A key pressed and released, as a person does (keydown, then keyup). */
function key(el: Element, init: { key: string; ctrlKey?: boolean; metaKey?: boolean; shiftKey?: boolean }) {
  fireEvent.keyDown(el, init);
  fireEvent.keyUp(el, init);
}

/** A control that waits is still focusable: it says so with aria-disabled. */
const waiting = (el: Element) => el.getAttribute("aria-disabled") === "true";

/** Waits until Allow has held still long enough to take a click. */
async function steady() {
  await waitFor(() =>
    expect(waiting((screen.getByTestId("approval-allow") as HTMLButtonElement))).toBe(false),
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
    key(el, { key: "Enter", ctrlKey: true });
    key(el, { key: "Enter", ctrlKey: true, shiftKey: true });
    key(el, { key: "a" });
    key(el, { key: "d" });
    expect(h.onAllow).toHaveBeenCalledTimes(2);
    expect(h.onAllowThread).toHaveBeenCalledTimes(1);
    expect(h.onDeny).toHaveBeenCalledTimes(1);
    key(el, { key: "Escape" });
    expect(document.activeElement).toBe(screen.getByTestId("approval-deny"));
  });

  it("F-TH-11 Deny and explain sends the reason; typing in it never triggers the keys", () => {
    const h = card();
    key(screen.getByTestId("approval-card"), { key: "e" });
    const reason = screen.getByLabelText("Tell the agent why");
    fireEvent.change(reason, { target: { value: "  use the staging db  " } });
    key(reason, { key: "d" });
    expect(h.onDeny).not.toHaveBeenCalled();
    key(reason, { key: "Enter", metaKey: true });
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
    expect(waiting(allow())).toBe(true);
    expect(waiting(thread())).toBe(true);
    expect(allow().title).toBe("Allow turns on once this has been still on screen for a moment.");
    fireEvent.click(allow());
    key(el, { key: "a" });
    key(el, { key: "Enter", ctrlKey: true });
    key(el, { key: "Enter", ctrlKey: true, shiftKey: true });
    expect(h.onAllow).not.toHaveBeenCalled();
    expect(h.onAllowThread).not.toHaveBeenCalled();
    // Deny never waits.
    key(el, { key: "d" });
    expect(h.onDeny).toHaveBeenCalledWith(null);
    await steady();
    key(el, { key: "a" });
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
    key(el, { key: "Enter", ctrlKey: true });
    key(el, { key: "Enter", ctrlKey: true, shiftKey: true });
    expect(h.onAllow).not.toHaveBeenCalled();
    expect(h.onAllowThread).not.toHaveBeenCalled();
    await waitFor(() => expect(waiting(allow())).toBe(true));
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
    await waitFor(() => expect(waiting(thread())).toBe(true));
    expect(waiting(allow())).toBe(true);
    fireEvent.click(thread());
    key(el, { key: "a" });
    expect(h.onAllow).not.toHaveBeenCalled();
    expect(h.onAllowThread).not.toHaveBeenCalled();
    act(() => covering?.remove());
    covering = null;
    await steady();
    expect(waiting(thread())).toBe(false);
  });

  it("S4: keys answer only the card that has focus", async () => {
    layout();
    const h = card();
    await steady();
    const el = screen.getByTestId("approval-card");
    // Focus elsewhere: a key event that reaches the card anyway is ignored.
    (document.activeElement as HTMLElement | null)?.blur();
    key(el, { key: "a" });
    expect(h.onAllow).not.toHaveBeenCalled();
    el.focus();
    key(el, { key: "a" });
    expect(h.onAllow).toHaveBeenCalledTimes(1);
  });

  it("N2: the same card given another request waits again, and a click meant for the first doesn't answer the second", async () => {
    layout();
    const a = vi.fn();
    const b = vi.fn();
    const view = (requestKey: string, onAllow: () => void) => (
      <UiProvider theme="dark">
        <ApprovalCard
          kind="command"
          title="run"
          requestKey={requestKey}
          onAllow={onAllow}
          onDeny={() => undefined}
        />
      </UiProvider>
    );
    const { rerender } = render(view("apr_a", a));
    await steady();
    rerender(view("apr_b", b));
    expect(waiting(allow())).toBe(true);
    fireEvent.click(allow());
    key(screen.getByTestId("approval-card"), { key: "a" });
    expect(a).not.toHaveBeenCalled();
    expect(b).not.toHaveBeenCalled();
    await steady();
    fireEvent.click(allow());
    expect(b).toHaveBeenCalledTimes(1);
  });

  it("S4: Allow and Allow for this thread are marked as controls that grant", () => {
    card();
    expect(allow().hasAttribute("data-grant")).toBe(true);
    expect(thread().hasAttribute("data-grant")).toBe(true);
    expect(screen.getByTestId("approval-deny").hasAttribute("data-grant")).toBe(false);
  });

  it("S3: a waiting Allow stays reachable by Tab, and Enter, Space or a click on it does nothing", async () => {
    layout();
    const user = userEvent.setup();
    const h = card();
    expect(waiting(allow())).toBe(true);
    // Tab from the card reaches Allow, waiting or not.
    screen.getByTestId("approval-card").focus();
    await user.tab();
    expect(document.activeElement).toBe(allow());
    await user.keyboard("{Enter}");
    await user.keyboard(" ");
    await user.click(allow());
    expect(h.onAllow).not.toHaveBeenCalled();
    await steady();
    await user.keyboard("{Enter}");
    expect(h.onAllow).toHaveBeenCalledTimes(1);
  });

  describe("S2: scrolled out of view", () => {
    let intersecting = true;
    let callbacks: IntersectionObserverCallback[] = [];
    class FakeIO {
      constructor(private cb: IntersectionObserverCallback) {
        callbacks.push(cb);
      }
      observe(el: Element) {
        queueMicrotask(() =>
          this.cb([{ isIntersecting: intersecting, target: el } as IntersectionObserverEntry], this as never),
        );
      }
      disconnect() {}
      unobserve() {}
      takeRecords() {
        return [];
      }
    }
    afterEach(() => {
      vi.unstubAllGlobals();
      intersecting = true;
      callbacks = [];
    });

    it("a focused card that has scrolled away takes no key, pressed once or twice", async () => {
      vi.stubGlobal("IntersectionObserver", FakeIO);
      layout();
      const h = card();
      const el = screen.getByTestId("approval-card");
      el.focus();
      await steady();
      // The person scrolls on and types "a…a" into what they think is the
      // composer; focus is still on the card.
      intersecting = false;
      act(() => callbacks.forEach((cb) => cb([{ isIntersecting: false } as IntersectionObserverEntry], {} as never)));
      key(el, { key: "a" });
      await act(() => new Promise((r) => setTimeout(r, 600)));
      key(el, { key: "a" });
      key(el, { key: "Enter", ctrlKey: true });
      expect(h.onAllow).not.toHaveBeenCalled();
    });
  });

  describe("S1: a press that began on one request never grants the one that replaced it", () => {
    const view = (requestKey: string, onAllow: () => void) => (
      <UiProvider theme="dark">
        <ApprovalCard kind="command" title="run" requestKey={requestKey} onAllow={onAllow} onDeny={() => undefined} />
      </UiProvider>
    );
    const wait = (ms: number) => act(() => new Promise((r) => setTimeout(r, ms)));

    it("pressed on A, released 600 ms after B took its place: nothing is allowed", async () => {
      layout();
      const a = vi.fn();
      const b = vi.fn();
      const { rerender } = render(view("apr_a", a));
      await steady();
      fireEvent.pointerDown(allow());
      // A is withdrawn and B takes its place in the same card.
      rerender(view("apr_b", b));
      await wait(600);
      expect(waiting(allow())).toBe(false);
      fireEvent.pointerUp(allow());
      fireEvent.click(allow());
      expect(a).not.toHaveBeenCalled();
      expect(b).not.toHaveBeenCalled();
      // A press that begins on B, once B has held still, allows B.
      fireEvent.pointerDown(allow());
      fireEvent.pointerUp(allow());
      fireEvent.click(allow());
      expect(b).toHaveBeenCalledTimes(1);
    });

    it("keyboard: Space pressed on A and released over B, or Enter repeating across the change, allows nothing", async () => {
      layout();
      const a = vi.fn();
      const b = vi.fn();
      const { rerender } = render(view("apr_a", a));
      await steady();
      allow().focus();
      fireEvent.keyDown(allow(), { key: " " });
      rerender(view("apr_b", b));
      await wait(600);
      // Space activates on release.
      fireEvent.keyUp(allow(), { key: " " });
      fireEvent.click(allow());
      expect(a).not.toHaveBeenCalled();
      expect(b).not.toHaveBeenCalled();
      // A key held on C: its first press answers C (that's what was
      // pressed), its repeats once D has taken C's place answer nothing.
      const c = vi.fn();
      const d = vi.fn();
      rerender(view("apr_c", c));
      await steady();
      const card = screen.getByTestId("approval-card");
      fireEvent.keyDown(card, { key: "a" });
      expect(c).toHaveBeenCalledTimes(1);
      rerender(view("apr_d", d));
      await wait(600);
      expect(waiting(allow())).toBe(false);
      fireEvent.keyDown(card, { key: "a", repeat: true });
      fireEvent.keyDown(allow(), { key: "Enter", repeat: true });
      fireEvent.click(allow());
      expect(d).not.toHaveBeenCalled();
      expect(c).toHaveBeenCalledTimes(1);
      fireEvent.keyUp(card, { key: "a" });
    });
  });
});
