import type { SimDevice, SimKey, SimScreen, SimStatus } from "@tandem/protocol";

/** What a Simulator pane shows (F-CD-21). */
export type SimPhase = "loading" | "unavailable" | "empty" | "off" | "booting" | "live" | "stopping" | "failed";

export type SimPane = {
  phase: SimPhase;
  status: SimStatus | null;
  udid: string | null;
  /** Why it's unavailable or failed. */
  message: string | null;
};

export type SimEvent =
  | { type: "status"; status: SimStatus }
  | { type: "status_failed"; message: string }
  | { type: "select"; udid: string }
  | { type: "boot" }
  | { type: "booted" }
  | { type: "shutdown" }
  | { type: "stopped" }
  | { type: "failed"; message: string };

export const initialSimPane: SimPane = { phase: "loading", status: null, udid: null, message: null };

const phaseOf = (d: SimDevice): SimPhase =>
  d.state === "booted" ? "live" : d.state === "booting" ? "booting" : d.state === "shutting_down" ? "stopping" : "off";

export function deviceOf(s: SimPane): SimDevice | null {
  return s.status?.devices.find((d) => d.udid === s.udid) ?? null;
}

/** The pane's state machine. A boot or shutdown in flight isn't interrupted by a
 * device switch; the device list keeps the chosen device while it exists. */
export function reduceSim(s: SimPane, e: SimEvent): SimPane {
  switch (e.type) {
    case "status": {
      const st = e.status;
      if (!st.available) return { phase: "unavailable", status: st, udid: null, message: st.reason };
      if (st.devices.length === 0) return { phase: "empty", status: st, udid: null, message: null };
      const keep = st.devices.find((d) => d.udid === s.udid);
      const device = keep ?? st.devices[0];
      if (!device) return { phase: "empty", status: st, udid: null, message: null };
      // A boot or shutdown in flight keeps its phase until it answers.
      const busy = s.phase === "booting" || s.phase === "stopping";
      return { phase: busy && keep ? s.phase : phaseOf(device), status: st, udid: device.udid, message: null };
    }
    case "status_failed":
      return { ...s, phase: "unavailable", message: e.message };
    case "select": {
      if (s.phase === "booting" || s.phase === "stopping") return s;
      const device = s.status?.devices.find((d) => d.udid === e.udid);
      return device ? { ...s, udid: device.udid, phase: phaseOf(device), message: null } : s;
    }
    case "boot":
      return s.phase === "off" || s.phase === "failed" ? { ...s, phase: "booting", message: null } : s;
    case "booted":
      return { ...s, phase: "live", status: withState(s, "booted"), message: null };
    case "shutdown":
      return s.phase === "live" ? { ...s, phase: "stopping", message: null } : s;
    case "stopped":
      return { ...s, phase: "off", status: withState(s, "shutdown"), message: null };
    case "failed":
      return { ...s, phase: "failed", message: e.message };
  }
}

function withState(s: SimPane, state: SimDevice["state"]): SimStatus | null {
  if (!s.status) return null;
  return { ...s.status, devices: s.status.devices.map((d) => (d.udid === s.udid ? { ...d, state } : d)) };
}

export type Rect = { left: number; top: number; width: number; height: number };

/** A click on the frame (drawn with `object-fit: contain`) as a point on the
 * device's screen in points, or null when it's outside the picture. */
export function toDevicePoint(
  clientX: number,
  clientY: number,
  box: Rect,
  image: { width: number; height: number },
  screen: Pick<SimScreen, "widthPoints" | "heightPoints">,
): { x: number; y: number } | null {
  if (box.width <= 0 || box.height <= 0 || image.width <= 0 || image.height <= 0) return null;
  const scale = Math.min(box.width / image.width, box.height / image.height);
  const drawnW = image.width * scale;
  const drawnH = image.height * scale;
  const fx = (clientX - box.left - (box.width - drawnW) / 2) / drawnW;
  const fy = (clientY - box.top - (box.height - drawnH) / 2) / drawnH;
  if (fx < 0 || fy < 0 || fx > 1 || fy > 1) return null;
  return { x: fx * screen.widthPoints, y: fy * screen.heightPoints };
}

const KEYS: Record<string, SimKey> = {
  Enter: "enter",
  Backspace: "backspace",
  Tab: "tab",
  Escape: "escape",
  ArrowLeft: "left",
  ArrowRight: "right",
  ArrowUp: "up",
  ArrowDown: "down",
};

/** A keystroke for the device: a named key, a character to type, or nothing
 * (shortcuts with Mod/Ctrl/Alt stay with the app). */
export function simInputFor(e: { key: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean }):
  | { key: SimKey }
  | { text: string }
  | null {
  if (e.metaKey || e.ctrlKey || e.altKey) return null;
  const named = KEYS[e.key];
  if (named) return { key: named };
  return [...e.key].length === 1 ? { text: e.key } : null;
}
