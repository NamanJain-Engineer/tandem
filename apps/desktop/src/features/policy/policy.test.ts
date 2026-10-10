import { describe, expect, it } from "vitest";
import type { OrgPolicy } from "@tandem/protocol";
import { allowsAccess, allowsEngine, clampAccess } from "./policy";

const policy = (p: Partial<OrgPolicy>): OrgPolicy => ({
  allowedEngines: null,
  allowedPlugins: null,
  maxAccess: "full",
  forceTelemetryOff: false,
  disableAuto: false,
  disableConductor: false,
  disablePhone: false,
  auditEnabled: false,
  ...p,
});

describe("F-BL-06 org policy in the app", () => {
  it("F-BL-06.1 max access Edits refuses Full and shows Edits", () => {
    expect(allowsAccess("edits", "full")).toBe(false);
    expect(allowsAccess("edits", "read")).toBe(true);
    expect(clampAccess("edits", "full")).toBe("edits");
    expect(clampAccess("full", "full")).toBe("full");
  });

  it("allowed engines", () => {
    expect(allowsEngine(null, "codex")).toBe(true);
    expect(allowsEngine(policy({ allowedEngines: ["claude"] }), "codex")).toBe(false);
    expect(allowsEngine(policy({ allowedEngines: ["claude"] }), "claude")).toBe(true);
  });
});
