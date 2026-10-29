import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { COVER_EVERY_MS, SETTLE_MS, useSteadyBox } from "./steadyBox";

/** A row like a held draft's: a box with one granting button among three. */
function Row({ n, onAllow }: { n: number; onAllow?: () => void }) {
  const box = useSteadyBox(true, `row-${n}`);
  return (
    <div ref={box.ref} data-row={n}>
      <button
        type="button"
        data-grant
        data-testid={`allow-${n}`}
        disabled={!box.steady}
        onClick={() => box.check() && onAllow?.()}
      >
        Allow
      </button>
      <button type="button" data-testid={`deny-${n}`}>
        Deny
      </button>
      <button type="button">Open</button>
    </div>
  );
}

/** Buttons side by side, 100 apart and 90 wide; each row 40 high. */
function layout() {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    const row = this.closest<HTMLElement>("[data-row]");
    const top = 40 * Number(row?.dataset.row ?? 0);
    const siblings = this.tagName === "BUTTON" ? Array.from(this.parentElement?.children ?? []) : [];
    const left = siblings.length > 0 ? 100 * siblings.indexOf(this) : 0;
    const width = siblings.length > 0 ? 90 : 300;
    return {
      left,
      top,
      width,
      height: 30,
      right: left + width,
      bottom: top + 30,
      x: left,
      y: top,
      toJSON: () => ({}),
    } as DOMRect;
  });
}

/** An IntersectionObserver that reports the rows below `inView` as in view. */
let inView = Infinity;
let observers: { cb: IntersectionObserverCallback; els: Element[] }[] = [];
class FakeIO {
  constructor(private cb: IntersectionObserverCallback) {
    observers.push({ cb, els: [] });
  }
  observe(el: Element) {
    observers.find((o) => o.cb === this.cb)?.els.push(el);
    const n = Number((el as HTMLElement).dataset.row ?? 0);
    queueMicrotask(() =>
      this.cb(
        [{ isIntersecting: n < inView, target: el } as IntersectionObserverEntry],
        this as unknown as IntersectionObserver,
      ),
    );
  }
  disconnect() {}
  unobserve() {}
  takeRecords() {
    return [];
  }
}
/** Tells every observer its elements' current state. */
function rescroll() {
  for (const o of observers)
    for (const el of o.els) {
      const n = Number((el as HTMLElement).dataset.row ?? 0);
      o.cb(
        [{ isIntersecting: n < inView, target: el } as IntersectionObserverEntry],
        {} as IntersectionObserver,
      );
    }
}

let hits = 0;
let covering: Element | null = null;
let coverX: [number, number] | null = null;
beforeEach(() => {
  hits = 0;
  covering = null;
  coverX = null;
  inView = Infinity;
  observers = [];
  layout();
  Object.defineProperty(document, "elementFromPoint", {
    value: (x: number) => {
      hits += 1;
      return covering !== null && (coverX === null || (x >= coverX[0] && x < coverX[1])) ? covering : null;
    },
    configurable: true,
  });
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  covering?.remove();
  Object.defineProperty(document, "elementFromPoint", { value: undefined, configurable: true });
});

const wait = (ms: number) => act(() => new Promise((r) => setTimeout(r, ms)));

