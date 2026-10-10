import { describe, expect, it } from "vitest";
import type { EntitlementView } from "@tandem/protocol";
import { FREE_VIEW } from "../../test/fakeBackend";
import { flowRows, HOSTED_FEATURES } from "./flows";

const row = (rows: ReturnType<typeof flowRows>, id: string) => rows.find((r) => r.id === id);

describe("F-TL-07 what leaves this machine", () => {
  it("F-TL-07.1 enabling cloud STT turns its row On, with its destination", () => {
    const off = flowRows({ dictate_engine: "local" }, FREE_VIEW, true);
    expect(row(off, "cloud_stt")?.status).toBe("off");
    const on = flowRows({ dictate_engine: "cloud" }, FREE_VIEW, true);
    expect(row(on, "cloud_stt")).toMatchObject({
      status: "on",
      sends: "Audio of each take",
      destination: "Vibezeo API → speech-to-text provider",
    });
  });

  it("covers every hosted feature, telemetry and the Team audit", () => {
    const ids = HOSTED_FEATURES.map((f) => f.id);
    expect(ids).toEqual(
      expect.arrayContaining(["cloud_stt", "enhance", "auto", "usage_stats", "crash_reports", "team_audit", "thread_titles"]),
    );
  });

  it("an org that forces telemetry off shows it off; its audit shows on", () => {
    const team: EntitlementView = {
      ...FREE_VIEW,
      plan: "team",
      policy: {
        allowedEngines: null,
        allowedPlugins: null,
        maxAccess: "full",
        forceTelemetryOff: true,
        disableAuto: false,
        disableConductor: false,
        disablePhone: false,
        auditEnabled: true,
      },
    };
    const rows = flowRows({ telemetry_usage: true }, team, true);
    expect(row(rows, "usage_stats")?.status).toBe("off");
    expect(row(rows, "team_audit")?.status).toBe("on");
    // Signed out, nothing account-bound is sent.
    expect(row(flowRows({}, null, false), "account")?.status).toBe("off");
  });
});
