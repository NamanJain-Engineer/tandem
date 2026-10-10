import { useEffect, useState } from "react";
import type { DevicesView, HotkeyStatus, IpcError } from "@tandem/protocol";
import {
  Badge,
  Button,
  Callout,
  HotkeyRecorder,
  Kbd,
  Select,
  useToast,
  type BadgeVariant,
} from "@tandem/ui";
import { t, type MessageKey } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { copyText } from "../../ipc/clipboard";
import { useDictate } from "../../stores/dictate";
import { useSetting, useSettings } from "../../stores/settings";
import { bindingFrom } from "./binding";
import { DictateSetup } from "./DictateSetup";
import { DictionarySheet } from "./DictionarySheet";
import { HistorySheet } from "./HistorySheet";
import { ModelManager } from "./ModelManager";
import { formatBytes, formatDuration } from "./format";

const CHIP: Record<HotkeyStatus["kind"], { key: MessageKey; variant: BadgeVariant }> = {
  active: { key: "dictate.chip.active", variant: "done" },
  needs_permission: { key: "dictate.chip.needsPermission", variant: "waiting" },
  unavailable: { key: "dictate.chip.unavailable", variant: "error" },
  paused: { key: "dictate.chip.paused", variant: "waiting" },
  starting: { key: "dictate.chip.starting", variant: "idle" },
  not_running: { key: "dictate.chip.notRunning", variant: "idle" },
};

export function HotkeyChip({ status }: { status: HotkeyStatus }) {
  const chip = CHIP[status.kind];
  return (
    <span data-testid="dictate-hotkey-chip" data-kind={status.kind}>
      <Badge variant={chip.variant}>{t(chip.key)}</Badge>
    </span>
  );
}

/** A shortcut row: the key, the recorder and its validation error. */
export function ShortcutField({ mode, label }: { mode: "hold" | "toggle"; label: string }) {
  const status = useDictate((s) => s.status);
  const [error, setError] = useState<string | null>(null);
  const current = mode === "hold" ? status?.hold : status?.toggle;
  const save = (binding: string | null) =>
    invoke<HotkeyStatus>("dictate_set_binding", { binding, mode }).then(
      () => {
        setError(null);
        void useDictate.getState().loadStatus();
      },
      (e: IpcError) => setError(e.message),
    );
  return (
    <div className="flex flex-col items-end gap-1" data-testid={`dictate-shortcut-${mode}`}>
      <div className="flex items-center gap-2">
        {current ? <Kbd keys={current.label.split("+")} /> : <span className="text-sm text-fg-muted">{t("dictate.shortcut.notSet")}</span>}
        <HotkeyRecorder
          label={label}
          idleText={current ? t("dictate.shortcut.change") : t("dictate.shortcut.set")}
          recordingText={t("dictate.shortcut.recording")}
          modifierOnly
          mouseButtons
          onRecord={(keys) => {
            const b = bindingFrom(keys);
            if (b) void save(b);
            else setError(t("dictate.shortcut.unknownKey"));
          }}
          onClear={mode === "toggle" ? () => void save(null) : undefined}
        />
      </div>
      {error && (
        <span role="alert" className="text-sm text-[var(--tn-status-error-text,var(--tn-danger))]" data-testid={`dictate-shortcut-error-${mode}`}>
          {error}
        </span>
      )}
    </div>
  );
}

/** The live mic meter while a take records (and the device's name). */
export function MicMeter() {
  const level = useDictate((s) => s.level);
  return (
    <span className="inline-flex h-3 w-16 items-end gap-0.5" aria-hidden="true">
      {[0.15, 0.35, 0.55, 0.75].map((th) => (
        <span key={th} className={level > th ? "h-full w-1/4 rounded-xs bg-accent" : "h-1/3 w-1/4 rounded-xs bg-border"} />
      ))}
    </span>
  );
}

export function MicSelect({ k, label, none }: { k: "dictate_mic" | "dictate_backup_mic"; label: string; none: string }) {
  const value = useSetting<string | null>(k, null);
  const [view, setView] = useState<DevicesView | null>(null);
  useEffect(() => {
    void invoke<DevicesView>("dictate_devices").then(setView, () => setView({ devices: [], chosen: null }));
  }, []);
  const def = view?.devices.find((d) => d.isDefault);
  const options = [
    {
      value: "",
      label:
        k === "dictate_mic"
          ? def
            ? t("dictate.mic.systemDefaultNamed", { name: def.name })
            : t("dictate.mic.systemDefault")
          : none,
    },
    ...(view?.devices ?? []).map((d) => ({
      value: d.id,
      label: d.name,
      description: d.bluetooth ? t("dictate.mic.bluetooth") : d.loopback ? t("dictate.mic.loopback") : undefined,
    })),
  ];
  return (
    <div className="flex items-center gap-2">
      <Select
        ariaLabel={label}
        size="sm"
        options={options}
        value={value ?? ""}
        onValueChange={(v) => void useSettings.getState().set(k, v === "" ? null : v)}
        className="w-64"
      />
      {k === "dictate_mic" && <MicMeter />}
    </div>
  );
}

/** UI-62's top card: ready (or "Finish Dictate setup"), the key, the engine
 * chip, the four stats, Try it and History & stats. */
