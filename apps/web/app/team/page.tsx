"use client";

import { useCallback, useEffect, useState } from "react";
import { Button, Checkbox, Input, Select, Switch } from "@tandem/ui";
import { PRICES } from "@tandem/protocol/plans";
import { DEFAULT_POLICY, type MaxAccess, type OrgPolicy } from "@tandem/protocol/team";
import { Card, Notice, Shell } from "../../components/Shell";
import { api, ApiFailure, formatDate, type Me } from "../../lib/api";
import { formatCredits as n } from "../../lib/credits";
import { type AuditRow, auditLabel, ENGINE_CHOICES, type OrgView, ROLE_LABEL } from "../../lib/team";
import { useMe } from "../../lib/useMe";

const dollars = (cents: number) => `$${(cents / 100).toFixed(0)}`;
const failed = (err: unknown) => (err instanceof ApiFailure ? err.message : "That didn't work. Try again.");

/** F-WB-05 Team page (F-BL-06): members, invites, caps, seats, policy and the audit log. */
export default function TeamPage() {
  const { me, error, reload } = useMe();
  return (
    <Shell title="Team" subtitle="One plan for your team: pooled credits, shared rules, and a record of what matters.">
      {error && <Notice tone="error">{error}</Notice>}
      {!me && !error && <Notice>Loading…</Notice>}
      {me && !me.org && <StartTeam me={me} />}
      {me?.org && <Org me={me} onLeft={reload} />}
    </Shell>
  );
}

function StartTeam({ me }: { me: Me }) {
  const [seats, setSeats] = useState(String(PRICES.team.minSeats));
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  async function start() {
    setBusy(true);
    setErr(null);
    try {
      const { url } = await api<{ url: string }>("/v1/billing/checkout", {
        method: "POST",
        body: { plan: "team", interval: "month", seats: Math.max(PRICES.team.minSeats, Number(seats) || 0) },
      });
      window.location.assign(url);
    } catch (e) {
      setErr(failed(e));
      setBusy(false);
    }
  }
  return (
    <Card>
      <h2 className="text-lg font-semibold">Start a Team plan</h2>
      <p className="mt-1 text-sm text-fg-muted">
        {`${dollars(PRICES.team.month)} per seat per month, at least ${PRICES.team.minSeats} seats, with a ${PRICES.team.trialDays}-day trial. Each seat adds 10,000 credits a month to a pool the whole team shares.`}
      </p>
      {me.plan === "pro" && (
        <p className="mt-2 text-sm text-fg-muted">Your Pro plan stays as it is; cancel it from Billing if the team plan replaces it.</p>
      )}
      <div className="mt-4 flex items-end gap-3">
        <Input
          label="Seats"
          type="number"
          min={PRICES.team.minSeats}
          max={500}
          value={seats}
          onChange={(e) => setSeats(e.target.value)}
          className="w-28"
        />
        <Button variant="primary" loading={busy} onClick={() => void start()}>
          Start Team trial
        </Button>
      </div>
      {err && <div className="mt-3"><Notice tone="error">{err}</Notice></div>}
    </Card>
  );
}

