"use client";

import { Suspense, useState, type FormEvent } from "react";
import { useSearchParams } from "next/navigation";
import { Button } from "@tandem/ui";
import { Card, Field, Notice, Shell } from "../../components/Shell";
import { api, ApiFailure } from "../../lib/api";

function ResetForm() {
  const token = useSearchParams().get("token");
  const [message, setMessage] = useState<{ tone: "error" | "success"; text: string } | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const form = new FormData(e.currentTarget);
    setBusy(true);
    try {
      if (token) {
        await api("/v1/auth/reset-password", {
          method: "POST",
          body: { token, newPassword: form.get("password") },
        });
        setMessage({ tone: "success", text: "Your password is changed. You can sign in now." });
      } else {
        await api("/v1/auth/request-password-reset", {
          method: "POST",
          body: { email: form.get("email"), redirectTo: "/reset" },
        });
        setMessage({
          tone: "success",
          text: "If that address has an account, we sent a reset link. It expires in 30 minutes.",
        });
      }
    } catch (err) {
      setMessage({
        tone: "error",
        text: err instanceof ApiFailure ? err.message : "That didn't work. Try again.",
      });
    }
    setBusy(false);
  }

  return (
    <form onSubmit={submit} className="flex max-w-sm flex-col gap-4">
      {token ? (
        <Field
          label="New password (at least 10 characters)"
          name="password"
          type="password"
          autoComplete="new-password"
        />
      ) : (
        <Field label="Email" name="email" type="email" autoComplete="email" />
      )}
      {message && <Notice tone={message.tone}>{message.text}</Notice>}
      <Button type="submit" variant="primary" loading={busy}>
        {token ? "Set password" : "Send reset link"}
      </Button>
    </form>
  );
}

export default function ResetPage() {
  return (
    <Shell title="Reset your password">
      <Card>
        <Suspense>
          <ResetForm />
        </Suspense>
      </Card>
    </Shell>
  );
}
