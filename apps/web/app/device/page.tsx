"use client";

import { useEffect, useState } from "react";
import { Button } from "@tandem/ui";
import { Card, Notice, Shell } from "../../components/Shell";
import { api, ApiFailure } from "../../lib/api";
import { useMe } from "../../lib/useMe";

type Info = { user_code: string; client: string; version: string; expires_at: string };

/** F-BE-03: approve a sign-in code a CLI or headless app shows. */
export default function DevicePage() {
  const { me, error: meError } = useMe();
  const [code, setCode] = useState("");
  const [info, setInfo] = useState<Info | null>(null);
  const [state, setState] = useState<"idle" | "approved" | "denied">("idle");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setCode(new URLSearchParams(window.location.search).get("code") ?? "");
  }, []);

  useEffect(() => {
    if (!me || code.replace(/[\s-]/g, "").length !== 8) {
      setInfo(null);
      return;
    }
    api<Info>(`/v1/auth/device/info?user_code=${encodeURIComponent(code)}`)
      .then((i) => {
        setInfo(i);
        setError(null);
      })
      .catch((err: unknown) => {
        setInfo(null);
        setError(err instanceof ApiFailure ? err.message : "Couldn't check that code.");
      });
  }, [me, code]);

  async function decide(approve: boolean) {
    try {
      await api("/v1/auth/device/approve", { method: "POST", body: { user_code: code, approve } });
      setState(approve ? "approved" : "denied");
    } catch (err) {
      setError(err instanceof ApiFailure ? err.message : "That didn't work. Try again.");
    }
  }

  return (
    <Shell title="Sign in a device" subtitle="Enter the code your terminal shows. Only approve a code you started yourself.">
      {meError && <Notice tone="error">{meError}</Notice>}
      {!me && !meError && <Notice>Loading…</Notice>}
      {me && state === "approved" && (
        <Notice tone="success">{`Approved. ${info?.client ?? "The device"} is signed in as ${me.user.email}; you can close this page.`}</Notice>
      )}
      {me && state === "denied" && <Notice>Denied. Nothing was signed in.</Notice>}
      {me && state === "idle" && (
        <Card>
          <label className="flex flex-col gap-1.5 text-sm text-fg-muted">
            Code
            <input
              name="code"
              value={code}
              onChange={(e) => setCode(e.target.value.toUpperCase())}
              autoComplete="off"
              spellCheck={false}
              placeholder="ABCD-EFGH"
              className="h-8 w-40 rounded-sm border border-border bg-surface-4 px-2.5 font-mono text-base tracking-widest text-fg outline-none focus:border-accent focus:shadow-focus"
            />
          </label>
          {info && (
            <div className="mt-4 flex flex-col gap-3">
              <p className="text-md" data-testid="device-client">
                {`${info.client} ${info.version} wants to sign in as ${me.user.email}.`}
              </p>
              <div className="flex gap-3">
                <Button variant="primary" onClick={() => void decide(true)}>
                  Approve
                </Button>
                <Button variant="secondary" onClick={() => void decide(false)}>
                  Deny
                </Button>
              </div>
            </div>
          )}
          {error && (
            <div className="mt-3">
              <Notice tone="error">{error}</Notice>
            </div>
          )}
        </Card>
      )}
    </Shell>
  );
}
