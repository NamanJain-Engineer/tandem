import hosted from "@tandem/protocol/hosted-features.json";
import type { EntitlementView, OrgPolicy, SettingKey } from "@tandem/protocol";

/** One row of hosted-features.json (F-TL-07): its text is the single source for the app and the site. */
export type HostedFeature = {
  id: string;
  label: string;
  sends: string;
  destination: string;
  state:
    | { kind: "setting"; key: SettingKey; equals: unknown }
    | { kind: "on_use" }
    | { kind: "signed_in" }
    | { kind: "policy"; field: keyof OrgPolicy };
};

export const HOSTED_FEATURES = hosted.features as HostedFeature[];

export type FlowStatus = "on" | "off" | "on_use";

export type FlowRow = { id: string; label: string; status: FlowStatus; sends: string; destination: string };

/** "What leaves this machine", live: each row's state from settings, sign-in and the org policy. */
export function flowRows(
  values: Partial<Record<SettingKey, unknown>>,
  entitlement: EntitlementView | null,
  signedIn: boolean,
  features: HostedFeature[] = HOSTED_FEATURES,
): FlowRow[] {
  const policy = entitlement?.policy ?? null;
  return features.map((f) => {
    let status: FlowStatus;
    switch (f.state.kind) {
      case "setting": {
        const locked =
          (f.state.key === "telemetry_usage" || f.state.key === "telemetry_crash") && policy?.forceTelemetryOff;
        status = !locked && values[f.state.key] === f.state.equals ? "on" : "off";
        break;
      }
      case "on_use":
        status = signedIn ? "on_use" : "off";
        break;
      case "signed_in":
        status = signedIn ? "on" : "off";
        break;
      case "policy":
        status = policy?.[f.state.field] === true ? "on" : "off";
        break;
    }
    return { id: f.id, label: f.label, status, sends: f.sends, destination: f.destination };
  });
}
