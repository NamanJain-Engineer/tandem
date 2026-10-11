import { describe, expect, it } from "vitest";
import type { SimStatus } from "@tandem/protocol";
import { initialSimPane, reduceSim, simInputFor, toDevicePoint, type SimPane } from "./model";

const A = "8A3F1C2E-1111-4A6B-9C1D-0123456789AB";
const B = "5B2E9D10-2222-4C3D-8E9F-0123456789AB";
const status = (over: Partial<SimStatus> = {}): SimStatus => ({
  available: true,
  reason: null,
  runtimes: [{ identifier: "rt", name: "iOS 18.0", version: "18.0" }],
  devices: [
    { udid: A, name: "iPhone 16", runtime: "iOS 18.0", state: "shutdown" },
    { udid: B, name: "iPad Air", runtime: "iOS 18.0", state: "booted" },
  ],
  input: true,
  ...over,
});
const run = (...events: Parameters<typeof reduceSim>[1][]): SimPane => events.reduce(reduceSim, initialSimPane);

describe("the Simulator pane's state machine (F-CD-21)", () => {
  it("F-CD-21.2 no tools or no runtime is unavailable with the reason; no devices is empty", () => {
    expect(run({ type: "status", status: status({ available: false, reason: "No iOS Simulator runtime is installed.", devices: [] }) })).toMatchObject({
      phase: "unavailable",
      message: "No iOS Simulator runtime is installed.",
    });
    expect(run({ type: "status", status: status({ devices: [] }) }).phase).toBe("empty");
    expect(run({ type: "status_failed", message: "x" }).phase).toBe("unavailable");
  });

  it("F-CD-21 picks the first device (booted ones come first from the core) and shows its state", () => {
    const booted = status({ devices: [status().devices[1]!, status().devices[0]!] });
    expect(run({ type: "status", status: booted })).toMatchObject({ phase: "live", udid: B });
    expect(run({ type: "status", status: status() })).toMatchObject({ phase: "off", udid: A });
  });

  it("F-CD-21 boot → booted → shutdown → stopped, and a device switch waits for a boot in flight", () => {
    let s = run({ type: "status", status: status() });
    s = reduceSim(s, { type: "boot" });
    expect(s.phase).toBe("booting");
    expect(reduceSim(s, { type: "select", udid: B }).udid).toBe(A);
    // A refresh during the boot keeps booting.
    expect(reduceSim(s, { type: "status", status: status() }).phase).toBe("booting");
    s = reduceSim(s, { type: "booted" });
    expect(s.phase).toBe("live");
    expect(s.status?.devices.find((d) => d.udid === A)?.state).toBe("booted");
    s = reduceSim(s, { type: "shutdown" });
    expect(s.phase).toBe("stopping");
    s = reduceSim(s, { type: "stopped" });
    expect(s).toMatchObject({ phase: "off", udid: A });
    s = reduceSim(s, { type: "select", udid: B });
    expect(s).toMatchObject({ phase: "live", udid: B });
    // Shutdown only from live; boot only from off or failed.
    expect(reduceSim(run({ type: "status", status: status() }), { type: "shutdown" }).phase).toBe("off");
    const failed = reduceSim(reduceSim(run({ type: "status", status: status() }), { type: "boot" }), { type: "failed", message: "Unable to boot" });
    expect(failed).toMatchObject({ phase: "failed", message: "Unable to boot" });
    expect(reduceSim(failed, { type: "boot" }).phase).toBe("booting");
  });
});

describe("clicks and keys (F-CD-21)", () => {
  const screen = { widthPoints: 393, heightPoints: 852 };
  it("maps a click on a letterboxed frame to device points, and ignores the bars", () => {
    // A 1179×2556 frame drawn in an 800×600 box: 276.76 wide, centered.
    const box = { left: 100, top: 50, width: 800, height: 600 };
    const img = { width: 1179, height: 2556 };
    const drawnW = 1179 * (600 / 2556);
    const left = 100 + (800 - drawnW) / 2;
    const mid = toDevicePoint(left + drawnW / 2, 50 + 300, box, img, screen);
    expect(mid?.x).toBeCloseTo(196.5, 3);
    expect(mid?.y).toBeCloseTo(426, 3);
    const corner = toDevicePoint(left, 50, box, img, screen);
    expect(corner).toEqual({ x: 0, y: 0 });
    expect(toDevicePoint(left - 5, 300, box, img, screen)).toBeNull();
    expect(toDevicePoint(10, 10, { left: 0, top: 0, width: 0, height: 0 }, img, screen)).toBeNull();
  });

  it("named keys and characters go to the device; shortcuts stay with the app", () => {
    const k = (key: string, mods: Partial<{ metaKey: boolean; ctrlKey: boolean; altKey: boolean }> = {}) =>
      simInputFor({ key, metaKey: false, ctrlKey: false, altKey: false, ...mods });
    expect(k("Enter")).toEqual({ key: "enter" });
    expect(k("ArrowUp")).toEqual({ key: "up" });
    expect(k("a")).toEqual({ text: "a" });
    expect(k("é")).toEqual({ text: "é" });
    expect(k("Shift")).toBeNull();
    expect(k("w", { metaKey: true })).toBeNull();
    expect(k("c", { ctrlKey: true })).toBeNull();
  });
});
