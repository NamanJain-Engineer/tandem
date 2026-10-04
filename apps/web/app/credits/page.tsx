"use client";

import { useEffect, useState } from "react";
import { Card, Notice, Shell } from "../../components/Shell";
import { api, ApiFailure, formatDate } from "../../lib/api";

type Credits = { balance: { monthly: number; purchased: number; resetsAt: string | null } };

export default function CreditsPage() {
  const [credits, setCredits] = useState<Credits | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api<Credits>("/v1/credits")
      .then(setCredits)
      .catch((err: unknown) => {
        if (err instanceof ApiFailure && err.status === 401) window.location.replace("/signin?next=/credits");
        else setError("Couldn't load your credits.");
      });
  }, []);

  const n = (v: number) => new Intl.NumberFormat().format(v);
  return (
    <Shell
      title="Credits"
      subtitle="Credits pay for hosted features: cloud dictation, Enhance, Conductor and Auto."
    >
      {error && <Notice tone="error">{error}</Notice>}
      {!credits && !error && <Notice>Loading…</Notice>}
      {credits && (
        <Card>
          <dl className="grid grid-cols-[160px_1fr] gap-y-2 text-base tabular-nums">
            <dt className="text-fg-muted">Monthly</dt>
            <dd data-testid="credits-monthly">{n(credits.balance.monthly)}</dd>
            <dt className="text-fg-muted">Purchased</dt>
            <dd>{n(credits.balance.purchased)}</dd>
            <dt className="text-fg-muted">Resets</dt>
            <dd>{credits.balance.resetsAt ? formatDate(credits.balance.resetsAt) : "—"}</dd>
          </dl>
        </Card>
      )}
    </Shell>
  );
}
