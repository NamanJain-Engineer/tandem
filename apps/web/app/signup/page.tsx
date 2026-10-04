"use client";

import { useState, type FormEvent } from "react";
import Link from "next/link";
import { Button } from "@tandem/ui";
import { Card, Field, Notice, Shell } from "../../components/Shell";
import { api, ApiFailure } from "../../lib/api";

export default function SignUpPage() {
  const [state, setState] = useState<"idle" | "busy" | "sent">("idle");
  const [error, setError] = useState<string | null>(null);

  async function submit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const form = new FormData(e.currentTarget);
    setState("busy");
    setError(null);
    try {
      await api("/v1/auth/sign-up/email", {
        method: "POST",
        body: {
          name: form.get("name"),
          email: form.get("email"),
          password: form.get("password"),
          callbackURL: "/verify",
        },
      });
      setState("sent");
    } catch (err) {
      setError(err instanceof ApiFailure ? err.message : "Couldn't create the account. Try again.");
      setState("idle");
    }
  }

  return (
    <Shell title="Create your Tandem account">
      <Card>
        {state === "sent" ? (
          <Notice tone="success">Check your email. We sent a link to verify your address.</Notice>
        ) : (
          <form onSubmit={submit} className="flex max-w-sm flex-col gap-4">
            <Field label="Name" name="name" autoComplete="name" />
            <Field label="Email" name="email" type="email" autoComplete="email" />
            <Field
              label="Password (at least 10 characters)"
              name="password"
              type="password"
              autoComplete="new-password"
            />
            {error && <Notice tone="error">{error}</Notice>}
            <Button type="submit" variant="primary" loading={state === "busy"}>
              Create account
            </Button>
            <p className="text-sm text-fg-muted">
              Already have an account?{" "}
              <Link className="text-accent hover:underline" href="/signin">
                Sign in
              </Link>
            </p>
          </form>
        )}
      </Card>
    </Shell>
  );
}
