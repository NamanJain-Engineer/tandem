"use client";

import { useEffect, useState } from "react";
import { Card, Notice, Shell } from "../../../components/Shell";
import { api, ApiFailure } from "../../../lib/api";

/** The emailed invite link (F-BL-06): joins the signed-in account to the org. */
export default function JoinTeamPage() {
  const [state, setState] = useState<"working" | "joined" | "failed">("working");
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    const token = new URLSearchParams(window.location.search).get("token") ?? "";
    api("/v1/orgs/invites/accept", { method: "POST", body: { token } })
      .then(() => setState("joined"))
      .catch((err: unknown) => {
        if (err instanceof ApiFailure && err.status === 401) {
          window.location.replace(`/signin?next=${encodeURIComponent(`/team/join?token=${token}`)}`);
          return;
        }
        setMessage(err instanceof ApiFailure ? err.message : "That didn't work. Try again.");
        setState("failed");
      });
  }, []);

  return (
    <Shell title="Join a team">
      {state === "working" && <Notice>Joining…</Notice>}
      {state === "failed" && <Notice tone="error">{message}</Notice>}
      {state === "joined" && (
        <Card>
          <p className="text-md" data-testid="joined">You&apos;ve joined the team.</p>
          <p className="mt-2 text-sm text-fg-muted">
            Your desktop app picks up the Team plan within seconds. <a className="text-accent underline" href="/team">Open the Team page</a>
          </p>
        </Card>
      )}
    </Shell>
  );
}
