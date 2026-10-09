import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PickedElement } from "@tandem/protocol";
// The script the core injects into pages, run here against a test page.
import script from "../../../src-tauri/src/browser/picker.js?raw";

type Report = { elements: PickedElement[]; stopped: boolean };

const NONCE = "0123456789abcdef0123456789abcdef";
let reports: string[] = [];
const iframeSrc = Object.getOwnPropertyDescriptor(HTMLIFrameElement.prototype, "src");

/** A report's selection, from its address. */
function decode(url: string, nonce = NONCE): Report {
  const prefix = `tandem-picker://${nonce}/`;
  expect(url.startsWith(prefix)).toBe(true);
  return JSON.parse(decodeURIComponent(url.slice(prefix.length))) as Report;
}

/** The page reports through a frame's address; read them here. */
function last(): Report {
  return decode(reports.at(-1) ?? "");
}

function click(el: Element, opts: MouseEventInit = {}): boolean {
  return el.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ...opts }));
}

const $ = (sel: string) => document.querySelector(sel) as HTMLElement;

beforeEach(() => {
  reports = [];
  Object.defineProperty(HTMLIFrameElement.prototype, "src", {
    configurable: true,
    set(value: string) {
      reports.push(value);
    },
    get() {
      return "";
    },
  });
  // A page with repeats, nesting and no ids where it matters, plus a
  // duplicated id (which can't name anything).
  document.body.innerHTML = `
    <header><nav><a href="/">Home</a><a href="/docs">Docs</a></nav></header>
    <main id="app">
      <ul>
        <li>One</li><li>Two <b>bold</b></li><li>Three</li>
      </ul>
      <section><div><span>x</span></div><div><span>y</span><span>z</span></div></section>
      <button id="dup">A</button><button id="dup">B</button>
      <p>${"long ".repeat(1200)}</p>
    </main>`;
  new Function(script.replace("__TANDEM_NONCE__", NONCE))();
});

afterEach(() => {
  document.dispatchEvent(new CustomEvent(`tandem-picker-stop:${NONCE}`));
  if (iframeSrc) Object.defineProperty(HTMLIFrameElement.prototype, "src", iframeSrc);
  document.body.innerHTML = "";
});

describe("element picker script (F-CD-18)", () => {
  it("F-CD-18.1 two picked elements get selectors that each match exactly that one element", () => {
    const second = $("main > ul > li:nth-of-type(2)");
    const z = document.querySelectorAll("section span")[2] as HTMLElement;
    click(second);
    click(z, { shiftKey: true });
    const { elements, stopped } = last();
    expect(stopped).toBe(false);
    expect(elements).toHaveLength(2);
    const [a, b] = elements;
    for (const [e, el] of [
      [a, second],
      [b, z],
    ] as const) {
      const all = document.querySelectorAll(e?.selector ?? "");
      expect(all, e?.selector).toHaveLength(1);
      expect(all[0]).toBe(el);
    }
    // Every element of the page gets a selector naming only itself.
    for (const el of Array.from(document.querySelectorAll("body *"))) {
      click(el);
      const sel = last().elements[0]?.selector ?? "";
      const all = document.querySelectorAll(sel);
      expect(all, sel).toHaveLength(1);
      expect(all[0]).toBe(el);
    }
  });

  it("F-CD-18 click picks, Shift+click adds, Alt+click takes the parent, and the page never sees the click", () => {
    let pageSaw = 0;
    document.addEventListener("click", () => pageSaw++);
    expect(click($("b"))).toBe(false);
    expect(last().elements.map((e) => e.tag)).toEqual(["b"]);
    click($("li"), { shiftKey: true });
    click($("li"), { shiftKey: true });
    expect(last().elements.map((e) => e.tag)).toEqual(["b", "li"]);
    click($("b"), { altKey: true });
    expect(last().elements).toHaveLength(1);
    expect(document.querySelector(last().elements[0]?.selector ?? "")).toBe($("b").parentElement);
    expect(pageSaw).toBe(0);
  });

  it("F-CD-18 a picked element carries its component, styles, place and outerHTML cut at 4 KB", () => {
    const save = $("#dup");
    function SaveButton() {
      return null;
    }
    Object.assign(save, { "__reactFiber$x1": { type: "button", return: { type: SaveButton, return: null } } });
    Object.assign($("li"), { __vueParentComponent: { type: { __name: "TodoItem" } } });
    click(save);
    const [e] = last().elements;
    expect(e?.component).toEqual({ name: "SaveButton", framework: "React" });
    expect(e?.html).toBe('<button id="dup">A</button>');
    expect(e?.truncated).toBe(false);
    expect(Object.keys(e?.styles ?? {})).toEqual(["display", "position", "font", "color", "background", "margin", "padding"]);
    expect(e?.rect).toMatchObject({ width: expect.any(Number), height: expect.any(Number) });
    // A duplicated id names nothing: the selector says where it is instead.
    expect(document.querySelectorAll(e?.selector ?? "")).toHaveLength(1);
    expect(e?.selector).not.toBe("#dup");
    click($("li"));
    expect(last().elements[0]?.component).toEqual({ name: "TodoItem", framework: "Vue" });
    click($("p"));
    const long = last().elements[0];
    expect(long?.html).toHaveLength(4096);
    expect(long?.truncated).toBe(true);
  });

  it("F-CD-18 reports go by window where the browser doesn't see frames, or once the page's CSP refuses the frame", () => {
    // The empty selection went at once, so a refusing page says so early.
    expect(reports).toHaveLength(1);
    expect(last()).toEqual({ elements: [], stopped: false });
    const opened: string[] = [];
    const open = vi.spyOn(window, "open").mockImplementation((url) => {
      opened.push(String(url));
      return null;
    });
    document.dispatchEvent(Object.assign(new Event("securitypolicyviolation"), { blockedURI: "tandem-picker" }));
    click($("b"));
    expect(reports).toHaveLength(1);
    expect(opened).toHaveLength(1);
    expect(decode(opened[0] ?? "").elements.map((e) => e.tag)).toEqual(["b"]);
    document.dispatchEvent(new CustomEvent(`tandem-picker-stop:${NONCE}`));

    // A session told to go by window (WebView2) sends nothing until a pick.
    const other = "fedcba9876543210fedcba9876543210";
    new Function(script.replace("__TANDEM_NONCE__", other).replace("__TANDEM_VIA__", "window"))();
    expect(opened).toHaveLength(1);
    click($("li"));
    expect(reports).toHaveLength(1);
    expect(decode(opened.at(-1) ?? "", other).elements.map((e) => e.tag)).toEqual(["li"]);
    document.dispatchEvent(new CustomEvent(`tandem-picker-stop:${other}`));
    open.mockRestore();
  });

  it("F-CD-18 Esc stops: the app hears it, and clicks go back to the page", () => {
    click($("li"));
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    expect(last().stopped).toBe(true);
    const before = reports.length;
    expect(click($("li"))).toBe(true);
    expect(reports).toHaveLength(before);
    // Its marks are gone too.
    expect(document.querySelector("tandem-picker-layer")).toBeNull();
  });
});
