import type { Access, OrgPolicy } from "@tandem/protocol";

const RANK: Record<Access, number> = { read: 0, edits: 1, full: 2 };

/** Whether `access` is within a Team org's maximum (F-BL-06). */
export function allowsAccess(max: Access, access: Access): boolean {
  return RANK[access] <= RANK[max];
}

/** `access` lowered to the org's maximum, as the core starts the session (F-BL-06). */
export function clampAccess(max: Access, access: Access): Access {
  return allowsAccess(max, access) ? access : max;
}

export function allowsEngine(policy: OrgPolicy | null | undefined, engineId: string): boolean {
  return !policy?.allowedEngines || policy.allowedEngines.includes(engineId);
}