export function DictateHomeCard({ onTry, onHistory }: { onTry: () => void; onHistory: () => void }) {
  const status = useDictate((s) => s.status);
  const stats = useDictate((s) => s.stats);
  useEffect(() => {
    void useDictate.getState().loadStats();
  }, []);
  if (!status) return null;
  const ready = status.verified && (status.model !== null || status.engine === "cloud");
  return (
    <section
      aria-label={t("dictate.home.label")}
      className="flex flex-col gap-3 rounded-lg border border-border bg-surface-2 p-4"
      data-testid="dictate-home"
      data-ready={ready ? "true" : "false"}
    >
      <div className="flex flex-wrap items-center gap-2">
        <h2 className="text-lg font-semibold text-fg">{ready ? t("dictate.home.ready") : t("dictate.home.finish")}</h2>
        <span className="text-sm text-fg-muted">{t("dictate.home.hold", { key: status.hold.label })}</span>
        <Badge variant="idle">
          {status.engine === "cloud"
            ? t("dictate.home.cloudChip")
            : status.model
              ? t("dictate.home.engineChip", { model: status.model.name })
              : t("dictate.home.noModel")}
        </Badge>
        <span className="flex-1" />
        <Button size="sm" variant={ready ? "secondary" : "primary"} onClick={onTry} data-testid="dictate-try">
          {ready ? t("dictate.home.try") : t("dictate.home.setUp")}
        </Button>
      </div>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-sm text-fg-muted" data-testid="dictate-stats">
        <span>{t("dictate.stats.words", { n: stats?.words ?? 0 })}</span>
        <span>{formatDuration(stats?.speakingMs ?? 0)}</span>
        <span>{t("dictate.stats.takes", { n: stats?.takes ?? 0 })}</span>
        <span>{t("dictate.stats.wpm", { n: stats?.wpm ?? 0 })}</span>
        <span className="flex-1" />
        <Button size="sm" variant="ghost" onClick={onHistory} data-testid="dictate-open-history">
          {t("dictate.home.history")}
        </Button>
      </div>
    </section>
  );
}

/** Settings › Dictate (UI-62/108): the parts that aren't plain setting rows. */
export function DictateExtras() {
  const status = useDictate((s) => s.status);
  const models = useDictate((s) => s.models);
  const { show } = useToast();
  const [sheet, setSheet] = useState<"models" | "dictionary" | "history" | "setup" | null>(null);
  const rules = useRuleCount(sheet);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    void useDictate
      .getState()
      .loadStatus()
      .then(() => setFailed(useDictate.getState().status === null));
    void useDictate.getState().loadModels();
  }, []);
  if (!status && failed)
    return (
      <p role="alert" className="text-sm text-fg-muted" data-testid="dictate-unavailable">
        {t("dictate.unavailable")}
      </p>
    );
  if (!status)
    return (
      <p className="text-sm text-fg-muted" data-testid="dictate-loading">
        {t("dictate.loading")}
      </p>
    );
  const active = models?.models.find((m) => m.id === models.active);
  return (
    <div className="flex flex-col gap-4" data-testid="dictate-extras">
      <DictateHomeCard onTry={() => setSheet("setup")} onHistory={() => setSheet("history")} />
      {status.missingTools && (
        <Callout tone="warning" title={t("dictate.tools.title", { tools: status.missingTools.tools.join(", ") })}>
          <div className="flex flex-col gap-1 font-mono text-sm" data-testid="dictate-missing-tools">
            <span>{status.missingTools.apt}</span>
            <span>{status.missingTools.dnf}</span>
            <span>{status.missingTools.pacman}</span>
          </div>
        </Callout>
      )}
      {status.deepLinkCommand && (
        <Callout tone="info" title={t("dictate.wayland.title")}>
          <div className="flex items-center gap-2">
            <code className="font-mono text-sm" data-testid="dictate-deeplink-command">
              {status.deepLinkCommand}
            </code>
            <Button
              size="sm"
              variant="ghost"
              onClick={() =>
                void copyText(status.deepLinkCommand ?? "").then(() => show({ tone: "success", title: t("dictate.wayland.copied") }))
              }
            >
              {t("dictate.wayland.copy")}
            </Button>
          </div>
        </Callout>
      )}
      <dl className="grid grid-cols-[1fr_auto] items-center gap-x-4 gap-y-3 rounded-lg border border-border p-4">
        <dt className="text-base text-fg">{t("dictate.model.label")}</dt>
        <dd className="flex items-center gap-2 text-sm text-fg-muted" data-testid="dictate-model-row">
          {active ? t("dictate.model.active", { name: active.name, size: formatBytes(Number(active.bytes)) }) : t("dictate.model.none")}
          <Button size="sm" variant="secondary" onClick={() => setSheet("models")} data-testid="dictate-manage-models">
            {t("dictate.model.manage")}
          </Button>
        </dd>
        <dt className="text-base text-fg">{t("dictate.shortcut.status")}</dt>
        <dd>
          <HotkeyChip status={status.hotkey} />
        </dd>
        <dt className="text-base text-fg">{t("dictate.dictionary.row", { n: rules })}</dt>
        <dd>
          <Button size="sm" variant="secondary" onClick={() => setSheet("dictionary")} data-testid="dictate-open-dictionary">
            {t("dictate.open")}
          </Button>
        </dd>
      </dl>
      <ModelManager open={sheet === "models"} onClose={() => setSheet(null)} />
      <DictionarySheet open={sheet === "dictionary"} onClose={() => setSheet(null)} />
      <HistorySheet open={sheet === "history"} onClose={() => setSheet(null)} />
      <DictateSetup open={sheet === "setup"} onClose={() => setSheet(null)} />
    </div>
  );
}

function useRuleCount(rev: unknown): number {
  const [n, setN] = useState(0);
  useEffect(() => {
    void invoke<unknown[] | null>("dictionary_list").then((r) => setN(r?.length ?? 0), () => undefined);
  }, [rev]);
  return n;
}
