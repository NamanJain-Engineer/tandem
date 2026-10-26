"use client";

import { useEffect, useState } from "react";
import { Button } from "@tandem/ui";
import { CREDIT_PACKS, type CreditPack } from "@tandem/protocol/plans";
import { Card, Notice, Shell } from "../../components/Shell";
import { api, ApiFailure, formatDate } from "../../lib/api";
import { type Credits, formatCredits as n, ledgerLabel, PRICE_LIST } from "../../lib/credits";
import { useMe } from "../../lib/useMe";

const dollars = (cents: number) => `$${(cents / 100).toFixed(0)}`;

export default function CreditsPage() {
  const { me, error: meError } = useMe();
  const [credits, setCredits] = useState<Credits | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<CreditPack | null>(null);
  const [purchased, setPurchased] = useState<string | null>(null);

  useEffect(() => {
    setPurchased(new URLSearchParams(window.location.search).get("purchased"));
    api<Credits>("/v1/credits")
      .then(setCredits)
      .catch((err: unknown) => {
        if (err instanceof ApiFailure && err.status === 401) window.location.replace("/signin?next=/credits");
        else setError("Couldn't load your credits.");
      });
  }, []);

  async function buy(pack: CreditPack) {
    setBusy(pack);
    setError(null);
    try {
      const { url } = await api<{ url: string }>("/v1/billing/credit-packs/checkout", { method: "POST", body: { pack } });
      window.location.assign(url);
    } catch (err) {
      setError(err instanceof ApiFailure ? err.message : "That didn't work. Try again.");
      setBusy(null);
    }
  }

  const member = me?.org?.role === "member" && me.plan === "team";
  const packs = Object.entries(CREDIT_PACKS) as Array<[CreditPack, (typeof CREDIT_PACKS)[CreditPack]]>;
  return (
    <Shell title="Credits" subtitle="Credits pay for hosted features. Coding uses your own AI accounts and costs no credits.">
      {(error ?? meError) && <Notice tone="error">{error ?? meError}</Notice>}
      {purchased && purchased in CREDIT_PACKS && (
        <Notice tone="success">
          {`Thanks: ${n(CREDIT_PACKS[purchased as CreditPack].credits)} credits are on their way to your balance.`}
        </Notice>
      )}
      {!credits && !error && <Notice>Loading…</Notice>}
      {credits && me && (
        <div className="mt-4 flex flex-col gap-6">
          <Card>
            {credits.org && <p className="mb-3 text-sm text-fg-muted">{`Your organization's pool: ${me.org?.name ?? ""}`}</p>}
            <dl className="grid grid-cols-[160px_1fr] gap-y-2 text-base tabular-nums">
              <dt className="text-fg-muted">Monthly</dt>
              <dd data-testid="credits-monthly">{n(credits.balance.monthly)}</dd>
              <dt className="text-fg-muted">Purchased</dt>
              <dd data-testid="credits-purchased">{n(credits.balance.purchased)}</dd>
              <dt className="text-fg-muted">Resets</dt>
              <dd>{credits.balance.resetsAt ? formatDate(credits.balance.resetsAt) : "—"}</dd>
              {credits.org?.cap != null && (
                <>
                  <dt className="text-fg-muted">Your monthly cap</dt>
                  <dd data-testid="credits-cap">{`${n(credits.org.spent)} of ${n(credits.org.cap)} used`}</dd>
                </>
              )}
            </dl>
          </Card>

          <Card>
            <h2 className="text-lg font-semibold">Buy credits</h2>
            <p className="mt-1 text-sm text-fg-muted">
              Purchased credits never expire and are spent after your monthly credits.
            </p>
            {!me.limits.creditPacks ? (
              <p className="mt-4 text-sm" data-testid="packs-nothing-to-buy">
                There&apos;s nothing to buy: everything that runs on your computer works without an account.
                Hosted features come with Vibezeo accounts.
              </p>
            ) : member ? (
              <p className="mt-4 text-sm" data-testid="packs-ask-admin">
                Your organization&apos;s owner or admins buy credits for the team.
              </p>
            ) : (
              <div className="mt-4 flex flex-wrap gap-3">
                {packs.map(([id, p]) => (
                  <Button key={id} variant="secondary" loading={busy === id} onClick={() => void buy(id)} data-testid={`pack-${id}`}>
                    {`${n(p.credits)} credits · ${dollars(p.priceCents)}`}
                  </Button>
                ))}
              </div>
            )}
          </Card>

          <Card>
            <h2 className="text-lg font-semibold">What credits pay for</h2>
            <table className="mt-3 w-full text-left text-sm">
              <tbody>
                {PRICE_LIST.map((p) => (
                  <tr key={p.feature} className="border-t border-divider first:border-t-0">
                    <td className="py-2 pr-4">{p.feature}</td>
                    <td className="py-2 text-fg-muted">{p.price}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </Card>

          <Card>
            <h2 className="text-lg font-semibold">History</h2>
            {credits.ledger.length === 0 ? (
              <p className="mt-3 text-sm text-fg-muted">Nothing yet.</p>
            ) : (
              <table className="mt-3 w-full text-left text-sm tabular-nums" data-testid="credits-ledger">
                <thead className="text-fg-subtle">
                  <tr>
                    <th className="py-2 pr-4 font-medium">Date</th>
                    <th className="py-2 pr-4 font-medium">What</th>
                    <th className="py-2 pr-4 font-medium">Bucket</th>
                    <th className="py-2 text-right font-medium">Credits</th>
                  </tr>
                </thead>
                <tbody>
                  {credits.ledger.map((row) => (
                    <tr key={row.id} className="border-t border-divider">
                      <td className="py-2 pr-4 text-fg-muted">{formatDate(row.createdAt)}</td>
                      <td className="py-2 pr-4">{ledgerLabel(row)}</td>
                      <td className="py-2 pr-4 text-fg-muted">{row.bucket === "monthly" ? "Monthly" : "Purchased"}</td>
                      <td className="py-2 text-right">{`${row.delta > 0 ? "+" : ""}${n(row.delta)}`}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </Card>
        </div>
      )}
    </Shell>
  );
}
