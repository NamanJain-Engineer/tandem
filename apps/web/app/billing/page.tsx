"use client";

import { useState } from "react";
import { Button } from "@tandem/ui";
import { Card, Notice, Shell } from "../../components/Shell";
import { api, ApiFailure } from "../../lib/api";
import { planLine } from "../../lib/plan";
import { useMe } from "../../lib/useMe";

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
              <p className="text-sm text-fg-muted" data-testid="nothing-to-buy">
                There&apos;s nothing to buy: everything that runs on your computer works without an account.
                Hosted features come with Vibezeo accounts.
              </p>
            </Card>
          )}
          {failure && <Notice tone="error">{failure}</Notice>}
        </div>
      )}
    </Shell>
  );
}
