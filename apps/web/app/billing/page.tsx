"use client";

import { useState } from "react";
import { Button } from "@tandem/ui";
import { PRICES } from "@tandem/protocol/plans";
import { Card, Notice, Shell } from "../../components/Shell";
import { api, ApiFailure } from "../../lib/api";
import { planLine } from "../../lib/plan";
import { useMe } from "../../lib/useMe";

const dollars = (cents: number) => `$${(cents / 100).toFixed(0)}`;

export default function BillingPage() {
  const { me, error } = useMe();
  const [busy, setBusy] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  async function go(key: string, path: string, body: unknown) {
    setBusy(key);
    setFailure(null);
    try {
      const { url } = await api<{ url: string }>(path, { method: "POST", body });
      window.location.assign(url);
    } catch (err) {
      setFailure(err instanceof ApiFailure ? err.message : "That didn't work. Try again.");
      setBusy(null);
    }
  }

  return (
    <Shell title="Billing">
      {error && <Notice tone="error">{error}</Notice>}
      {!me && !error && <Notice>Loading…</Notice>}
      {me && (
        <div className="flex flex-col gap-6">
          <Card>
            <p className="text-md" data-testid="plan-line">
              {planLine(me)}
            </p>
            {me.plan !== "free" && (
              <div className="mt-4">
                <Button
                  variant="secondary"
                  loading={busy === "portal"}
                  onClick={() => go("portal", "/v1/billing/portal", {})}
                >
                  Manage billing
                </Button>
              </div>
            )}
          </Card>
          {me.plan === "free" && (
            <Card>
              <h2 className="text-lg font-semibold">Upgrade</h2>
              <p className="mt-1 text-sm text-fg-muted">
                Pro includes unlimited panes and workspaces, routines, cloud dictation and 10,000 credits a
                month. 7-day free trial.
              </p>
              <div className="mt-4 flex flex-wrap gap-3">
                <Button
                  variant="primary"
                  loading={busy === "pro-month"}
                  onClick={() => go("pro-month", "/v1/billing/checkout", { plan: "pro", interval: "month" })}
                >
                  {`Upgrade to Pro · ${dollars(PRICES.pro.month)}/month`}
                </Button>
                <Button
                  variant="secondary"
                  loading={busy === "pro-year"}
                  onClick={() => go("pro-year", "/v1/billing/checkout", { plan: "pro", interval: "year" })}
                >
                  {`Pro yearly · ${dollars(PRICES.pro.year)}/year`}
                </Button>
                <Button
                  variant="secondary"
                  loading={busy === "team"}
                  onClick={() =>
                    go("team", "/v1/billing/checkout", { plan: "team", interval: "month", seats: 2 })
                  }
                >
                  {`Team · ${dollars(PRICES.team.month)}/seat/month`}
                </Button>
              </div>
            </Card>
          )}
          {failure && <Notice tone="error">{failure}</Notice>}
        </div>
      )}
    </Shell>
  );
}
