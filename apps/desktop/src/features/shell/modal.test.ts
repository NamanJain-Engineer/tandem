import { afterEach, describe, expect, it } from "vitest";
import { modalOpen } from "./modal";

describe("modalOpen (UI_SPEC §2.10)", () => {
  afterEach(() => document.body.replaceChildren());

  it("F-WT-01.5 a danger confirm (alertdialog) counts as an open dialog, as a dialog does (D-841)", () => {
    expect(modalOpen()).toBe(false);
    const el = document.createElement("div");
    el.setAttribute("role", "alertdialog");
    document.body.append(el);
    expect(modalOpen()).toBe(true);
    el.setAttribute("role", "dialog");
    expect(modalOpen()).toBe(true);
    el.setAttribute("role", "menu");
    expect(modalOpen()).toBe(false);
  });
});
