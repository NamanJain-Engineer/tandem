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
  sso_changed: "Single sign-on changed",
  sso_removed: "Single sign-on removed",
  sso_domain_added: "SSO domain added",
  sso_domain_verified: "SSO domain verified",
  sso_domain_removed: "SSO domain removed",
  sso_sign_in: "Signed in with SSO",
};

/** `GET /v1/orgs/:id/sso` (F-BL-06 P2). */
export type SsoView = {
  connection: {
    provider: "oidc" | "workos" | "mock";
    issuer: string | null;
    clientId: string | null;
    hasSecret: boolean;
    organization: string | null;
    enforce: boolean;
    updatedAt: string;
  } | null;
  domains: Array<{ domain: string; verified: boolean; record: { type: "TXT"; name: string; value: string } }>;
  providers: { oidc: boolean; workos: boolean };
  redirectUri: string;
};

export const SSO_PROVIDER_LABEL: Record<"oidc" | "workos" | "mock", string> = {
  oidc: "OpenID Connect",
  workos: "SAML through WorkOS",
  mock: "Test identity provider",
};

export function auditLabel(row: AuditRow): string {
  const base = EVENTS[row.event] ?? row.event;
  const m = row.meta;
  if (row.event === "seats_changed") return `${base}: ${String(m.from)} → ${String(m.to)}`;
  if (row.event === "policy_changed" && typeof m.changed === "string" && m.changed) return `${base}: ${m.changed}`;
  if (row.event === "full_access_used" && typeof m.engine === "string") return `${base} · ${m.engine}`;
  if (row.event.startsWith("sso_domain_") && typeof m.domain === "string") return `${base}: ${m.domain}`;
  return base;
}

export const ROLE_LABEL: Record<OrgRole, string> = { owner: "Owner", admin: "Admin", member: "Member" };
