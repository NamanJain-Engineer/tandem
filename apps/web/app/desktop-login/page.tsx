"use client";

import { Suspense, useEffect, useState } from "react";
import { useSearchParams } from "next/navigation";
import { Card, Notice, Shell } from "../../components/Shell";
import { api, ApiFailure } from "../../lib/api";

const REDIRECT = /^(http:\/\/127\.0\.0\.1:\d{2,5}\/callback|tandem(-dev)?:\/\/auth\/callback)$/;

function DesktopLogin() {
  const params = useSearchParams();
  const [state, setState] = useState<"working" | "done" | "error">("working");
  const [error, setError] = useState<string>("");

  useEffect(() => {
    const redirectUri = params.get("redirect_uri") ?? "";
    const attempt = async () => {
      if (!REDIRECT.test(redirectUri)) throw new Error("This sign-in link isn't from the Tandem app.");
      const session = await fetch("/v1/auth/get-session", { credentials: "include" }).then((r) =>
        r.ok ? r.json() : null,
      );
      if (!session?.user) {
        window.location.replace(
          `/signin?next=${encodeURIComponent(window.location.pathname + window.location.search)}`,
        );
        return;
      }
      const { code } = await api<{ code: string }>("/v1/auth/desktop/authorize", {
        method: "POST",
        body: {
          challenge: params.get("challenge"),
          state: params.get("state"),
          redirect_uri: redirectUri,
          device_fingerprint: params.get("df"),
          device_name: params.get("device_name") ?? undefined,
          platform: params.get("platform") ?? "linux",
        },
      });
      const target = new URL(redirectUri);
      target.searchParams.set("code", code);
      target.searchParams.set("state", params.get("state") ?? "");
      setState("done");
      window.location.assign(target.toString());
    };
    attempt().catch((err: unknown) => {
      setError(err instanceof ApiFailure || err instanceof Error ? err.message : "Sign-in failed.");
      setState("error");
    });
  }, [params]);

  if (state === "error") return <Notice tone="error">{error}</Notice>;
  if (state === "done") return <Notice tone="success">You can return to Tandem.</Notice>;
  return <Notice>Signing you in to the Tandem app…</Notice>;
}

export default function DesktopLoginPage() {
  return (
    <Shell title="Sign in to the Tandem app">
      <Card>
        <Suspense>
          <DesktopLogin />
        </Suspense>
      </Card>
    </Shell>
  );
}
