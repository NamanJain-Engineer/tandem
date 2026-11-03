import { useEffect, useState } from "react";
import { Bot } from "lucide-react";
import type { CloseRequest } from "@tandem/protocol";
import { Button, Checkbox, Dialog } from "@tandem/ui";
import { t } from "../../i18n";
import { listen } from "../../ipc/events";
import { invoke } from "../../ipc/invoke";

/** E2E: whether the dialog tells the core it showed a question; off, the
 * core goes ahead without it, as with a hung window (D-2563). */
let sayingSeen = true;
export function setCloseSeen(on: boolean): void {
  sayingSeen = on;
}

/** F-SH-14: closing the window (or Quit) with agents running asks whether to
 * keep them running with the window hidden. The core asks only when the
 * setting is "ask"; dismissing keeps the window open. It's mounted on every
 * screen and says it showed each question, or the close goes ahead after 5 s
 * without it (D-2563). */
export function CloseDialog() {
  const [request, setRequest] = useState<CloseRequest | null>(null);
  const [remember, setRemember] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let stop: (() => void) | undefined;
    void listen<CloseRequest>("app:close-requested", (r) => {
      setRemember(false);
      setRequest(r);
    }).then((s) => (stop = s));
    return () => stop?.();
  }, []);

  useEffect(() => {
    if (request && sayingSeen) void invoke("app_close_seen", { id: request.id }).catch(() => undefined);
  }, [request]);

  const decide = async (keep: boolean) => {
    setBusy(true);
    await invoke("app_close_decide", { keep, remember }, { timeoutMs: 15_000 }).catch(() => undefined);
    setBusy(false);
    setRequest(null);
  };

  const n = request?.agents.length ?? 0;
  return (
    <Dialog
      open={request !== null}
      onOpenChange={(o) => !o && !busy && setRequest(null)}
      size="sm"
      title={t("close.title", { n })}
      description={t("close.body")}
      footer={
        <>
          <Button variant="ghost" disabled={busy} data-testid="close-stop" onClick={() => void decide(false)}>
            {t("close.stop")}
          </Button>
          <Button variant="primary" loading={busy} data-testid="close-keep" onClick={() => void decide(true)}>
            {t("close.keep")}
          </Button>
        </>
      }
    >
      <ul data-testid="close-agents" className="flex flex-col gap-1.5">
        {request?.agents.map((a) => (
          <li key={a.key} className="flex items-center gap-2 text-base">
            <Bot size={14} aria-hidden="true" className="shrink-0 text-fg-muted" />
            <span className="min-w-0 flex-1 truncate text-sm text-fg">
              {a.title ?? (a.key.startsWith("thread:") ? t("close.thread") : t("close.agent"))}
            </span>
          </li>
        ))}
      </ul>
      <div className="mt-3">
        <Checkbox label={t("close.remember")} checked={remember} onCheckedChange={setRemember} />
      </div>
    </Dialog>
  );
}
