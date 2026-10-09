import type { OrgPolicy, OrgRole } from "@tandem/protocol/team";
import registry from "../../../crates/tandem-engines/registry.json";

export type OrgView = {
  id: string;
  name: string;
  role: OrgRole;
  plan: "free" | "pro" | "team";
  status: string;
  seats: number | null;
  memberCount: number;
  members: Array<{
    userId: string;
    email: string;
    name: string;
    role: OrgRole;
    monthlyCap: number | null;
    spent: number;
    joinedAt: string;
  }>;
  invites: Array<{ id: string; email: string; role: string; expiresAt: string }>;
  policy: OrgPolicy;
  credits: { monthly: number; purchased: number; resetsAt: string | null };
};

export type AuditRow = { id: string; event: string; meta: Record<string, unknown>; createdAt: string; actorEmail: string | null };

/** Engines a policy can allow: the app's own registry. */
export const ENGINE_CHOICES = (registry as Array<{ id: string; label: string }>).map((e) => ({ id: e.id, label: e.label }));

const EVENTS: Record<string, string> = {
  org_created: "Organization created",
  member_invited: "Member invited",
  member_added: "Member joined",
  member_removed: "Member removed",
  member_changed: "Member changed",
  policy_changed: "Policy changed",
  seats_changed: "Seats changed",
  full_access_used: "Full access used",
  plugin_write_approved: "Plugin write approved",
  plugin_connected: "Plugin connected",
};

export function auditLabel(row: AuditRow): string {
  const base = EVENTS[row.event] ?? row.event;
  const m = row.meta;
  if (row.event === "seats_changed") return `${base}: ${String(m.from)} → ${String(m.to)}`;
  if (row.event === "policy_changed" && typeof m.changed === "string" && m.changed) return `${base}: ${m.changed}`;
  if (row.event === "full_access_used" && typeof m.engine === "string") return `${base} · ${m.engine}`;
  return base;
}

export const ROLE_LABEL: Record<OrgRole, string> = { owner: "Owner", admin: "Admin", member: "Member" };
