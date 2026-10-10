// The Team plan's admin policy (PRODUCT_SPEC F-BL-06, ARCHITECTURE §14.4).
// It travels in the entitlement token's `policy` claim; the desktop mirrors
// it in `tandem_core::policy::OrgPolicy` and enforces it, and the server
// refuses hosted features it disables.

export type OrgRole = "owner" | "admin" | "member";
export type MaxAccess = "read" | "edits" | "full";

export type OrgPolicy = {
  /** Engine ids members may run; `null` means every engine. */
  allowedEngines: string[] | null;
  /** Plugin ids members may connect; `null` means every plugin. */
  allowedPlugins: string[] | null;
  /** The highest access level a thread may use. */
  maxAccess: MaxAccess;
  /** Usage statistics and crash reports stay off and locked. */
  forceTelemetryOff: boolean;
  disableAuto: boolean;
  disableConductor: boolean;
  /** The phone companion stays off. */
  disablePhone: boolean;
  /** Members' apps send metadata-only audit events (F-TL-10). */
  auditEnabled: boolean;
};

export const DEFAULT_POLICY: OrgPolicy = {
  allowedEngines: null,
  allowedPlugins: null,
  maxAccess: "full",
  forceTelemetryOff: false,
  disableAuto: false,
  disableConductor: false,
  disablePhone: false,
  auditEnabled: false,
};

/** Audit events members' apps may send (F-TL-10); everything else is dropped. */
export const CLIENT_AUDIT_EVENTS = ["full_access_used", "plugin_write_approved", "plugin_connected"] as const;
export type ClientAuditEvent = (typeof CLIENT_AUDIT_EVENTS)[number];

/** Events the server records itself. */
export const SERVER_AUDIT_EVENTS = [
  "org_created",
  "member_invited",
  "member_added",
  "member_removed",
  "member_changed",
  "policy_changed",
  "seats_changed",
] as const;
