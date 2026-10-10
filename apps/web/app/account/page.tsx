"use client";

import { useState, type FormEvent } from "react";
import { Button } from "@tandem/ui";
import { Card, Field, Notice, Shell } from "../../components/Shell";
import { api, ApiFailure } from "../../lib/api";
import { useMe } from "../../lib/useMe";

export default function AccountPage() {
  const { me, error } = useMe();
  const [message, setMessage] = useState<{ tone: "error" | "success"; text: string } | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  async function changePassword(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const form = new FormData(e.currentTarget);
    try {
      await api("/v1/auth/change-password", {
        method: "POST",
        body: {
          currentPassword: form.get("current"),
          newPassword: form.get("next"),
          revokeOtherSessions: false,
        },
      });
      setMessage({ tone: "success", text: "Password changed." });
    } catch (err) {
      setMessage({
        tone: "error",
        text: err instanceof ApiFailure ? err.message : "Couldn't change the password.",
      });
    }
  }

  async function deleteAccount() {
    const typed = window.prompt(
      "Type DELETE to delete your account. This cancels your subscription now, signs you out on every device and removes your data within 30 days.",
    );
    if (typed !== "DELETE") return;
    setDeleteError(null);
    try {
      await api("/v1/me", { method: "DELETE", body: { confirm: "DELETE" } });
    } catch (err) {
      // A Team owner with members is asked to transfer it first (F-TL-05).
      setDeleteError(err instanceof ApiFailure ? err.message : "Couldn't request the deletion. Try again.");
      return;
    }
    await api("/v1/auth/sign-out", { method: "POST", body: {} }).catch(() => undefined);
    window.location.replace("/");
  }

  async function signOut() {
    await api("/v1/auth/sign-out", { method: "POST", body: {} });
    window.location.replace("/signin");
  }

  return (
    <Shell title="Account">
      {error && <Notice tone="error">{error}</Notice>}
      {!me && !error && <Notice>Loading…</Notice>}
      {me && (
        <div className="flex flex-col gap-6">
          <Card>
            <dl className="grid grid-cols-[140px_1fr] gap-y-2 text-base">
              <dt className="text-fg-muted">Email</dt>
              <dd data-testid="account-email">{me.user.email}</dd>
              <dt className="text-fg-muted">Plan</dt>
              <dd className="uppercase">{me.plan}</dd>
            </dl>
            <div className="mt-4">
              <Button variant="secondary" onClick={signOut}>
                Sign out
              </Button>
            </div>
          </Card>
          <Card>
            <h2 className="text-lg font-semibold">Change password</h2>
            <form onSubmit={changePassword} className="mt-4 flex max-w-sm flex-col gap-4">
              <Field
                label="Current password"
                name="current"
                type="password"
                autoComplete="current-password"
              />
              <Field label="New password" name="next" type="password" autoComplete="new-password" />
              {message && <Notice tone={message.tone}>{message.text}</Notice>}
              <Button type="submit" variant="primary">
                Change password
              </Button>
            </form>
          </Card>
          <Card>
            <h2 className="text-lg font-semibold">Delete account</h2>
            <p className="mt-1 text-sm text-fg-muted">
              Cancels your subscription now, signs you out on every device and deletes your account and
              personal data within 30 days.
            </p>
            {deleteError && (
              <div className="mt-3">
                <Notice tone="error">{deleteError}</Notice>
              </div>
            )}
            <div className="mt-4">
              <Button variant="danger" onClick={deleteAccount}>
                Delete account…
              </Button>
            </div>
          </Card>
        </div>
      )}
    </Shell>
  );
}