describe("useSteadyBox (F-TL-12.12)", () => {
  it("N1: 500 rows, 10 in view: only those sample, only their granting button, about 10 times a second", async () => {
    vi.stubGlobal("IntersectionObserver", FakeIO);
    inView = 10;
    render(
      <div>
        {Array.from({ length: 500 }, (_, n) => (
          <Row key={n} n={n} />
        ))}
      </div>,
    );
    await waitFor(() => expect((screen.getByTestId("allow-0") as HTMLButtonElement).disabled).toBe(false));
    hits = 0;
    const rects = vi.mocked(HTMLElement.prototype.getBoundingClientRect);
    rects.mockClear();
    let frames = 0;
    const count = () => {
      frames += 1;
      id = requestAnimationFrame(count);
    };
    let id = requestAnimationFrame(count);
    await wait(1_000);
    cancelAnimationFrame(id);
    // Each in-view box: one granting button, 5 points, every COVER_EVERY_MS.
    const bound = 10 * 5 * (1_000 / COVER_EVERY_MS + 2);
    expect(hits).toBeGreaterThan(0);
    expect(hits).toBeLessThanOrEqual(bound);
    // Per frame, the boxes measured are the ones in view, not all 500.
    expect(rects.mock.calls.length / Math.max(frames, 1)).toBeLessThan(10 * 3);
    // Out of view, a row is off and stays off.
    expect((screen.getByTestId("allow-400") as HTMLButtonElement).disabled).toBe(true);
  });

  it("N1: a box scrolled into view starts its wait then", async () => {
    vi.stubGlobal("IntersectionObserver", FakeIO);
    inView = 1;
    render(
      <div>
        <Row n={0} />
        <Row n={1} />
      </div>,
    );
    await waitFor(() => expect((screen.getByTestId("allow-0") as HTMLButtonElement).disabled).toBe(false));
    expect((screen.getByTestId("allow-1") as HTMLButtonElement).disabled).toBe(true);
    inView = 2;
    act(() => rescroll());
    await wait(SETTLE_MS / 2);
    expect((screen.getByTestId("allow-1") as HTMLButtonElement).disabled).toBe(true);
    await waitFor(() => expect((screen.getByTestId("allow-1") as HTMLButtonElement).disabled).toBe(false));
  });

  it("N5: a cover over Deny alone doesn't hold Allow off; one over Allow's corner does", async () => {
    const onAllow = vi.fn();
    render(<Row n={0} onAllow={onAllow} />);
    const allow = () => screen.getByTestId("allow-0") as HTMLButtonElement;
    await waitFor(() => expect(allow().disabled).toBe(false));
    covering = document.createElement("div");
    document.body.append(covering);
    coverX = [100, 200];
    await wait(COVER_EVERY_MS * 3);
    expect(allow().disabled).toBe(false);
    fireEvent.click(allow());
    expect(onAllow).toHaveBeenCalledTimes(1);
    coverX = [80, 90];
    await waitFor(() => expect(allow().disabled).toBe(true));
    fireEvent.click(allow());
    expect(onAllow).toHaveBeenCalledTimes(1);
  });

  it("N1: between samples a click still looks for a cover on the spot", async () => {
    const onAllow = vi.fn();
    render(<Row n={0} onAllow={onAllow} />);
    const allow = () => screen.getByTestId("allow-0") as HTMLButtonElement;
    await waitFor(() => expect(allow().disabled).toBe(false));
    covering = document.createElement("div");
    document.body.append(covering);
    fireEvent.click(allow());
    expect(onAllow).not.toHaveBeenCalled();
  });

  it("nit: a cover that comes and goes between samples still counts, caught at the press", async () => {
    const onAllow = vi.fn();
    render(<Row n={0} onAllow={onAllow} />);
    const allow = () => screen.getByTestId("allow-0") as HTMLButtonElement;
    await waitFor(() => expect(allow().disabled).toBe(false));
    // An overlay up for 20 ms: the first press of a double-click lands on
    // it (and closes it), the second on Allow.
    covering = document.createElement("div");
    document.body.append(covering);
    fireEvent.pointerDown(covering);
    fireEvent.pointerUp(covering);
    covering.remove();
    covering = null;
    fireEvent.click(allow());
    expect(onAllow).not.toHaveBeenCalled();
  });

  it('nit: data-grant="false" is not a grant: a cover over it holds nothing off', async () => {
    function Marked({ onAllow }: { onAllow: () => void }) {
      const box = useSteadyBox(true, "m");
      return (
        <div ref={box.ref} data-row={0}>
          <button type="button" data-grant data-testid="allow" onClick={() => box.check() && onAllow()}>
            Allow
          </button>
          <button type="button" data-grant="false">
            Deny
          </button>
        </div>
      );
    }
    const onAllow = vi.fn();
    render(<Marked onAllow={onAllow} />);
    await wait(SETTLE_MS + 100);
    covering = document.createElement("div");
    document.body.append(covering);
    coverX = [100, 200];
    fireEvent.click(screen.getByTestId("allow"));
    expect(onAllow).toHaveBeenCalledTimes(1);
  });

  it("M1: a box taller than the view (a held draft with long args) works once its control is in view, and waits while it's past the edge", async () => {
    // The observer says the box is partly in view, never wholly.
    class PartIO {
      constructor(private cb: IntersectionObserverCallback) {}
      observe(el: Element) {
        queueMicrotask(() =>
          this.cb(
            [{ isIntersecting: true, intersectionRatio: 0.57, target: el } as IntersectionObserverEntry],
            this as unknown as IntersectionObserver,
          ),
        );
      }
      disconnect() {}
      unobserve() {}
      takeRecords() {
        return [];
      }
    }
    vi.stubGlobal("IntersectionObserver", PartIO);
    let grantTop = 100;
    vi.mocked(HTMLElement.prototype.getBoundingClientRect).mockImplementation(function (this: HTMLElement) {
      // The box runs from 400 px above the window to well below it.
      const r = this.tagName === "BUTTON" ? { left: 10, top: grantTop, width: 90, height: 30 } : { left: 0, top: -400, width: 300, height: 1_400 };
      return { ...r, right: r.left + r.width, bottom: r.top + r.height, x: r.left, y: r.top, toJSON: () => ({}) } as DOMRect;
    });
    const onAllow = vi.fn();
    render(<Row n={0} onAllow={onAllow} />);
    const allow = () => screen.getByTestId("allow-0") as HTMLButtonElement;
    await waitFor(() => expect(allow().disabled).toBe(false));
    fireEvent.click(allow());
    expect(onAllow).toHaveBeenCalledTimes(1);
    // Scrolled so Allow is past the window's bottom edge: it waits.
    grantTop = window.innerHeight + 20;
    await waitFor(() => expect(allow().disabled).toBe(true));
    fireEvent.click(allow());
    expect(onAllow).toHaveBeenCalledTimes(1);
    // Back in view: it waits its moment, then works again.
    grantTop = 200;
    await waitFor(() => expect(allow().disabled).toBe(false));
  });
});
