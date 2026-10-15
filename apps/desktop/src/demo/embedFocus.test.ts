import { afterEach, describe, expect, it, vi } from "vitest";
import { holdFocusUntilEngaged } from "./embedFocus";

// D-1821: on the home page the demo runs in a frame. Its composer autofocuses,
// which scrolled the page down to the frame and took the keyboard from it.
describe("the embedded demo's focus (D-1821)", () => {
  const original = HTMLElement.prototype.focus;
  afterEach(() => {
    HTMLElement.prototype.focus = original;
    document.body.innerHTML = "";
  });

  it("takes no focus until the visitor clicks or types in it, then never scrolls the page", () => {
    const calls: (FocusOptions | undefined)[] = [];
    HTMLElement.prototype.focus = function (this: HTMLElement, options?: FocusOptions) {
      calls.push(options);
      original.call(this, options);
    };
    const release = holdFocusUntilEngaged(window);
    const box = document.createElement("textarea");
    document.body.append(box);

    box.focus();
    expect(document.activeElement).not.toBe(box);
    expect(calls).toEqual([]);

    window.dispatchEvent(new Event("pointerdown"));
    box.focus();
    expect(document.activeElement).toBe(box);
    expect(calls).toEqual([{ preventScroll: true }]);

    box.blur();
    box.focus({ focusVisible: true } as FocusOptions);
    expect(calls[1]).toEqual({ focusVisible: true, preventScroll: true });
    release();
  });

  it("a key pressed in the frame engages it too, and release puts focus back as it was", () => {
    const before = vi.fn(original);
    HTMLElement.prototype.focus = before;
    const release = holdFocusUntilEngaged(window);
    expect(HTMLElement.prototype.focus).not.toBe(before);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "a" }));
    const box = document.createElement("input");
    document.body.append(box);
    box.focus();
    expect(document.activeElement).toBe(box);
    release();
    expect(HTMLElement.prototype.focus).toBe(before);
  });
});
