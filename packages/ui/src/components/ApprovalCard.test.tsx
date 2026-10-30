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
  /** The control measured last: what's at its points when nothing covers
   * it (the guard measures each one just before it hit-tests its points). */
  let measured: Element | null = null;
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
      // eslint-disable-next-line @typescript-eslint/no-this-alias -- the stub hands back the control measured last
      measured = this;
      if (this.tagName !== "BUTTON") return rect(0, shift, 400, 30);
      const row = Array.from(this.parentElement?.children ?? []);
      return rect(100 * row.indexOf(this), shift, 90, 30);
    });
    Object.defineProperty(document, "elementFromPoint", {
      value: (x: number) =>
        covering !== null && (coverX === null || (x >= coverX[0] && x < coverX[1])) ? covering : measured,
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

  describe("S2: a second press while one is held doesn't take its place", () => {
    const view = (requestKey: string, onAllow: () => void) => (
      <UiProvider theme="dark">
        <ApprovalCard kind="command" title="run" requestKey={requestKey} onAllow={onAllow} onDeny={() => undefined} />
      </UiProvider>
    );
    const wait = (ms: number) => act(() => new Promise((r) => setTimeout(r, ms)));

    it("mouse held on A, B replaces it, Esc tapped 500 ms after B settled, mouse released: nothing is allowed", async () => {
      layout();
      const a = vi.fn();
      const b = vi.fn();
      const { rerender } = render(view("apr_a", a));
      await steady();
      fireEvent.pointerDown(allow(), { pointerId: 1 });
      rerender(view("apr_b", b));
      await wait(1_050);
      // Esc to "cancel" when the card changed under the pointer.
      fireEvent.keyDown(document.body, { key: "Escape", code: "Escape" });
      fireEvent.keyUp(document.body, { key: "Escape", code: "Escape" });
      fireEvent.pointerUp(allow(), { pointerId: 1 });
      fireEvent.click(allow());
      expect(a).not.toHaveBeenCalled();
      expect(b).not.toHaveBeenCalled();
    });

    it("mouse held on A, B replaces it, another finger taps elsewhere, mouse released: nothing is allowed", async () => {
      layout();
      const a = vi.fn();
      const b = vi.fn();
      const { rerender } = render(view("apr_a", a));
      await steady();
      fireEvent.pointerDown(allow(), { pointerId: 1 });
      rerender(view("apr_b", b));
      await wait(1_050);
      fireEvent.pointerDown(document.body, { pointerId: 2, pointerType: "touch" });
      fireEvent.pointerUp(document.body, { pointerId: 2, pointerType: "touch" });
      // The tap's compatibility mouse events (WebView2 fires them).
      fireEvent.mouseDown(document.body, { button: 0 });
      fireEvent.mouseUp(document.body, { button: 0 });
      fireEvent.click(document.body);
      await wait(300);
      fireEvent.pointerUp(allow(), { pointerId: 1 });
      fireEvent.click(allow());
      expect(b).not.toHaveBeenCalled();
      // A fresh press on B, once it has held still, allows it.
      fireEvent.pointerDown(allow(), { pointerId: 1 });
      fireEvent.pointerUp(allow(), { pointerId: 1 });
      fireEvent.click(allow());
      expect(b).toHaveBeenCalledTimes(1);
    });
  });

  it("N1: a held A answers once: its repeats on the same card answer nothing more", async () => {
    layout();
    const h = card();
    await steady();
    const el = screen.getByTestId("approval-card");
    el.focus();
    fireEvent.keyDown(el, { key: "a", code: "KeyA" });
    expect(h.onAllow).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(el, { key: "a", code: "KeyA", repeat: true });
    fireEvent.keyDown(el, { key: "Enter", code: "Enter", ctrlKey: true, repeat: true });
    expect(h.onAllow).toHaveBeenCalledTimes(1);
    fireEvent.keyUp(el, { key: "a", code: "KeyA" });
  });

  describe("M1/N4: the press tracker", () => {
    const view = (requestKey: string, onAllow: () => void) => (
      <UiProvider theme="dark">
        <ApprovalCard kind="command" title="run" requestKey={requestKey} onAllow={onAllow} onDeny={() => undefined} />
      </UiProvider>
    );
    const wait = (ms: number) => act(() => new Promise((r) => setTimeout(r, ms)));

    it("M1: mouse held on A, B replaces it, another window takes focus, mouse released: nothing is allowed", async () => {
      layout();
      const a = vi.fn();
      const b = vi.fn();
      const { rerender } = render(view("apr_a", a));
      await steady();
      fireEvent.pointerDown(allow(), { pointerId: 1 });
      rerender(view("apr_b", b));
      fireEvent.blur(window);
      await wait(1_050);
      fireEvent.focus(window);
      fireEvent.pointerUp(allow(), { pointerId: 1 });
      fireEvent.click(allow());
      expect(a).not.toHaveBeenCalled();
      expect(b).not.toHaveBeenCalled();
    });

    it("M1: Space held on A, blur and refocus, Space released over B: nothing is allowed", async () => {
      layout();
      const a = vi.fn();
      const b = vi.fn();
      const { rerender } = render(view("apr_a", a));
      await steady();
      allow().focus();
      fireEvent.keyDown(allow(), { key: " ", code: "Space" });
      rerender(view("apr_b", b));
      fireEvent.blur(window);
      await wait(1_050);
      fireEvent.focus(window);
      // Keydowns after refocus are repeats: they start no press.
      fireEvent.keyDown(allow(), { key: " ", code: "Space", repeat: true });
      fireEvent.keyUp(allow(), { key: " ", code: "Space" });
      fireEvent.click(allow());
      expect(b).not.toHaveBeenCalled();
    });

    it("N4: Shift held since before the card changed doesn't hold back a fresh shift-click on it", async () => {
      layout();
      const b = vi.fn();
      const { rerender } = render(view("apr_a", () => undefined));
      await steady();
      // Shift goes down for a shift-click, then the card changes.
      fireEvent.keyDown(document.body, { key: "Shift", code: "ShiftLeft" });
      rerender(view("apr_b", b));
      await wait(600);
      fireEvent.pointerDown(allow(), { pointerId: 1, shiftKey: true });
      fireEvent.pointerUp(allow(), { pointerId: 1, shiftKey: true });
      fireEvent.click(allow(), { shiftKey: true });
      expect(b).toHaveBeenCalledTimes(1);
      fireEvent.keyUp(document.body, { key: "Shift", code: "ShiftLeft" });
    });

    it("N4: a click handled late is judged by when it happened, not when its handler ran", async () => {
      layout();
      const a = vi.fn();
      const b = vi.fn();
      const { rerender } = render(view("apr_a", a));
      await steady();
      fireEvent.pointerDown(allow(), { pointerId: 1 });
      rerender(view("apr_b", b));
      await wait(1_050);
      // Released now; the click happened 10 ms later, but its handler runs
      // 300 ms after that (slow listeners): it still belongs to the press.
      const released = performance.now();
      const up = new PointerEvent("pointerup", { bubbles: true, pointerId: 1 } as PointerEventInit);
      Object.defineProperty(up, "timeStamp", { value: released });
      allow().dispatchEvent(up);
      await wait(300);
      const click = new MouseEvent("click", { bubbles: true, cancelable: true });
      Object.defineProperty(click, "timeStamp", { value: released + 10 });
      act(() => {
        allow().dispatchEvent(click);
      });
      expect(b).not.toHaveBeenCalled();
    });
  });

  describe("SF1/SF4: lost releases and skewed clocks", () => {
    const view = (requestKey: string, onAllow: () => void) => (
      <UiProvider theme="dark">
        <ApprovalCard kind="command" title="run" requestKey={requestKey} onAllow={onAllow} onDeny={() => undefined} />
      </UiProvider>
    );
    const wait = (ms: number) => act(() => new Promise((r) => setTimeout(r, ms)));

    it("SF1: after a release the page never saw, the next click (a pointermove with the button newly down, no pointerdown) is judged by its own press", async () => {
      layout();
      const b = vi.fn();
      const { rerender } = render(view("apr_a", () => undefined));
      await steady();
      // Pressed on A; the window goes away and the release is never seen.
      fireEvent.pointerDown(allow(), { pointerId: 1, pointerType: "mouse" });
      rerender(view("apr_b", b));
      await wait(600);
      expect(waiting(allow())).toBe(false);
      // The engine still thinks the button is down: the next press arrives
      // as a pointermove with the button newly down and a mousedown (no
      // pointerdown), then its pointerup and click.
      fireEvent.pointerMove(allow(), { pointerId: 1, pointerType: "mouse", button: 0, buttons: 1 });
      fireEvent.mouseDown(allow(), { button: 0, buttons: 1 });
      fireEvent.pointerUp(allow(), { pointerId: 1, pointerType: "mouse" });
      fireEvent.mouseUp(allow(), { button: 0 });
      fireEvent.click(allow());
      expect(b).toHaveBeenCalledTimes(1);
    });

    it("SF4: an event clock running ahead of performance.now() can't make a press on A look like one on B", async () => {
      layout();
      const a = vi.fn();
      const b = vi.fn();
      const { rerender } = render(view("apr_a", a));
      await steady();
      // The engine stamps events 700 ms ahead (seen as +440 ms after a
      // clock slew; anything over the 500 ms wait would reopen D-3058).
      const skew = 700;
      const down = new PointerEvent("pointerdown", { bubbles: true, pointerId: 1, pointerType: "mouse" } as PointerEventInit);
      Object.defineProperty(down, "timeStamp", { value: performance.now() + skew });
      act(() => {
        allow().dispatchEvent(down);
      });
      rerender(view("apr_b", b));
      await wait(600);
      const up = new PointerEvent("pointerup", { bubbles: true, pointerId: 1, pointerType: "mouse" } as PointerEventInit);
      Object.defineProperty(up, "timeStamp", { value: performance.now() + skew });
      allow().dispatchEvent(up);
      const click = new MouseEvent("click", { bubbles: true, cancelable: true });
      Object.defineProperty(click, "timeStamp", { value: performance.now() + skew });
      act(() => {
        allow().dispatchEvent(click);
      });
      expect(a).not.toHaveBeenCalled();
      expect(b).not.toHaveBeenCalled();
    });
  });

  describe("round 16: what can restart the mouse's press", () => {
    const view = (requestKey: string, onAllow: () => void) => (
      <UiProvider theme="dark">
        <ApprovalCard kind="command" title="run" requestKey={requestKey} onAllow={onAllow} onDeny={() => undefined} />
      </UiProvider>
    );
    const wait = (ms: number) => act(() => new Promise((r) => setTimeout(r, ms)));

    it("M1: a bare synthetic mousedown while the mouse is held on A can't restart its press", async () => {
      layout();
      const b = vi.fn();
      const { rerender } = render(view("apr_a", () => undefined));
      await steady();
      fireEvent.pointerDown(allow(), { pointerId: 1, pointerType: "mouse" });
      rerender(view("apr_b", b));
      await wait(1_050);
      document.body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0 }));
      fireEvent.pointerUp(allow(), { pointerId: 1, pointerType: "mouse" });
      fireEvent.click(allow());
      expect(b).not.toHaveBeenCalled();
    });

    it("D-3117: a pointermove with an empty pointerType while the mouse is held on A can't restart its press", async () => {
      layout();
      const b = vi.fn();
      const { rerender } = render(view("apr_a", () => undefined));
      await steady();
      fireEvent.pointerDown(allow(), { pointerId: 1, pointerType: "mouse" });
      rerender(view("apr_b", b));
      await wait(1_050);
      // A synthetic event built without one has pointerType "".
      fireEvent.pointerMove(document.body, { pointerId: 1, pointerType: "", button: 0, buttons: 1 });
      fireEvent.pointerUp(allow(), { pointerId: 1, pointerType: "mouse" });
      fireEvent.click(allow());
      expect(b).not.toHaveBeenCalled();
    });

    it("SF1: a press stamped 1.1 s in the past (begun before B appeared) isn't taken as a new one", async () => {
      layout();
      const b = vi.fn();
      const { rerender } = render(view("apr_a", () => undefined));
      await steady();
      rerender(view("apr_b", b));
      await wait(600);
      const down = new PointerEvent("pointerdown", { bubbles: true, pointerId: 1, pointerType: "mouse" } as PointerEventInit);
      Object.defineProperty(down, "timeStamp", { value: performance.now() - 1_100 });
      allow().dispatchEvent(down);
      fireEvent.pointerUp(allow(), { pointerId: 1, pointerType: "mouse" });
      fireEvent.click(allow());
      expect(b).not.toHaveBeenCalled();
    });
  });
});