function Org({ me, onLeft }: { me: Me; onLeft: () => Promise<void> }) {
  const orgId = me.org?.id ?? "";
  const [org, setOrg] = useState<OrgView | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  /** Bumped after each change, so the audit log shows it. */
  const [rev, setRev] = useState(0);

  const load = useCallback(async () => {
    try {
      setOrg(await api<OrgView>(`/v1/orgs/${encodeURIComponent(orgId)}`));
    } catch (e) {
      setErr(failed(e));
    }
  }, [orgId]);
  useEffect(() => {
    void load();
  }, [load]);

  const act = async (fn: () => Promise<unknown>, done?: string) => {
    setErr(null);
    setNote(null);
    try {
      await fn();
      if (done) setNote(done);
      await load();
      setRev((r) => r + 1);
    } catch (e) {
      setErr(failed(e));
    }
  };

  if (!org) return err ? <Notice tone="error">{err}</Notice> : <Notice>Loading…</Notice>;
  const admin = org.role !== "member";
  const self = org.members.find((m) => m.userId === me.user.id);

  return (
    <div className="flex flex-col gap-6">
      {err && <Notice tone="error">{err}</Notice>}
      {note && <Notice tone="success">{note}</Notice>}
      <Card>
        <div className="flex items-start justify-between gap-4">
          <div>
            <h2 className="text-lg font-semibold" data-testid="org-name">
              {org.name}
            </h2>
            <p className="mt-1 text-sm text-fg-muted" data-testid="org-summary">
              {[
                org.plan === "team" ? "Team plan" : "No active Team plan",
                org.seats ? `${org.memberCount} of ${org.seats} seats used` : null,
                `you're ${ROLE_LABEL[org.role].toLowerCase()}`,
              ]
                .filter(Boolean)
                .join(" · ")}
            </p>
            <p className="mt-1 text-sm text-fg-muted">{`Pool: ${n(org.credits.monthly)} monthly + ${n(org.credits.purchased)} purchased credits`}</p>
            {self?.monthlyCap != null && (
              <p className="mt-1 text-sm" data-testid="my-cap">{`Your cap: ${n(self.spent)} of ${n(self.monthlyCap)} credits this month`}</p>
            )}
          </div>
          {org.role !== "owner" && (
            <Button
              variant="secondary"
              onClick={() =>
                void act(async () => {
                  await api(`/v1/orgs/${orgId}/members/${me.user.id}`, { method: "DELETE" });
                  await onLeft();
                })
              }
            >
              Leave organization
            </Button>
          )}
        </div>
        {admin && org.seats !== null && <Seats org={org} onSave={(seats) => act(() => api(`/v1/orgs/${orgId}/seats`, { method: "PUT", body: { seats } }), "Seats updated.")} />}
      </Card>

      {admin && (
        <>
          <Members org={org} me={me} act={act} />
          <Invite org={org} act={act} />
          <Policy org={org} act={act} />
          <Audit orgId={orgId} rev={rev} />
        </>
      )}
      {!admin && (
        <Card>
          <h2 className="text-lg font-semibold">Your organization&apos;s rules</h2>
          <PolicySummary policy={org.policy} />
        </Card>
      )}
    </div>
  );
}

function Seats({ org, onSave }: { org: OrgView; onSave: (seats: number) => Promise<void> }) {
  const [seats, setSeats] = useState(String(org.seats ?? PRICES.team.minSeats));
  return (
    <div className="mt-4 flex items-end gap-3">
      <Input label="Seats" type="number" min={PRICES.team.minSeats} max={500} value={seats} onChange={(e) => setSeats(e.target.value)} className="w-28" />
      <Button variant="secondary" onClick={() => void onSave(Number(seats))} disabled={Number(seats) === org.seats}>
        Change seats
      </Button>
      <p className="pb-1.5 text-sm text-fg-muted">{`${dollars(PRICES.team.month)} per seat per month; the next invoice follows.`}</p>
    </div>
  );
}

type Act = (fn: () => Promise<unknown>, done?: string) => Promise<void>;

