// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PhoneApproval } from "@tandem/remote/messages";
import { ApprovalRow } from "./RemoteApp";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const approval: PhoneApproval = {
  id: "ap_1",
  summary: "Run `pnpm deploy`",
  detail: "pnpm deploy",
  source: "Pane · auth tests",
  queued: false,
  deskOnly: false,
  createdAt: 1,
};

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const show = (a: PhoneApproval, decide: (id: string, d: "allow" | "deny") => Promise<boolean>) =>
  act(() => root.render(<ApprovalRow a={a} canAnswer decide={decide} />));
const allow = () => host.querySelector<HTMLButtonElement>('[data-testid="remote-approval-allow"]')!;
const deny = () => host.querySelector<HTMLButtonElement>('[data-testid="remote-approval-deny"]')!;
const waiting = (el: Element) => el.getAttribute("aria-disabled") === "true";
const pause = (ms: number) => act(() => new Promise((r) => setTimeout(r, ms)));

describe("The phone's approvals (F-TL-12.21)", () => {
  it("Allow waits until its row has held still, a tap before that allows nothing, and Deny never waits", async () => {
    const decide = vi.fn(async () => false);
    show(approval, decide);
    expect(allow().hasAttribute("data-grant")).toBe(true);
    expect(waiting(allow())).toBe(true);
    expect(allow().getAttribute("title")).toBe(
      "Allow turns on once this has been still on screen for a moment.",
    );
    act(() => allow().click());
    expect(decide).not.toHaveBeenCalled();
    await pause(700);
    expect(waiting(allow())).toBe(false);
    act(() => allow().click());
    await pause(0);
    expect(decide).toHaveBeenCalledWith("ap_1", "allow");
    // Another request in its place waits again.
    show({ ...approval, id: "ap_2", summary: "Run `rm -rf build`" }, decide);
    expect(waiting(allow())).toBe(true);
    act(() => deny().click());
    await pause(0);
    expect(decide).toHaveBeenLastCalledWith("ap_2", "deny");
  });

  it("an answer goes once: both buttons are off while it's on its way, and back if the computer didn't take it", async () => {
    let finish: () => void = () => undefined;
    const decide = vi.fn(() => new Promise<boolean>((r) => (finish = () => r(false))));
    show(approval, decide);
    act(() => deny().click());
    act(() => deny().click());
    expect(decide).toHaveBeenCalledTimes(1);
    expect(deny().disabled).toBe(true);
    expect(allow().disabled).toBe(true);
    await act(async () => finish());
    expect(deny().disabled).toBe(false);
  });

  it("an answer the computer took isn't sent again before the list moves, and a dropped queued Allow can be sent again", async () => {
    // Back on after the reply, the buttons sent a queued Allow again before
    // the list showed it queued (the review of 8f35e5d0).
    const decide = vi.fn(async () => true);
    show(approval, decide);
    await pause(700);
    act(() => allow().click());
    await pause(0);
    act(() => allow().click());
    act(() => deny().click());
    await pause(0);
    expect(decide).toHaveBeenCalledTimes(1);
    expect(allow().disabled).toBe(true);
    // The list shows it queued, then not: the computer dropped it.
    show({ ...approval, queued: true }, decide);
    expect(host.textContent).toContain("Waiting for you on your computer");
    show({ ...approval, queued: false }, decide);
    await pause(700);
    expect(allow().disabled).toBe(false);
    act(() => allow().click());
    await pause(0);
    expect(decide).toHaveBeenCalledTimes(2);
  });

  it("a pane's run is answered on the computer: no Allow on the phone, only Deny", () => {
    show(
      { ...approval, deskOnly: true },
      vi.fn(async () => false),
    );
    expect(host.querySelector('[data-testid="remote-approval-allow"]')).toBeNull();
    expect(host.textContent).toContain("Answer on your computer");
    expect(deny()).not.toBeNull();
  });
});
