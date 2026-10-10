"use client";

import { Suspense, useState } from "react";
import { useSearchParams } from "next/navigation";
import { Button } from "@tandem/ui";
import { Card, Notice, Shell } from "../../../components/Shell";
import { api, ApiFailure } from "../../../lib/api";

/** Only a path on this site: the mock never sends anyone elsewhere. */
function sameOrigin(url: string | null): string | null {
  if (!url) return null;
  try {
    const u = new URL(url, window.location.origin);
    return u.origin === window.location.origin ? u.pathname + u.search : null;
  } catch {
    return null;
  }
}

function MockCheckout() {
  const params = useSearchParams();
  const session = params.get("session") ?? "";
  const next = sameOrigin(params.get("next")) ?? "/billing/success";
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function complete() {
    setBusy(true);
    try {
      await api("/v1/billing/mock/complete", { method: "POST", body: { sessionId: session } });
      window.location.assign(next);
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
