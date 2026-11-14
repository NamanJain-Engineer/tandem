import type { ReactNode } from "react";
import { Mail } from "lucide-react";
import type { IpcError } from "@tandem/protocol";
import { Button, Callout, Spinner, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useSession } from "../../stores/session";

async function copy(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    /* clipboard can be unavailable */
  }
}

/** UI-03 sign-in (F-BE-02): system browser + PKCE, or continue offline. */
export function SignIn() {
  const auth = useSession((s) => s.auth);
  const { show } = useToast();
  const start = () =>
    invoke("auth_sign_in").catch((e: IpcError) => show({ tone: "error", title: e.message }));
  const pending = auth?.pending ?? null;
  const error = auth?.error ?? null;
  if (auth && !auth.hosted) return <EarlyAccessWelcome />;
  return (
    <Welcome>
      <section aria-labelledby="signin-title" className="flex w-full flex-col gap-5">
        <WelcomeHero />

        {error && (
          <Callout
            tone="error"
            title={error.message}
            actions={
              <>
                <Button size="sm" variant="primary" onClick={() => void start()}>
                  {t("signin.tryAgain")}
                </Button>
                <Button
                  size="sm"
                  onClick={() => {
                    void copy(error.detail);
                    show({ tone: "success", title: t("signin.detailsCopied") });
                  }}
                >
                  {t("signin.copyDetails")}
                </Button>
              </>
            }
          />
        )}

        {pending ? (
          <div
            role="status"
            className="flex flex-col items-start gap-3 rounded-md border border-border bg-surface-3 p-4"
          >
            <p className="inline-flex items-center gap-2 text-base text-fg">
              <Spinner />
              {t("signin.waiting")}
            </p>
            <div className="flex gap-4">
              <Button
                size="sm"
                variant="link"
                onClick={() => invoke("auth_cancel", { opId: pending.opId }).catch(() => undefined)}
              >
                {t("signin.cancel")}
              </Button>
              <Button
                size="sm"
                variant="link"
                onClick={() => {
                  void copy(pending.url);
                  show({ title: t("signin.linkCopied") });
                }}
              >
                {t("signin.copyLink")}
              </Button>
            </div>
          </div>
        ) : (
          <div className="flex flex-col gap-2">
            <Button size="lg" variant="primary" fullWidth onClick={() => void start()}>
              {t("signin.google")}
            </Button>
            <Button size="lg" fullWidth iconLeft={<Mail size={16} />} onClick={() => void start()}>
              {t("signin.email")}
            </Button>
          </div>
        )}

        <div className="flex items-center gap-3 text-sm text-fg-subtle">
          <span className="h-px flex-1 bg-divider" />
          {t("signin.or")}
          <span className="h-px flex-1 bg-divider" />
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button onClick={() => invoke("auth_continue_offline").catch(() => undefined)}>
            {t("signin.offline")}
          </Button>
          <span className="text-sm text-fg-subtle">{t("signin.offlineNote")}</span>
        </div>
      </section>
    </Welcome>
  );
}

/** UI-03 in an early-access build: no accounts yet, and everything local runs
 * free (D-105, D-869). */
function EarlyAccessWelcome() {
  return (
    <Welcome>
      <section aria-labelledby="signin-title" className="flex w-full flex-col items-center gap-8">
        <WelcomeHero />
        <div className="flex w-full max-w-[320px] flex-col items-center gap-3">
          <Button
            size="lg"
            variant="primary"
            fullWidth
            onClick={() => invoke("auth_continue_offline").catch(() => undefined)}
          >
            {t("signin.earlyAccess.start")}
          </Button>
          <span className="text-sm text-fg-subtle" data-testid="signin-free">
            {t("signin.earlyAccess.note")}
          </span>
        </div>
      </section>
    </Welcome>
  );
}

/** The first run's first frame (F-SH-08 behaviour 1, DESIGN §5.3.1): the
 * content centred on the window's ground, and the terms at the foot. */
function Welcome({ children }: { children: ReactNode }) {
  return (
    <main id="main" className="flex h-full flex-col items-center overflow-auto bg-bg px-6">
      <div className="flex w-full max-w-[440px] flex-1 flex-col justify-center py-12">{children}</div>
      <p className="pb-6 text-xs text-fg-subtle">{t("signin.terms")}</p>
    </main>
  );
}

/** The two rings at 96 px on the brand's bloom, drawn in once, over the
 * title and one line. */
function WelcomeHero() {
  return (
    <div className="relative flex flex-col items-center gap-6 text-center">
      <div
        aria-hidden="true"
        data-testid="welcome-bloom"
        className="tn-welcome-bloom pointer-events-none absolute left-1/2 top-12 h-[360px] w-[720px] -translate-x-1/2 -translate-y-1/2"
      />
      <svg
        width={96}
        height={96}
        viewBox="0 0 28 28"
        fill="none"
        aria-hidden="true"
        data-testid="welcome-rings"
        className="relative"
      >
        <circle
          className="tn-ring-draw"
          pathLength={100}
          cx="10.5"
          cy="14"
          r="7"
          stroke="var(--tn-accent)"
          strokeWidth="1.4"
          transform="rotate(-90 10.5 14)"
        />
        <circle
          className="tn-ring-draw"
          pathLength={100}
          cx="17.5"
          cy="14"
          r="7"
          stroke="var(--tn-accent)"
          strokeWidth="1.4"
          transform="rotate(90 17.5 14)"
        />
      </svg>
      <div className="relative flex flex-col gap-2">
        <h1 id="signin-title" className="text-3xl font-semibold tracking-tight text-balance text-fg">
          {t("signin.title")}
        </h1>
        <p className="text-md text-pretty text-fg-muted">{t("signin.subtitle")}</p>
      </div>
    </div>
  );
}
