"use client";

import { Suspense, useState, type FormEvent } from "react";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { Button } from "@tandem/ui";
import { Card, Field, Notice, Shell } from "../../components/Shell";
import { api, ApiFailure } from "../../lib/api";
import { SSO_ERRORS } from "../../lib/sso";

function SignInForm() {
  const router = useRouter();
  const params = useSearchParams();
  const next = params.get("next") ?? "/account";
  const ssoError = params.get("sso_error");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ tone: "error" | "success"; text: string } | null>(
    ssoError ? { tone: "error", text: SSO_ERRORS[ssoError] ?? SSO_ERRORS.SSO_ACCOUNT ?? "" } : null,
  );

  async function submit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const form = new FormData(e.currentTarget);
    setBusy(true);
    setMessage(null);
    try {
      await api("/v1/auth/sign-in/email", {
        method: "POST",
        body: { email: form.get("email"), password: form.get("password") },
      });
      router.push(next.startsWith("/") ? next : "/account");
    } catch (err) {
      setMessage({
        tone: "error",
        text: err instanceof ApiFailure ? err.message : "Couldn't sign in. Try again.",
      });
      setBusy(false);
    }
  }

  async function magicLink(e: React.MouseEvent<HTMLButtonElement>) {
    const form = e.currentTarget.form;
    const email = form ? new FormData(form).get("email") : null;
    if (!email) {
      setMessage({ tone: "error", text: "Enter your email first." });
      return;
    }
    try {
      await api("/v1/auth/sign-in/magic-link", { method: "POST", body: { email, callbackURL: next } });
      setMessage({ tone: "success", text: "Check your email for a sign-in link." });
    } catch (err) {
      setMessage({
        tone: "error",
        text: err instanceof ApiFailure ? err.message : "Couldn't send the link.",
      });
    }
  }

  async function sso(e: React.MouseEvent<HTMLButtonElement>) {
    const form = e.currentTarget.form;
    const email = form ? new FormData(form).get("email") : null;
    if (!email) {
      setMessage({ tone: "error", text: "Enter your work email first." });
      return;
    }
    setMessage(null);
    try {
      const { url } = await api<{ url: string }>("/v1/sso/start", {
        method: "POST",
        body: { email, callbackURL: next.startsWith("/") ? next : "/account" },
      });
      window.location.assign(url);
    } catch (err) {
      setMessage({
        tone: "error",
        text: err instanceof ApiFailure ? err.message : "Couldn't start single sign-on.",
      });
    }
  }

  return (
    <form onSubmit={submit} className="flex max-w-sm flex-col gap-4">
      <Field label="Email" name="email" type="email" autoComplete="email" />
      <Field
        label="Password"
        name="password"
        type="password"
        autoComplete="current-password"
        required={false}
      />
      {message && <Notice tone={message.tone}>{message.text}</Notice>}
      <Button type="submit" variant="primary" loading={busy}>
        Sign in
      </Button>
      <Button type="button" variant="ghost" onClick={magicLink}>
        Email me a sign-in link
      </Button>
      <Button type="button" variant="secondary" onClick={sso} data-testid="sso-sign-in">
        Sign in with SSO
      </Button>
      <p className="text-sm text-fg-muted">
        <Link className="text-accent hover:underline" href="/reset">
          Forgot password?
        </Link>{" "}
        ·{" "}
        <Link className="text-accent hover:underline" href="/signup">
          Create an account
        </Link>
      </p>
    </form>
  );
}

export default function SignInPage() {
  return (
    <Shell title="Sign in to Tandem">
      <Card>
        <Suspense>
          <SignInForm />
        </Suspense>
      </Card>
    </Shell>
  );
}
