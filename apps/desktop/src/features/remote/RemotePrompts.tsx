import { useEffect, useState } from "react";
import { Button, Dialog, KeyValue, SecretInput } from "@tandem/ui";
import { t } from "../../i18n";
import { useRemote } from "../../stores/remote";
import { useSteadyBox } from "../secrets/ApprovalGuard";

/** ssh's questions, wherever a connection started: an unknown host's key
 * (F-RM-01 "unknown hosts show the fingerprint for approval") and the
 * askpass helper's password prompt. */
export function RemotePrompts() {
  const ask = useRemote((s) => s.ask);
  const [secret, setSecret] = useState("");
  useEffect(() => setSecret(""), [ask?.opId]);
  const prompt = ask?.prompt;
  // The dialog can open on its own whenever a connection starts: Trust and
  // connect waits until it has held still, uncovered, so a click aimed at
  // what was there can't trust a key (F-TL-12.12).
  const box = useSteadyBox(
    prompt?.kind === "hostkey",
    ask && prompt?.kind === "hostkey" ? `${ask.opId}\n${prompt.host}\n${prompt.fingerprint}` : null,
  );
  if (prompt?.kind === "hostkey") {
    return (
      <Dialog
        open
        dismissable={false}
        onOpenChange={(o) => !o && useRemote.getState().answerHostKey(false)}
        title={t("remote.hostkey.title", { host: prompt.host })}
        description={t("remote.hostkey.body")}
        footer={
          <span ref={box.ref} className="flex gap-2">
            <Button
              variant="ghost"
              data-testid="remote-hostkey-reject"
              onClick={() => useRemote.getState().answerHostKey(false)}
            >
              {t("remote.hostkey.reject")}
            </Button>
            <Button
              variant="primary"
              data-grant
              data-testid="remote-hostkey-accept"
              waiting={!box.steady}
              title={box.steady ? undefined : t("approval.waitingTrust")}
              onClick={() => box.check() && useRemote.getState().answerHostKey(true)}
            >
              {t("remote.hostkey.accept")}
            </Button>
          </span>
        }
      >
        <div data-testid="remote-hostkey" className="flex flex-col gap-3">
          <KeyValue
            items={[
              { key: t("remote.hostkey.host"), value: prompt.host },
              { key: t("remote.hostkey.type"), value: prompt.keyType },
              {
                key: t("remote.hostkey.fingerprint"),
                value: (
                  <span className="break-all font-mono" data-testid="remote-hostkey-fp">
                    {prompt.fingerprint}
                  </span>
                ),
              },
            ]}
          />
          <p className="text-sm text-fg-muted">{t("remote.hostkey.check")}</p>
        </div>
      </Dialog>
    );
  }
  if (prompt?.kind === "password") {
    return (
      <Dialog
        open
        onOpenChange={(o) => !o && useRemote.getState().answerPassword(null)}
        title={t("remote.password.title")}
        footer={
          <>
            <Button variant="ghost" onClick={() => useRemote.getState().answerPassword(null)}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="primary"
              data-testid="remote-password-ok"
              disabled={secret === ""}
              onClick={() => useRemote.getState().answerPassword(secret)}
            >
              {t("remote.password.ok")}
            </Button>
          </>
        }
      >
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (secret !== "") useRemote.getState().answerPassword(secret);
          }}
        >
          <SecretInput
            autoFocus
            label={prompt.label}
            data-testid="remote-password"
            value={secret}
            autoComplete="off"
            onChange={(e) => setSecret(e.target.value)}
            hint={t("remote.password.hint")}
          />
        </form>
      </Dialog>
    );
  }
  return null;
}
