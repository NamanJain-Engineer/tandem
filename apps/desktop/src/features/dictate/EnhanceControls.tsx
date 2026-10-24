import { useEffect, useState } from "react";
import { Button, ConfirmDialog, Input, Sheet, Switch, Textarea, Tooltip } from "@tandem/ui";
import { t } from "../../i18n";
import { useSession } from "../../stores/session";
import { useSetting, useSettings } from "../../stores/settings";

/** The formatting instructions' limit (F-DI-10). */
export const FORMATTING_MAX = 2_000;

type Key =
  | "dictate_quick_actions"
  | "dictate_auto_enhance"
  | "dictate_formatting"
  | "dictate_formatting_apps"
  | "dictate_enhance_disclosed";
const save = (key: Key, value: unknown) => useSettings.getState().set(key, value);

/** Enhance is hosted: it comes with Vibezeo accounts (D-2903). */
function useHosted(): boolean {
  return useSession((s) => s.entitlement?.limits.enhance ?? false);
}

/** Shows F-DI-10's disclosure once, before the first text would be sent. */
function useDisclosure(): { ask: (then: () => void) => void; dialog: React.ReactNode } {
  const disclosed = useSetting<boolean>("dictate_enhance_disclosed", false);
  const [pending, setPending] = useState<(() => void) | null>(null);
  return {
    ask: (then) => (disclosed ? then() : setPending(() => then)),
    dialog: (
      <ConfirmDialog
        open={pending !== null}
        onOpenChange={(o) => !o && setPending(null)}
        title={t("dictate.enhance.disclosureTitle")}
        body={t("dictate.enhance.disclosure")}
        confirmLabel={t("dictate.enhance.continue")}
        onConfirm={() => {
          void save("dictate_enhance_disclosed", true);
          pending?.();
          setPending(null);
        }}
      />
    ),
  };
}

/** With no account the controls show disabled, with the reason (UI-62). */
function HostedGate({ children }: { children: React.ReactElement }) {
  const pro = useHosted();
  return pro ? (
    children
  ) : (
    <Tooltip label={t("dictate.enhance.needsPro")}>
      <span className="inline-flex">{children}</span>
    </Tooltip>
  );
}

/** "Show Enhance and Polish after each take". */
export function QuickActionsControl({ label }: { label: string }) {
  const value = useSetting<boolean>("dictate_quick_actions", true);
  const pro = useHosted();
  return (
    <HostedGate>
      <Switch
        aria-label={label}
        checked={pro && value}
        disabled={!pro}
        onCheckedChange={(v) => void save("dictate_quick_actions", v)}
      />
    </HostedGate>
  );
}

/** "Enhance every take automatically": the disclosure comes first. */
export function AutoEnhanceControl({ label }: { label: string }) {
  const value = useSetting<boolean>("dictate_auto_enhance", false);
  const pro = useHosted();
  const { ask, dialog } = useDisclosure();
  return (
    <>
      <HostedGate>
        <span data-testid="dictate-auto-enhance" className="inline-flex">
          <Switch
            aria-label={label}
            checked={pro && value}
            disabled={!pro}
            onCheckedChange={(v) =>
              v
                ? ask(() => void save("dictate_auto_enhance", true))
                : void save("dictate_auto_enhance", false)
            }
          />
        </span>
      </HostedGate>
      {dialog}
    </>
  );
}

