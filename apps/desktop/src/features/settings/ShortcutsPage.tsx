// Settings › Shortcuts (UI-113, F-SH-09 P2): every registry command with the
// keys in effect, a recorder to change them, and the defaults a click away.
// Changes go to the `shortcut_overrides` setting; the dispatcher, the palette
// and the cheatsheet read it from there.
import { useMemo, useState } from "react";
import { Keyboard, RotateCcw, Search } from "lucide-react";
import { Button, ConfirmDialog, HotkeyRecorder, IconButton, Input, Kbd, SettingsGroup, Spinner, type RecordedKeys } from "@tandem/ui";
import { t } from "../../i18n";
import { useSettings } from "../../stores/settings";
import { useUi } from "../../stores/ui";
import { commandTitle, type ShortcutOverrides } from "../commands/commands";
import { keysOf, recordShortcut } from "../commands/keys";
import { conflictFor, groupRows, keysLabel, shortcutRows, withKeys, type ShortcutRow } from "../commands/shortcutRows";
import { useBoot } from "../shell/useBoot";

type Notice = { id: string; kind: "conflict"; keys: string; other: string } | { id: string; kind: "system" | "modifier" | "key" | "failed" };

const NONE: ShortcutOverrides = {};

export function ShortcutsPage() {
  const platform = useBoot((s) => s.boot?.platform ?? "linux");
  const mac = platform === "macos";
  const loaded = useSettings((s) => s.loaded);
  const overrides = useSettings((s) => (s.values.shortcut_overrides as ShortcutOverrides | undefined) ?? NONE);
  const terminalFirst = useSettings((s) => (s.values.term_shortcut_priority ?? "terminal") === "terminal");
  const [query, setQuery] = useState("");
  const [notice, setNotice] = useState<Notice | null>(null);
  const [announce, setAnnounce] = useState("");
  const [confirmReset, setConfirmReset] = useState(false);
  const rows = useMemo(() => shortcutRows(mac, overrides), [mac, overrides]);

  const apply = async (id: string, next: ShortcutOverrides) => {
    setNotice(null);
    if (!(await useSettings.getState().set("shortcut_overrides", next))) {
      setNotice({ id, kind: "failed" });
      return;
    }
    const keys = shortcutRows(mac, next).find((r) => r.id === id)?.keys;
    const action = commandTitle(id);
    setAnnounce(keys ? t("shortcuts.saved", { action, keys: keysLabel(keys, mac) }) : t("shortcuts.cleared", { action }));
  };

  const resetAll = async () => {
    setNotice(null);
    if (await useSettings.getState().set("shortcut_overrides", {})) setAnnounce(t("shortcuts.allReset"));
    else setNotice({ id: "", kind: "failed" });
  };

  const onRecord = (id: string, pressed: RecordedKeys) => {
    const r = recordShortcut(pressed, platform);
    const other = r.ok ? conflictFor(id, r.keys, mac, overrides) : null;
    if (!r.ok) setNotice({ id, kind: r.reason });
    else if (other) setNotice({ id, kind: "conflict", keys: r.keys, other });
    else void apply(id, withKeys(overrides, id, r.keys, mac));
  };

  /** The edited command takes the keys; the other one gets its old keys. */
  const swap = (n: Extract<Notice, { kind: "conflict" }>) => {
    const old = rows.find((r) => r.id === n.id)?.keys ?? "";
    void apply(n.id, withKeys(withKeys(overrides, n.id, n.keys, mac), n.other, old, mac));
  };

  if (!loaded) {
    return (
      <div className="flex justify-center py-10">
        <Spinner label={t("shortcuts.loading")} />
      </div>
    );
  }

  const groups = groupRows(rows, query, mac);
  const message = (n: Notice) =>
    n.kind === "conflict"
      ? t("shortcuts.conflict", { action: commandTitle(n.other) })
      : n.kind === "modifier"
        ? t(mac ? "shortcuts.modifier.mac" : "shortcuts.modifier.pc")
        : t(n.kind === "system" ? "shortcuts.system" : n.kind === "key" ? "shortcuts.key" : "shortcuts.saveFailed");

  const row = (r: ShortcutRow) => {
    const terminalNote =
      !mac && terminalFirst && r.keys !== null && r.terminal !== r.keys
        ? r.terminal
          ? t("shortcuts.inTerminal", { keys: keysLabel(r.terminal, mac) })
          : t("shortcuts.toShell")
        : null;
    const mine = notice?.id === r.id ? notice : null;
    return (
      <div key={r.id} data-testid={`shortcut-${r.id}`} className="flex flex-col gap-1 border-b border-divider px-3 py-2 last:border-b-0">
        <div className="flex min-h-8 items-center gap-3">
          <span className="min-w-0 flex-1 truncate text-base text-fg">{r.title}</span>
          <span data-testid="row-keys" className="shrink-0">
            {r.keys ? <Kbd keys={keysOf(r.keys)} /> : <span className="text-sm text-fg-subtle">{t("shortcuts.none")}</span>}
          </span>
          <HotkeyRecorder
            label={t("shortcuts.recordFor", { action: r.title })}
            idleText={t("shortcuts.record")}
            recordingText={t("shortcuts.recording")}
            onRecord={(pressed) => onRecord(r.id, pressed)}
            onClear={() => void apply(r.id, withKeys(overrides, r.id, "", mac))}
          />
          {r.rebound ? (
            <IconButton
              size="sm"
              icon={<RotateCcw size={13} />}
              label={t("shortcuts.reset", { action: r.title })}
              onClick={() => {
                const next = { ...overrides };
                delete next[r.id];
                void apply(r.id, next);
              }}
            />
          ) : (
            <span className="w-7 shrink-0" aria-hidden="true" />
          )}
        </div>
        {terminalNote && <p className="text-sm text-fg-subtle">{terminalNote}</p>}
        {mine && (
          <div role="alert" className="flex flex-wrap items-center gap-2 text-sm text-danger">
            {message(mine)}
            {mine.kind === "conflict" && (
              <>
                <Button size="sm" variant="secondary" onClick={() => swap(mine)}>
                  {t("shortcuts.swap")}
                </Button>
                <Button size="sm" variant="ghost" onClick={() => setNotice(null)}>
                  {t("common.cancel")}
                </Button>
              </>
            )}
          </div>
        )}
      </div>
    );
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-2">
        <Input
          type="search"
          label={t("shortcuts.search")}
          hideLabel
          placeholder={t("shortcuts.search")}
          prefix={<Search size={14} aria-hidden="true" />}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          className="min-w-[220px] flex-1"
        />
        <Button size="sm" variant="ghost" iconLeft={<Keyboard size={14} />} onClick={() => useUi.getState().setCheatsheet(true)}>
          {t("shortcuts.title")}
        </Button>
        <Button size="sm" variant="secondary" disabled={Object.keys(overrides).length === 0} onClick={() => setConfirmReset(true)}>
          {t("shortcuts.resetAll")}
        </Button>
      </div>
      {notice?.id === "" && (
        <p role="alert" className="text-sm text-danger">
          {t("shortcuts.saveFailed")}
        </p>
      )}
      {groups.length === 0 ? (
        <p className="px-3 py-6 text-center text-base text-fg-muted">{t("shortcuts.noMatch", { query: query.trim() })}</p>
      ) : (
        groups.map((g) => (
          <SettingsGroup key={g.id} label={t(g.label)}>
            {g.rows.map(row)}
          </SettingsGroup>
        ))
      )}
      <p role="status" data-testid="shortcuts-announce" className="sr-only">
        {announce}
      </p>
      <ConfirmDialog
        open={confirmReset}
        onOpenChange={setConfirmReset}
        title={t("shortcuts.resetAllTitle")}
        body={t("shortcuts.resetAllBody")}
        confirmLabel={t("shortcuts.resetAllConfirm")}
        cancelLabel={t("common.cancel")}
        onConfirm={() => {
          setConfirmReset(false);
          void resetAll();
        }}
      />
    </div>
  );
}
