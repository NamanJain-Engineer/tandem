import { describe, expect, it } from "vitest";
import { auditLabel, ENGINE_CHOICES } from "./team";

describe("F-BL-06 Team page helpers", () => {
  it("offers the app's own engines for the allowlist", () => {
    expect(ENGINE_CHOICES.map((e) => e.id)).toEqual(expect.arrayContaining(["claude", "codex"]));
  });

  it("names audit rows in words, metadata only", () => {
    const row = { id: "a", createdAt: "", actorEmail: null };
    expect(auditLabel({ ...row, event: "seats_changed", meta: { from: 3, to: 5 } })).toBe("Seats changed: 3 → 5");
    expect(auditLabel({ ...row, event: "full_access_used", meta: { engine: "claude" } })).toBe("Full access used · claude");
  });
});
