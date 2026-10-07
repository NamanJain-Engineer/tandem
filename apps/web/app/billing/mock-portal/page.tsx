"use client";

import { useState } from "react";
import { Button } from "@tandem/ui";
import { Card, Notice, Shell } from "../../../components/Shell";
import { api, ApiFailure } from "../../../lib/api";

export default function MockPortalPage() {
  const [error, setError] = useState<string | null>(null);

  async function act(action: "cancel_at_period_end" | "resume" | "cancel_now") {
    try {
      await api("/v1/billing/mock/portal", { method: "POST", body: { action } });
      window.location.assign("/billing");
    } catch (err) {
      setError(err instanceof ApiFailure ? err.message : "That didn't work.");
    }
  }

  return (
    <Shell title="Billing portal (mock)">
      <Card>
        <p className="text-sm text-fg-muted">Development stand-in for the payment provider's portal.</p>
        <div className="mt-4 flex flex-wrap gap-3">
          <Button variant="secondary" onClick={() => act("cancel_at_period_end")}>
            Cancel at period end
          </Button>
          <Button variant="secondary" onClick={() => act("resume")}>
            Resume subscription
          </Button>
          <Button variant="danger" onClick={() => act("cancel_now")}>
            Cancel now
          </Button>
        </div>
        {error && <Notice tone="error">{error}</Notice>}
      </Card>
    </Shell>
  );
}
