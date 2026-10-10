import { useEffect, useState } from "react";
import { Mail as MailIcon } from "lucide-react";
import type { IpcError, Mail } from "@tandem/protocol";
import { Banner, Button, Dialog, Skeleton, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";

/** A mail's delivery state in words (UI-71). */
export function mailState(m: Mail): string {
  switch (m.state) {
    case "delivered":
      return t("thread.mail.delivered");
    case "queued":
      return m.reason === "global_pause"
        ? t("thread.mail.queuedPaused")
        : m.reason === "rate_limit"
          ? t("thread.mail.queuedRate")
          : t("thread.mail.queued");
    case "refused":
      return m.reason === "not_permitted"
        ? t("thread.mail.refusedSender", { name: m.from })
        : m.reason === "deleted"
          ? t("thread.mail.refusedDeleted", { name: m.to })
          : t("thread.mail.refused", { name: m.to });
    case "failed":
      return t("thread.mail.failed", { name: m.to });
  }
}

function MailDialog({ mailId, onClose }: { mailId: string; onClose: () => void }) {
  const { show } = useToast();
  const [mail, setMail] = useState<Mail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const load = () =>
    invoke<Mail>("mail_get", { mailId }).then(
      (m) => {
        setMail(m);
        setError(null);
      },
      (e: IpcError) => setError(e.message),
    );
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mailId]);
  const retry = async () => {
    setBusy(true);
    try {
      await invoke("mail_retry", { mailId });
      await load();
    } catch (e) {
      show({ tone: "error", title: (e as IpcError).message });
    } finally {
      setBusy(false);
    }
  };
  const canRetry = mail?.state === "failed" || (mail?.state === "queued" && mail.reason === "rate_limit");
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={mail ? t("thread.mail.title", { from: mail.from, to: mail.to }) : t("thread.mail.titleLoading")}
      closeLabel={t("common.close")}
      footer={
        canRetry ? (
          <Button variant="secondary" loading={busy} onClick={() => void retry()}>
            {t("thread.mail.retry")}
          </Button>
        ) : undefined
      }
    >
      <div className="flex flex-col gap-3 px-6 pb-2" data-testid="mail-dialog">
        {error ? (
          <Banner tone="error" className="rounded-md">
            {error}
          </Banner>
        ) : !mail ? (
          <Skeleton className="w-full" height={64} />
        ) : (
          <>
            <p className="text-sm text-fg-muted">
              {t("thread.mail.when", {
                at: new Date(mail.createdAt).toLocaleString([], { dateStyle: "medium", timeStyle: "short" }),
              })}
              <span className="text-fg-subtle">{" · "}</span>
              <span data-testid="mail-state">{mailState(mail)}</span>
            </p>
            <p className="whitespace-pre-wrap break-words text-base text-fg" data-testid="mail-body">
              {mail.body}
            </p>
          </>
        )}
      </div>
    </Dialog>
  );
}

/** UI-71 "✉ {from} → {to}: …" with Open. */
export function MailRowView({
  mailId,
  from,
  to,
  preview,
}: {
  mailId: string;
  from: string;
  to: string;
  preview: string;
}) {
  const [open, setOpen] = useState(false);
  return (
    <div
      className="flex items-center gap-2 rounded-md border border-border bg-surface-2 px-3 py-1.5 text-sm text-fg-muted"
      data-testid="mail-row"
      data-mail-id={mailId}
    >
      <MailIcon size={14} aria-hidden="true" className="shrink-0" />
      <span className="min-w-0 flex-1 truncate">{t("thread.mail.row", { from, to, preview })}</span>
      <Button size="sm" variant="ghost" onClick={() => setOpen(true)}>
        {t("thread.mail.open")}
      </Button>
      {open && <MailDialog mailId={mailId} onClose={() => setOpen(false)} />}
    </div>
  );
}
