import { Mail } from "lucide-react";
import type { IpcError } from "@tandem/protocol";
import { Button, Callout, Spinner, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useSession } from "../../stores/session";
import { LogoMark } from "../shell/LogoMark";

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
  return (
    <main id="main" className="flex h-full items-center justify-center bg-bg p-6">
      <section aria-labelledby="signin-title" className="flex w-[520px] max-w-full flex-col gap-5 rounded-lg border border-border bg-surface-2 p-8 shadow-2">
        <div className="flex items-center gap-3">
          <LogoMark size={32} />
          <div>
            <h1 id="signin-title" className="text-xl font-semibold text-fg">
              {t("signin.title")}
            </h1>
            <p className="text-base text-fg-muted">{t("signin.subtitle")}</p>
          </div>
        </div>

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
          <div role="status" className="flex flex-col items-start gap-3 rounded-md border border-border bg-surface-3 p-4">
            <p className="inline-flex items-center gap-2 text-base text-fg">
              <Spinner />
              {t("signin.waiting")}
            </p>
            <div className="flex gap-4">
              <Button size="sm" variant="link" onClick={() => invoke("auth_cancel", { opId: pending.opId }).catch(() => undefined)}>
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
          <Button onClick={() => invoke("auth_continue_offline").catch(() => undefined)}>{t("signin.offline")}</Button>
          <span className="text-sm text-fg-subtle">{t("signin.offlineNote")}</span>
        </div>
        <p className="text-sm text-fg-subtle">{t("signin.terms")}</p>
      </section>
    </main>
  );
}
