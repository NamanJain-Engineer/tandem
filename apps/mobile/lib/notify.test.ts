import { describe, expect, it } from "vitest";
import { NoticeGate, present, routeNotice } from "./notify";
import { DEFAULT_PREFS, loadPrefs, savePrefs } from "./prefs";

describe("F-RM-05.3 local notifications", () => {
  it("banners in front, OS notifications behind, nothing when off", () => {
    expect(routeNotice(DEFAULT_PREFS, "active")).toBe("banner");
    expect(routeNotice(DEFAULT_PREFS, "background")).toBe("os");
    expect(routeNotice({ ...DEFAULT_PREFS, notifications: false }, "background")).toBe("none");
  });

  it("shows each notice once, and cuts long text", () => {
    const gate = new NoticeGate();
    const n = { kind: "approval" as const, id: "ap_1", title: "Approval needed", body: "x".repeat(400) };
    expect(gate.admit(n)).toBe(true);
    expect(gate.admit(n)).toBe(false);
    const shown = present(n);
    expect(shown.body.length).toBe(160);
    expect(shown.id).toBe("approval:ap_1");
  });

  it("F-RM-06.1 push starts off; in-app notifications start on", async () => {
    const map = new Map<string, string>();
    const kv = {
      getItemAsync: async (k: string) => map.get(k) ?? null,
      setItemAsync: async (k: string, v: string) => void map.set(k, v),
      deleteItemAsync: async (k: string) => void map.delete(k),
    };
    expect(await loadPrefs(kv)).toEqual({ notifications: true, push: false });
    await savePrefs(kv, { notifications: false, push: true });
    expect(await loadPrefs(kv)).toEqual({ notifications: false, push: true });
    map.set("vibezeo.prefs", "not json");
    expect(await loadPrefs(kv)).toEqual(DEFAULT_PREFS);
  });
});
