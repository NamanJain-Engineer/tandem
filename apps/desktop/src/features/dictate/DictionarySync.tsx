import { useEffect, useState } from "react";
import type { IpcError, Rule, SyncConflict, SyncResult } from "@tandem/protocol";
import { Button, Dialog, Spinner, Switch, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { listen } from "../../ipc/events";
import { invoke } from "../../ipc/invoke";
import { useSession } from "../../stores/session";
import { useSetting, useSettings } from "../../stores/settings";
import { ComesWithAccounts } from "../billing/ComesWithAccounts";

function ruleText(r: Rule): string {
  if (r.deletedAt !== null) return t("dictionary.sync.deleted");
  return r.phrases.length > 0 ? `${r.phrases.join(", ")} → ${r.replacement}` : r.replacement;
}

/** One rule both sides changed: the user keeps one (F-DI-09). */
function Conflict({ c, onDone }: { c: SyncConflict; onDone: () => void }) {
  const { show } = useToast();
  const keep = (which: "mine" | "theirs") =>
    void invoke("dictionary_sync_resolve", { id: c.id, keep: which }).then(onDone, (e: IpcError) =>
      show({ tone: "error", title: e.message }),
    );
  return (
    <li className="flex flex-col gap-2 px-3 py-2" data-testid="dictionary-sync-conflict">
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-sm">
        <dt className="text-fg-muted">{t("dictionary.sync.mine")}</dt>
        <dd className="truncate text-fg">{ruleText(c.local)}</dd>
        <dt className="text-fg-muted">{t("dictionary.sync.theirs")}</dt>
        <dd className="truncate text-fg">{ruleText(c.remote)}</dd>
      </dl>
      <div className="flex gap-2">
        <Button size="sm" variant="secondary" onClick={() => keep("mine")} data-testid="dictionary-sync-keep-mine">
          {t("dictionary.sync.keepMine")}
        </Button>
        <Button size="sm" variant="secondary" onClick={() => keep("theirs")} data-testid="dictionary-sync-keep-theirs">
          {t("dictionary.sync.keepTheirs")}
        </Button>
      </div>
    </li>
  );
}

/** "Sync dictionary" (F-DI-09, D-523): hosted, so it comes with Vibezeo
 * accounts (D-2903); off until the user turns it on;
 * only the rules travel. Hidden in builds without accounts. */
export function DictionarySync({ onChanged }: { onChanged: () => void }) {
  const { show } = useToast();
  const hosted = useSession((s) => s.auth?.hosted ?? false);
  const allowed = useSession((s) => s.entitlement?.limits.dictionarySync ?? false);
  const on = useSetting<boolean>("dictate_dictionary_sync", false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<SyncResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [askOff, setAskOff] = useState(false);

  const apply = (r: SyncResult) => {
    setResult(r);
    setError(null);
    if (r.pulled > 0 || r.conflicts.length > 0) onChanged();
  };
  const syncNow = () => {
    setBusy(true);
    void invoke<SyncResult>("dictionary_sync")
      .then(apply, (e: IpcError) => setError(e.message))
      .finally(() => setBusy(false));
  };
  useEffect(() => {
    if (!hosted || !allowed || !on) return;
    syncNow();
    let off: (() => void) | undefined;
    let gone = false;
    void listen<SyncResult>("dictionary:synced", apply).then((u) => {
      if (gone) u();
      else off = u;
    });
    return () => {
      gone = true;
      off?.();
    };
    // Sync once when the sheet opens with sync on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hosted, allowed, on]);

  if (!hosted) return null;
  const set = (v: boolean) => useSettings.getState().set("dictate_dictionary_sync", v);
  const turnOff = (forget: boolean) => {
    setAskOff(false);
    void set(false);
    setResult(null);
    if (forget)
      void invoke("dictionary_sync_forget").then(
        () => show({ tone: "success", title: t("dictionary.sync.forgotten") }),
        (e: IpcError) => show({ tone: "error", title: e.message }),
      );
  };
  const resolved = (id: string) => {
    setResult((r) => (r ? { ...r, conflicts: r.conflicts.filter((c) => c.id !== id) } : r));
    onChanged();
  };

  return (
    <section className="flex flex-col gap-2 rounded-md border border-border p-3" aria-label={t("dictionary.sync.title")} data-testid="dictionary-sync">
      <div className="flex items-center gap-3">
        <div className="flex flex-1 flex-col">
          <h3 className="text-sm font-semibold text-fg">{t("dictionary.sync.title")}</h3>
          <p className="text-sm text-fg-muted">{t("dictionary.sync.help")}</p>
        </div>
        {allowed ? (
          <Switch
            checked={on}
            aria-label={t("dictionary.sync.title")}
            onCheckedChange={(v) => (v ? void set(true) : setAskOff(true))}
          />
        ) : (
          <ComesWithAccounts testId="dictionary-sync-pro" />
        )}
      </div>
      {allowed && on && (
        <div className="flex items-center gap-2 text-sm text-fg-muted" data-testid="dictionary-sync-status">
          {busy ? (
            <Spinner label={t("dictionary.sync.syncing")} />
          ) : error ? (
            <span className="text-danger">{error}</span>
          ) : result ? (
            <span>{t("dictionary.sync.synced", { when: new Date(Number(result.syncedAt)).toLocaleTimeString() })}</span>
          ) : null}
          <Button size="sm" variant="ghost" disabled={busy} onClick={syncNow} data-testid="dictionary-sync-now">
            {t("dictionary.sync.now")}
          </Button>
        </div>
      )}
      {allowed && on && result && result.conflicts.length > 0 && (
        <>
          <p className="text-sm text-fg">{t("dictionary.sync.conflicts", { n: result.conflicts.length })}</p>
          <ul className="flex flex-col divide-y divide-divider rounded-md border border-border">
            {result.conflicts.map((c) => (
              <Conflict key={c.id} c={c} onDone={() => resolved(c.id)} />
            ))}
          </ul>
        </>
      )}
      <Dialog
        open={askOff}
        onOpenChange={setAskOff}
        title={t("dictionary.sync.offTitle")}
        footer={
          <>
            <Button variant="ghost" onClick={() => setAskOff(false)}>
              {t("dictionary.cancel")}
            </Button>
            <Button variant="secondary" onClick={() => turnOff(true)} data-testid="dictionary-sync-off-delete">
              {t("dictionary.sync.offDelete")}
            </Button>
            <Button variant="primary" onClick={() => turnOff(false)} data-testid="dictionary-sync-off">
              {t("dictionary.sync.off")}
            </Button>
          </>
        }
      >
        <p className="text-sm text-fg">{t("dictionary.sync.offBody")}</p>
      </Dialog>
    </section>
  );
}
