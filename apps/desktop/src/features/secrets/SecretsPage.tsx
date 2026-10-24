import { useEffect, useState } from "react";
import type { IpcError, SecretInfo } from "@tandem/protocol";
import {
  Button,
  Callout,
  ConfirmDialog,
  Dialog,
  EmptyState,
  Input,
  ListRow,
  Skeleton,
  useToast,
} from "@tandem/ui";
import { KeyRound, Plus } from "lucide-react";
import { t } from "../../i18n";
import { copyText } from "../../ipc/clipboard";
import { reference, useSecrets, validName } from "../../stores/secrets";

function updated(ms: number): string {
  return new Date(ms).toLocaleDateString(undefined, { day: "numeric", month: "short" });
}

/** Add a secret, or replace `replacing`'s value. The value never comes back. */
function SecretDialog({
  open,
  replacing,
  onOpenChange,
}: {
  open: boolean;
  replacing: string | null;
  onOpenChange: (open: boolean) => void;
}) {
  const list = useSecrets((s) => s.list);
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  // Values this short aren't masked in a run's pane (mask::MIN_LEN).
  const short = value !== "" && new TextEncoder().encode(value).length < 4;
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [confirmReplace, setConfirmReplace] = useState(false);
  useEffect(() => {
    if (open) {
      setName(replacing ?? "");
      setValue("");
      setError(null);
    }
  }, [open, replacing]);
  const trimmed = name.trim();
  const nameBad = trimmed !== "" && !validName(trimmed);
  const taken = replacing === null && list.some((s) => s.name === trimmed);
  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      await useSecrets.getState().save(trimmed, value);
      setValue("");
      onOpenChange(false);
    } catch (e) {
      setError((e as IpcError).message ?? t("secrets.saveFailed"));
    } finally {
      setSaving(false);
    }
  };
  const submit = () => (taken ? setConfirmReplace(true) : void save());
  return (
    <>
      <Dialog
        open={open}
        onOpenChange={onOpenChange}
        title={replacing ? t("secrets.replace.title", { name: replacing }) : t("secrets.add.title")}
        description={t("secrets.add.description")}
        footer={
          <>
            <Button variant="ghost" onClick={() => onOpenChange(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="primary"
              data-testid="secret-save"
              loading={saving}
              disabled={trimmed === "" || nameBad || value === ""}
              onClick={submit}
            >
              {replacing ? t("secrets.replace.save") : t("secrets.add.save")}
            </Button>
          </>
        }
      >
        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            if (trimmed !== "" && !nameBad && value !== "") submit();
          }}
        >
          <Input
            label={t("secrets.name")}
            data-testid="secret-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            disabled={replacing !== null}
            placeholder="stripe-test"
            hint={t("secrets.name.hint")}
            error={nameBad ? t("secrets.name.rules") : undefined}
            monospace
            spellCheck={false}
            autoFocus={replacing === null}
          />
          <Input
            label={t("secrets.value")}
            data-testid="secret-value"
            type="password"
            autoComplete="off"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            hint={short ? t("secrets.value.short") : t("secrets.value.hint")}
            spellCheck={false}
            autoFocus={replacing !== null}
          />
          {error && <Callout tone="error">{error}</Callout>}
        </form>
      </Dialog>
      <ConfirmDialog
        open={confirmReplace}
        onOpenChange={setConfirmReplace}
        title={t("secrets.replace.confirmTitle", { name: trimmed })}
        body={t("secrets.replace.confirmBody")}
        confirmLabel={t("secrets.replace.save")}
        onConfirm={() => void save()}
      />
    </>
  );
}

function SecretRow({ s, onReplace }: { s: SecretInfo; onReplace: (name: string) => void }) {
  const { show } = useToast();
  const [confirm, setConfirm] = useState(false);
  const ref = reference(s.name);
  return (
    <li data-testid="secret-row" data-name={s.name}>
      <ListRow
        leading={<KeyRound size={16} aria-hidden="true" className="text-fg-muted" />}
        title={s.name}
        secondary={
          <span className="flex items-center gap-2">
            <span className="font-mono" data-testid="secret-reference">
              {ref}
            </span>
            <span className="tabular-nums">{t("secrets.updated", { date: updated(s.updatedAt) })}</span>
          </span>
        }
        trailing={
          <span className="flex items-center gap-1.5">
            <Button
              size="sm"
              variant="ghost"
              data-testid="secret-copy"
              onClick={() => void copyText(ref).then((ok) => ok && show({ title: t("secrets.copied") }))}
            >
              {t("secrets.copy")}
            </Button>
            <Button size="sm" variant="ghost" data-testid="secret-replace" onClick={() => onReplace(s.name)}>
              {t("secrets.replace")}
            </Button>
            <Button size="sm" variant="ghost" data-testid="secret-delete" onClick={() => setConfirm(true)}>
              {t("secrets.delete")}
            </Button>
          </span>
        }
      />
      <ConfirmDialog
        open={confirm}
        onOpenChange={setConfirm}
        title={t("secrets.delete.title", { name: s.name })}
        body={t("secrets.delete.body")}
        confirmLabel={t("secrets.delete")}
        danger
        onConfirm={() =>
          void useSecrets
            .getState()
            .remove(s.name)
            .catch((e: IpcError) => show({ tone: "error", title: e.message }))
        }
      />
    </li>
  );
}

/** Settings › Secrets (F-TL-12): named values in the keychain, referred to
 * from a `.env` as `vibezeo-secret://name`. */
export function SecretsPage() {
  const list = useSecrets((s) => s.list);
  const state = useSecrets((s) => s.state);
  const error = useSecrets((s) => s.error);
  const [open, setOpen] = useState(false);
  const [replacing, setReplacing] = useState<string | null>(null);
  useEffect(() => {
    void useSecrets.getState().load();
  }, []);
  const add = () => {
    setReplacing(null);
    setOpen(true);
  };
  return (
    <section className="flex flex-col gap-3" data-testid="secrets-page">
      <div className="flex items-center justify-between">
        <h3 className="text-md font-semibold text-fg">{t("secrets.title")}</h3>
        {list.length > 0 && (
          <Button
            variant="primary"
            size="sm"
            iconLeft={<Plus size={14} />}
            data-testid="secret-add"
            onClick={add}
          >
            {t("secrets.add")}
          </Button>
        )}
      </div>
      {state === "idle" || state === "loading" ? (
        <div className="flex flex-col gap-2" data-testid="secrets-loading">
          <Skeleton className="h-10" />
          <Skeleton className="h-10" />
        </div>
      ) : state === "error" ? (
        <Callout
          tone="error"
          actions={
            <Button size="sm" variant="secondary" onClick={() => void useSecrets.getState().load()}>
              {t("secrets.retry")}
            </Button>
          }
        >
          <span data-testid="secrets-error">{error}</span>
        </Callout>
      ) : list.length === 0 ? (
        <EmptyState
          icon={<KeyRound size={20} />}
          title={t("secrets.empty.title")}
          body={t("secrets.empty.body")}
          primary={
            <Button variant="primary" data-testid="secret-add" onClick={add}>
              {t("secrets.add")}
            </Button>
          }
        />
      ) : (
        <ul className="flex flex-col gap-1" aria-label={t("secrets.title")}>
          {list.map((s) => (
            <SecretRow
              key={s.name}
              s={s}
              onReplace={(name) => {
                setReplacing(name);
                setOpen(true);
              }}
            />
          ))}
        </ul>
      )}
      <p className="text-sm text-fg-muted">{t("secrets.limits")}</p>
      <SecretDialog open={open} replacing={replacing} onOpenChange={setOpen} />
    </section>
  );
}