function Members({ org, me, act }: { org: OrgView; me: Me; act: Act }) {
  const [caps, setCaps] = useState<Record<string, string>>({});
  return (
    <Card>
      <h2 className="text-lg font-semibold">Members</h2>
      <table className="mt-3 w-full text-left text-sm" data-testid="members">
        <thead className="text-fg-subtle">
          <tr>
            <th className="py-2 pr-4 font-medium">Member</th>
            <th className="py-2 pr-4 font-medium">Role</th>
            <th className="py-2 pr-4 font-medium">Monthly cap</th>
            <th className="py-2 pr-4 font-medium">Used</th>
            <th className="py-2" />
          </tr>
        </thead>
        <tbody>
          {org.members.map((m) => {
            const draft = caps[m.userId] ?? (m.monthlyCap === null ? "" : String(m.monthlyCap));
            return (
              <tr key={m.userId} className="border-t border-divider" data-testid={`member-${m.email}`}>
                <td className="py-2 pr-4">
                  <p>{m.name || m.email}</p>
                  <p className="text-fg-muted">{m.email}</p>
                </td>
                <td className="py-2 pr-4">
                  {m.role === "owner" || org.role !== "owner" ? (
                    ROLE_LABEL[m.role]
                  ) : (
                    <Select
                      ariaLabel={`Role for ${m.email}`}
                      size="sm"
                      value={m.role}
                      options={[
                        { value: "member", label: "Member" },
                        { value: "admin", label: "Admin" },
                      ]}
                      onValueChange={(role) => void act(() => api(`/v1/orgs/${org.id}/members/${m.userId}`, { method: "PATCH", body: { role } }))}
                    />
                  )}
                </td>
                <td className="py-2 pr-4">
                  <div className="flex items-center gap-2">
                    <Input
                      label={`Monthly cap for ${m.email}`}
                      hideLabel
                      placeholder="No cap"
                      type="number"
                      min={0}
                      value={draft}
                      onChange={(e) => setCaps({ ...caps, [m.userId]: e.target.value })}
                      className="w-28"
                    />
                    <Button
                      size="sm"
                      variant="ghost"
                      disabled={draft === (m.monthlyCap === null ? "" : String(m.monthlyCap))}
                      onClick={() =>
                        void act(
                          () =>
                            api(`/v1/orgs/${org.id}/members/${m.userId}`, {
                              method: "PATCH",
                              body: { monthly_cap: draft.trim() === "" ? null : Number(draft) },
                            }),
                          "Cap saved.",
                        )
                      }
                    >
                      Save
                    </Button>
                  </div>
                </td>
                <td className="py-2 pr-4 tabular-nums">{n(m.spent)}</td>
                <td className="py-2 text-right">
                  {m.role !== "owner" && m.userId !== me.user.id && (
                    <Button size="sm" variant="ghost" onClick={() => void act(() => api(`/v1/orgs/${org.id}/members/${m.userId}`, { method: "DELETE" }), "Member removed.")}>
                      Remove
                    </Button>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {org.invites.length > 0 && (
        <>
          <h3 className="mt-6 text-md font-semibold">Pending invites</h3>
          <ul className="mt-2 text-sm" data-testid="invites">
            {org.invites.map((i) => (
              <li key={i.id} className="py-1 text-fg-muted">{`${i.email} · ${i.role} · expires ${formatDate(i.expiresAt)}`}</li>
            ))}
          </ul>
        </>
      )}
    </Card>
  );
}

function Invite({ org, act }: { org: OrgView; act: Act }) {
  const [email, setEmail] = useState("");
  const [role, setRole] = useState<"member" | "admin">("member");
  return (
    <Card>
      <h2 className="text-lg font-semibold">Invite someone</h2>
      <p className="mt-1 text-sm text-fg-muted">They get an email link that works for 7 days. A pending invite holds a seat.</p>
      <form
        className="mt-4 flex flex-wrap items-end gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          void act(() => api(`/v1/orgs/${org.id}/invites`, { method: "POST", body: { email, role } }), `Invite sent to ${email}.`).then(() => setEmail(""));
        }}
      >
        <Input label="Email" type="email" required value={email} onChange={(e) => setEmail(e.target.value)} className="w-72" />
        {org.role === "owner" && (
          <Select
            label="Role"
            value={role}
            onValueChange={(v) => setRole(v as "member" | "admin")}
            options={[
              { value: "member", label: "Member" },
              { value: "admin", label: "Admin" },
            ]}
          />
        )}
        <Button type="submit" variant="primary">
          Send invite
        </Button>
      </form>
    </Card>
  );
}

const ACCESS: Array<{ value: MaxAccess; label: string }> = [
  { value: "full", label: "Full (no limit)" },
  { value: "edits", label: "Edits" },
  { value: "read", label: "Read only" },
];

function Policy({ org, act }: { org: OrgView; act: Act }) {
  const [p, setP] = useState<OrgPolicy>({ ...DEFAULT_POLICY, ...org.policy });
  const [plugins, setPlugins] = useState((org.policy.allowedPlugins ?? []).join(", "));
  const set = (patch: Partial<OrgPolicy>) => setP({ ...p, ...patch });
  const save = () =>
    act(
      () =>
        api(`/v1/orgs/${org.id}/policy`, {
          method: "PUT",
          body: {
            ...p,
            allowedPlugins:
              p.allowedPlugins === null
                ? null
                : plugins
                    .split(",")
                    .map((s) => s.trim())
                    .filter(Boolean),
          },
        }),
      "Policy saved. Members' apps pick it up within seconds.",
    );
  return (
    <Card>
      <h2 className="text-lg font-semibold">Policy</h2>
      <p className="mt-1 text-sm text-fg-muted">Members&apos; apps enforce these, and the hosted services refuse what they forbid.</p>
      <div className="mt-4 flex flex-col gap-5">
        <Select label="Highest access level" value={p.maxAccess} options={ACCESS} onValueChange={(v) => set({ maxAccess: v as MaxAccess })} />
        <div>
          <Switch label="All engines" checked={p.allowedEngines === null} onCheckedChange={(all) => set({ allowedEngines: all ? null : ENGINE_CHOICES.map((e) => e.id) })} />
          {p.allowedEngines !== null && (
            <div className="mt-2 grid grid-cols-2 gap-2 sm:grid-cols-3">
              {ENGINE_CHOICES.map((e) => (
                <Checkbox
                  key={e.id}
                  label={e.label}
                  checked={p.allowedEngines?.includes(e.id) ?? false}
                  onCheckedChange={(on) =>
                    set({ allowedEngines: on ? [...(p.allowedEngines ?? []), e.id] : (p.allowedEngines ?? []).filter((x) => x !== e.id) })
                  }
                />
              ))}
            </div>
          )}
        </div>
        <div>
          <Switch label="All plugins" checked={p.allowedPlugins === null} onCheckedChange={(all) => set({ allowedPlugins: all ? null : [] })} />
          {p.allowedPlugins !== null && (
            <div className="mt-2">
              <Input label="Allowed plugin ids, separated by commas" value={plugins} onChange={(e) => setPlugins(e.target.value)} />
            </div>
          )}
        </div>
        <Switch label="Keep usage statistics and crash reports off" checked={p.forceTelemetryOff} onCheckedChange={(v) => set({ forceTelemetryOff: v })} />
        <Switch label="Turn off Auto" checked={p.disableAuto} onCheckedChange={(v) => set({ disableAuto: v })} />
        <Switch label="Turn off Conductor" checked={p.disableConductor} onCheckedChange={(v) => set({ disableConductor: v })} />
        <Switch label="Keep the phone companion off" checked={p.disablePhone} onCheckedChange={(v) => set({ disablePhone: v })} />
        <Switch
          label="Audit log: members' apps report Full access use and plugin write approvals (no content)"
          checked={p.auditEnabled}
          onCheckedChange={(v) => set({ auditEnabled: v })}
        />
        <div>
          <Button variant="primary" onClick={() => void save()}>
            Save policy
          </Button>
        </div>
      </div>
    </Card>
  );
}

function PolicySummary({ policy }: { policy: OrgPolicy }) {
  const items = [
    `Highest access level: ${ACCESS.find((a) => a.value === policy.maxAccess)?.label ?? policy.maxAccess}`,
    policy.allowedEngines === null ? "Every engine is allowed" : `Engines: ${policy.allowedEngines.join(", ") || "none"}`,
    policy.allowedPlugins === null ? "Every plugin is allowed" : `Plugins: ${policy.allowedPlugins.join(", ") || "none"}`,
    policy.forceTelemetryOff ? "Usage statistics and crash reports stay off" : null,
    policy.disableAuto ? "Auto is off" : null,
    policy.disableConductor ? "Conductor is off" : null,
    policy.disablePhone ? "The phone companion is off" : null,
    policy.auditEnabled ? "Your app reports Full access use and plugin write approvals to the admins (no content)" : null,
  ].filter((x): x is string => Boolean(x));
  return (
    <ul className="mt-3 list-disc pl-5 text-sm" data-testid="policy-summary">
      {items.map((i) => (
        <li key={i}>{i}</li>
      ))}
    </ul>
  );
}

function Audit({ orgId, rev }: { orgId: string; rev: number }) {
  const [rows, setRows] = useState<AuditRow[] | null>(null);
  const [next, setNext] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const load = useCallback(
    async (cursor?: string) => {
      try {
        const page = await api<{ events: AuditRow[]; next: string | null }>(
          `/v1/orgs/${orgId}/audit${cursor ? `?cursor=${encodeURIComponent(cursor)}` : ""}`,
        );
        setRows((r) => (cursor ? [...(r ?? []), ...page.events] : page.events));
        setNext(page.next);
      } catch (e) {
        setErr(failed(e));
      }
    },
    [orgId],
  );
  useEffect(() => {
    void load();
  }, [load, rev]);
  return (
    <Card>
      <h2 className="text-lg font-semibold">Audit log</h2>
      <p className="mt-1 text-sm text-fg-muted">Metadata only: who did what, and when. Never prompts, files or terminal output.</p>
      {err && <Notice tone="error">{err}</Notice>}
      {!rows && !err && <p className="mt-3 text-sm text-fg-muted">Loading…</p>}
      {rows?.length === 0 && <p className="mt-3 text-sm text-fg-muted">Nothing yet.</p>}
      {rows && rows.length > 0 && (
        <table className="mt-3 w-full text-left text-sm" data-testid="audit-log">
          <tbody>
            {rows.map((r) => (
              <tr key={r.id} className="border-t border-divider first:border-t-0">
                <td className="py-2 pr-4 text-fg-muted">{formatDate(r.createdAt)}</td>
                <td className="py-2 pr-4">{auditLabel(r)}</td>
                <td className="py-2 text-fg-muted">{r.actorEmail ?? ""}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {next && (
        <div className="mt-3">
          <Button size="sm" variant="ghost" onClick={() => void load(next)}>
            Load older
          </Button>
        </div>
      )}
    </Card>
  );
}
