"use client";

import { useCallback, useState } from "react";
import { Button, Input, Select } from "@tandem/ui";
import { Card, Notice, Shell } from "../../components/Shell";
import { api, ApiFailure, formatDate } from "../../lib/api";
import { formatCredits as n, ledgerLabel, type LedgerRow } from "../../lib/credits";
import { useMe } from "../../lib/useMe";

type Found = { id: string; email: string; name: string; role: string; createdAt: string };
type Detail = {
  user: { id: string; email: string; name: string; role: string; createdAt: string; deletionRequestedAt: string | null };
  plan: string;
  status: string;
  subscription: {
    plan: string;
    interval: string;
    status: string;
    seats: number;
    currentPeriodEnd: string;
    firstPaidAt: string | null;
    refundedAt: string | null;
  } | null;
  org: { name: string; role: string } | null;
  credits: { monthly: number; purchased: number; resetsAt: string | null; payer: { kind: string } };
  ledger: Array<LedgerRow & { note: string | null }>;
  actions: Array<{ id: string; action: string; reason: string; createdAt: string; adminEmail: string | null }>;
};

const failed = (err: unknown) => (err instanceof ApiFailure ? err.message : "That didn't work. Try again.");

/** F-BE-12: look a user up, see plan and ledger, grant credits, refund. Admins only. */
export default function AdminPage() {
  const { me, error } = useMe();
  const [q, setQ] = useState("");
  const [found, setFound] = useState<Found[] | null>(null);
  const [detail, setDetail] = useState<Detail | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);

  const open = useCallback(async (id: string) => {
    try {
      setDetail(await api<Detail>(`/v1/admin/users/${encodeURIComponent(id)}`));
      setErr(null);
    } catch (e) {
      setErr(failed(e));
    }
  }, []);

  if (error) return <Shell title="Admin"><Notice tone="error">{error}</Notice></Shell>;
  if (!me) return <Shell title="Admin"><Notice>Loading…</Notice></Shell>;
  if (me.user.role !== "admin") {
    return (
      <Shell title="Admin">
        <Notice tone="error">This page is for Vibezeo support staff.</Notice>
      </Shell>
    );
  }

  return (
    <Shell title="Admin" subtitle="Every change here is recorded with your name and the reason you give.">
      <div className="flex flex-col gap-6">
        <Card>
          <form
            className="flex items-end gap-3"
            onSubmit={(e) => {
              e.preventDefault();
              api<{ users: Found[] }>(`/v1/admin/users?q=${encodeURIComponent(q)}`)
                .then((r) => setFound(r.users))
                .catch((x: unknown) => setErr(failed(x)));
            }}
          >
            <Input label="Find a user by email" value={q} onChange={(e) => setQ(e.target.value)} className="w-80" />
            <Button type="submit" variant="primary">
              Search
            </Button>
          </form>
          {found?.length === 0 && <p className="mt-3 text-sm text-fg-muted">No users match.</p>}
          {found && found.length > 0 && (
            <ul className="mt-3 divide-y divide-divider text-sm" data-testid="admin-results">
              {found.map((u) => (
                <li key={u.id} className="flex items-center justify-between py-2">
                  <span>{`${u.email}${u.role === "admin" ? " · admin" : ""}`}</span>
                  <Button size="sm" variant="ghost" onClick={() => void open(u.id)}>
                    Open
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </Card>
        {err && <Notice tone="error">{err}</Notice>}
        {note && <Notice tone="success">{note}</Notice>}
        {detail && (
          <UserCard
            key={detail.user.id}
            d={detail}
            onDone={async (msg) => {
              setNote(msg);
              await open(detail.user.id);
            }}
            onError={setErr}
          />
        )}
      </div>
    </Shell>
  );
}

function UserCard({ d, onDone, onError }: { d: Detail; onDone: (msg: string) => Promise<void>; onError: (m: string) => void }) {
  const [amount, setAmount] = useState("");
  const [bucket, setBucket] = useState<"purchased" | "monthly">("purchased");
  const [reason, setReason] = useState("");
  const act = async (fn: () => Promise<unknown>, msg: string) => {
    try {
      await fn();
      setReason("");
      setAmount("");
      await onDone(msg);
    } catch (e) {
      onError(failed(e));
    }
  };
  const sub = d.subscription;
  return (
    <Card>
      <h2 className="text-lg font-semibold" data-testid="admin-user">
        {d.user.email}
      </h2>
      <dl className="mt-3 grid grid-cols-[160px_1fr] gap-y-1.5 text-sm" data-testid="admin-detail">
        <dt className="text-fg-muted">Plan</dt>
        <dd>{`${d.plan.toUpperCase()} · ${d.status}`}</dd>
        <dt className="text-fg-muted">Subscription</dt>
        <dd>
          {sub
            ? [
                `${sub.plan} ${sub.interval}`,
                sub.status,
                `${sub.seats} seat${sub.seats === 1 ? "" : "s"}`,
                sub.firstPaidAt ? `first paid ${formatDate(sub.firstPaidAt)}` : "never charged",
                sub.refundedAt ? `refunded ${formatDate(sub.refundedAt)}` : null,
              ]
                .filter(Boolean)
                .join(" · ")
            : "None"}
        </dd>
        <dt className="text-fg-muted">Organization</dt>
        <dd>{d.org ? `${d.org.name} (${d.org.role})` : "None"}</dd>
        <dt className="text-fg-muted">Credits</dt>
        <dd data-testid="admin-credits">{`${n(d.credits.monthly)} monthly · ${n(d.credits.purchased)} purchased${d.credits.payer.kind === "org" ? " (the org's pool)" : ""}`}</dd>
        <dt className="text-fg-muted">Joined</dt>
        <dd>{formatDate(d.user.createdAt)}</dd>
        {d.user.deletionRequestedAt && (
          <>
            <dt className="text-fg-muted">Deletion</dt>
            <dd>{`requested ${formatDate(d.user.deletionRequestedAt)}`}</dd>
          </>
        )}
      </dl>

      <div className="mt-6 flex flex-col gap-3">
        <Input label="Reason (required for every action)" value={reason} onChange={(e) => setReason(e.target.value)} />
        <div className="flex flex-wrap items-end gap-3">
          <Input label="Credits" type="number" min={1} value={amount} onChange={(e) => setAmount(e.target.value)} className="w-32" />
          <Select
            label="Bucket"
            value={bucket}
            onValueChange={(v) => setBucket(v as "purchased" | "monthly")}
            options={[
              { value: "purchased", label: "Purchased (never expire)" },
              { value: "monthly", label: "Monthly (reset with the plan)" },
            ]}
          />
          <Button
            variant="primary"
            disabled={!reason.trim() || !(Number(amount) > 0)}
            onClick={() =>
              void act(
                () => api(`/v1/admin/users/${d.user.id}/credits`, { method: "POST", body: { amount: Number(amount), reason, bucket } }),
                `Granted ${n(Number(amount))} credits.`,
              )
            }
          >
            Grant credits
          </Button>
        </div>
        <div className="flex flex-wrap gap-3">
          <Button
            variant="secondary"
            disabled={!reason.trim() || !sub || Boolean(sub.refundedAt)}
            onClick={() =>
              void act(() => api(`/v1/admin/users/${d.user.id}/refund`, { method: "POST", body: { reason, scope: "plan" } }), "Plan refunded.")
            }
          >
            Refund plan
          </Button>
          <Button
            variant="secondary"
            disabled={!reason.trim()}
            onClick={() =>
              void act(() => api(`/v1/admin/users/${d.user.id}/refund`, { method: "POST", body: { reason, scope: "pack" } }), "Credit pack refunded.")
            }
          >
            Refund last credit pack
          </Button>
        </div>
      </div>

      <h3 className="mt-6 text-md font-semibold">Ledger</h3>
      {d.ledger.length === 0 ? (
        <p className="mt-2 text-sm text-fg-muted">Nothing yet.</p>
      ) : (
        <table className="mt-2 w-full text-left text-sm tabular-nums" data-testid="admin-ledger">
          <tbody>
            {d.ledger.map((l) => (
              <tr key={l.id} className="border-t border-divider first:border-t-0">
                <td className="py-1.5 pr-4 text-fg-muted">{formatDate(l.createdAt)}</td>
                <td className="py-1.5 pr-4">{`${ledgerLabel(l)}${l.note ? ` · ${l.note}` : ""}`}</td>
                <td className="py-1.5 text-right">{`${l.delta > 0 ? "+" : ""}${n(l.delta)}`}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <h3 className="mt-6 text-md font-semibold">Admin actions</h3>
      {d.actions.length === 0 ? (
        <p className="mt-2 text-sm text-fg-muted">None.</p>
      ) : (
        <ul className="mt-2 text-sm" data-testid="admin-actions">
          {d.actions.map((a) => (
            <li key={a.id} className="py-1 text-fg-muted">{`${formatDate(a.createdAt)} · ${a.action.replace("_", " ")} · ${a.reason} · ${a.adminEmail ?? ""}`}</li>
          ))}
        </ul>
      )}
    </Card>
  );
}