/** "Formatting instructions" (≤2,000 characters) with "Per app…" overrides. */
export function FormattingControl({ label }: { label: string }) {
  const saved = useSetting<string | null>("dictate_formatting", null);
  const pro = useHosted();
  const [text, setText] = useState(saved ?? "");
  const [perApp, setPerApp] = useState(false);
  const { ask, dialog } = useDisclosure();
  useEffect(() => setText(saved ?? ""), [saved]);
  const commit = () => {
    const next = text.trim() ? text.slice(0, FORMATTING_MAX) : null;
    if (next === (saved ?? null)) return;
    if (next) ask(() => void save("dictate_formatting", next));
    else void save("dictate_formatting", null);
  };
  return (
    <HostedGate>
      <div className="flex w-80 flex-col items-end gap-1" data-testid="dictate-formatting">
        <Textarea
          label={label}
          hideLabel
          value={text}
          disabled={!pro}
          minRows={2}
          maxRows={6}
          placeholder={t("dictate.enhance.formattingPlaceholder")}
          onChange={(e) => setText(e.target.value.slice(0, FORMATTING_MAX))}
          onBlur={commit}
          className="w-full"
        />
        <span className="flex w-full items-center justify-between text-xs text-fg-muted">
          <span data-testid="dictate-formatting-count">
            {t("dictate.enhance.count", {
              n: text.length.toLocaleString(),
              max: FORMATTING_MAX.toLocaleString(),
            })}
          </span>
          <Button
            size="sm"
            variant="ghost"
            disabled={!pro}
            onClick={() => setPerApp(true)}
            data-testid="dictate-formatting-apps"
          >
            {t("dictate.enhance.perApp")}
          </Button>
        </span>
        {perApp && <PerAppSheet onClose={() => setPerApp(false)} ask={ask} />}
        {dialog}
      </div>
    </HostedGate>
  );
}

type Row = { app: string; text: string };

/** The app-profile overrides: an app id and its own instructions. */
function PerAppSheet({ onClose, ask }: { onClose: () => void; ask: (then: () => void) => void }) {
  const saved = useSetting<Record<string, string>>("dictate_formatting_apps", {});
  const [rows, setRows] = useState<Row[]>(() => Object.entries(saved).map(([app, text]) => ({ app, text })));
  const set = (i: number, patch: Partial<Row>) =>
    setRows((r) => r.map((row, j) => (j === i ? { ...row, ...patch } : row)));
  const dirty =
    JSON.stringify(rows) !== JSON.stringify(Object.entries(saved).map(([app, text]) => ({ app, text })));
  const commit = () => {
    const map: Record<string, string> = {};
    for (const r of rows)
      if (r.app.trim() && r.text.trim()) map[r.app.trim()] = r.text.slice(0, FORMATTING_MAX);
    const write = () => {
      void save("dictate_formatting_apps", map);
      onClose();
    };
    if (Object.keys(map).length > 0) ask(write);
    else write();
  };
  return (
    <Sheet
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("dictate.enhance.perAppTitle")}
      dirty={dirty}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t("dictionary.cancel")}
          </Button>
          <Button variant="primary" onClick={commit} data-testid="dictate-formatting-apps-save">
            {t("dictionary.save")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3" data-testid="dictate-formatting-apps-sheet">
        <p className="text-sm text-fg-muted">{t("dictate.enhance.perAppBody")}</p>
        {rows.length === 0 && <p className="text-sm text-fg-muted">{t("dictate.enhance.perAppEmpty")}</p>}
        {rows.map((r, i) => (
          <div key={i} className="flex flex-col gap-1.5 rounded-md border border-border p-3">
            <Input
              label={t("dictate.enhance.appId")}
              value={r.app}
              onChange={(e) => set(i, { app: e.target.value })}
            />
            <Textarea
              label={t("dictate.enhance.appInstructions")}
              value={r.text}
              minRows={2}
              onChange={(e) => set(i, { text: e.target.value.slice(0, FORMATTING_MAX) })}
            />
            <Button
              size="sm"
              variant="ghost"
              className="self-end"
              onClick={() => setRows((rs) => rs.filter((_, j) => j !== i))}
            >
              {t("dictate.enhance.removeApp")}
            </Button>
          </div>
        ))}
        <Button
          size="sm"
          variant="secondary"
          className="self-start"
          onClick={() => setRows((rs) => [...rs, { app: "", text: "" }])}
          data-testid="dictate-formatting-apps-add"
        >
          {t("dictate.enhance.addApp")}
        </Button>
      </div>
    </Sheet>
  );
}
