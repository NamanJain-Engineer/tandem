"use client";

import { Suspense, useState } from "react";
import { useSearchParams } from "next/navigation";
import { Button } from "@tandem/ui";
import { Card, Notice, Shell } from "../../../components/Shell";
import { api, ApiFailure } from "../../../lib/api";

function MockCheckout() {
  const session = useSearchParams().get("session") ?? "";
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function complete() {
    setBusy(true);
    try {
      await api("/v1/billing/mock/complete", { method: "POST", body: { sessionId: session } });
      window.location.assign("/billing/success");
    } catch (err) {
      setError(err instanceof ApiFailure ? err.message : "Checkout failed.");
      setBusy(false);
    }
  }

  return (
    <>
      <p className="text-sm text-fg-muted">Development checkout. No card is charged.</p>
      <div className="mt-4 flex gap-3">
        <Button variant="primary" loading={busy} onClick={complete}>
          Complete purchase
        </Button>
        <Button variant="ghost" onClick={() => window.location.assign("/billing")}>
          Cancel
        </Button>
      </div>
      {error && <Notice tone="error">{error}</Notice>}
    </>
  );
}

export default function MockCheckoutPage() {
  return (
    <Shell title="Checkout (mock)">
      <Card>
        <Suspense>
          <MockCheckout />
        </Suspense>
      </Card>
    </Shell>
  );
}
