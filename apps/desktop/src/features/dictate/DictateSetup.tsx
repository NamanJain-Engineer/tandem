import { useEffect, useRef, useState } from "react";
import { Check } from "lucide-react";
import type { ComposerHandle } from "@tandem/ui";
import type { IpcError, PermissionStatus } from "@tandem/protocol";
import { Badge, Button, Callout, Dialog, SegmentedControl, Select, Textarea, WaveformRing, insertText, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { listen } from "../../ipc/events";
import { invoke } from "../../ipc/invoke";
import { useDictate } from "../../stores/dictate";
import { useSetting, useSettings } from "../../stores/settings";
import { useBoot } from "../shell/useBoot";
import { presets } from "./binding";
import { MicSelect, ShortcutField } from "./DictatePage";
import { EngineControl } from "./EngineControl";
import { useComposerDictation } from "./useComposerDictation";

/** "Try it" (F-DI-11 AC2): passes only once a native press and release of
 * the shortcut were both seen. */
export function TryIt() {
  const status = useDictate((s) => s.status);
  const [phase, setPhase] = useState<"idle" | "waiting" | "down" | "done">(status?.verified ? "done" : "idle");
  useEffect(() => {
    const offs = [
      listen<{ phase: string }>("dictate:key", (e) => setPhase((p) => (p === "done" ? p : e.phase === "down" ? "down" : p))),
      listen<null>("dictate:verified", () => {
        setPhase("done");
        void useDictate.getState().loadStatus();
      }),
    ];
    return () => {
      offs.forEach((o) => void o.then((off) => off()));
      void invoke("dictate_verify_stop").catch(() => undefined);
    };
  }, []);
  const start = () => {
    setPhase("waiting");
    void invoke("dictate_verify_start").catch(() => undefined);
  };
  return (
    <div className="flex flex-col items-start gap-2" data-testid="dictate-try-it" data-phase={phase}>
      {phase === "done" ? (
        <span data-testid="dictate-key-detected">
          <Badge variant="done">
            <Check size={12} aria-hidden="true" /> {t("dictate.setup.detected")}
          </Badge>
        </span>
      ) : phase === "idle" ? (
        <Button size="sm" variant="secondary" onClick={start} data-testid="dictate-verify">
          {t("dictate.setup.pressNow")}
        </Button>
      ) : (
        <span role="status" className="text-sm text-fg">
          {phase === "down" ? t("dictate.setup.release") : t("dictate.setup.waiting", { key: status?.hold.label ?? "" })}
        </span>
      )}
      {status?.session === "mac" && phase === "waiting" && <span className="text-sm text-fg-muted">{t("dictate.setup.fnHint")}</span>}
    </div>
  );
}

/** A text field dictation can type into: words land at its caret. */
function TestField() {
  const [text, setText] = useState("");
  const area = useRef<HTMLTextAreaElement | null>(null);
  const handle = useRef<ComposerHandle | null>({
    focus: () => area.current?.focus(),
    insertAtCaret: (words) => {
      const el = area.current;
      setText((cur) => {
        const start = el?.selectionStart ?? cur.length;
        const end = el?.selectionEnd ?? start;
        return insertText(cur, start, end, words).value;
      });
    },
  });
  const { onFocusChange } = useComposerDictation("onboarding:try", handle);
  return (
    <Textarea
      ref={area}
      label={t("dictate.setup.testField")}
      hideLabel
      placeholder={t("dictate.setup.testField")}
      rows={2}
      value={text}
      onChange={(e) => setText(e.target.value)}
      onFocus={() => onFocusChange(true)}
      onBlur={() => onFocusChange(false)}
      data-testid="dictate-test-field"
    />
  );
}

function Permissions() {
  const [p, setP] = useState<PermissionStatus | null>(null);
  const load = () => void invoke<PermissionStatus>("dictate_permissions").then(setP, () => undefined);
  useEffect(() => {
    load();
    const id = setInterval(load, 2_000);
    return () => clearInterval(id);
  }, []);
  if (!p || p.inputMonitoring === "not_needed") return null;
  const row = (kind: "input_monitoring" | "accessibility", label: string, why: string, state: string) => (
    <li className="flex items-center gap-3">
      <span className="flex flex-1 flex-col">
        <span className="text-base text-fg">{label}</span>
        <span className="text-sm text-fg-muted">{why}</span>
      </span>
      {state === "granted" ? (
        <Badge variant="done">{t("dictate.permissions.granted")}</Badge>
      ) : (
        <Button size="sm" variant="secondary" onClick={() => void invoke("dictate_open_permission_settings", { kind })}>
          {t("dictate.permissions.grant")}
        </Button>
      )}
    </li>
  );
  return (
    <section className="flex flex-col gap-2" aria-label={t("dictate.permissions.title")}>
      <h3 className="text-sm font-semibold text-fg">{t("dictate.permissions.title")}</h3>
      <ul className="flex flex-col gap-2">
        {row("input_monitoring", t("dictate.permissions.inputMonitoring"), t("dictate.permissions.inputMonitoringWhy"), p.inputMonitoring)}
        {row("accessibility", t("dictate.permissions.accessibility"), t("dictate.permissions.accessibilityWhy"), p.accessibility)}
      </ul>
      {p.inputMonitoring === "granted" && (
        <Callout tone="info">
          <span className="flex items-center gap-2">
            {t("dictate.permissions.relaunch")}
            <Button size="sm" variant="secondary" onClick={() => void invoke("dictate_relaunch")}>
              {t("dictate.permissions.quitReopen")}
            </Button>
          </span>
        </Callout>
      )}
    </section>
  );
}

function ShortcutChoice() {
  const platform = useBoot((s) => s.boot?.platform ?? "linux");
  const status = useDictate((s) => s.status);
  const p = presets(platform);
  const current = status?.hold.binding ?? p.recommended;
  const [choice, setChoice] = useState(current === p.recommended ? "recommended" : current === p.oneKey ? "one" : "custom");
  const set = (binding: string) =>
    void invoke("dictate_set_binding", { binding, mode: "hold" }).then(() => void useDictate.getState().loadStatus(), () => undefined);
  return (
    <section className="flex flex-col gap-2" aria-label={t("dictate.setup.shortcut")}>
      <h3 className="text-sm font-semibold text-fg">{t("dictate.setup.shortcut")}</h3>
      <SegmentedControl
        label={t("dictate.setup.shortcut")}
        size="sm"
        value={choice}
        onValueChange={(v) => {
          setChoice(v);
          if (v === "recommended") set(p.recommended);
          if (v === "one") set(p.oneKey);
        }}
        items={[
          { id: "recommended", label: t("dictate.setup.recommended") },
          { id: "one", label: t("dictate.setup.oneKey") },
          { id: "custom", label: t("dictate.setup.custom") },
        ]}
      />
      {choice === "custom" && <ShortcutField mode="hold" label={t("dictate.shortcut.hold")} />}
      <span className="text-sm text-fg-muted">{t("dictate.setup.current", { key: status?.hold.label ?? "" })}</span>
      {p.fallback && (
        <Button size="sm" variant="ghost" className="self-start" onClick={() => set(p.fallback ?? "Ctrl+Alt")}>
          {t("dictate.setup.fnFallback")}
        </Button>
      )}
    </section>
  );
}

function Engine() {
  const models = useDictate((s) => s.models);
  const { show } = useToast();
  useEffect(() => {
    void useDictate.getState().loadModels();
  }, []);
  const rec = models?.models.find((m) => m.recommended && m.available && m.runnable) ?? models?.models.find((m) => m.available && m.runnable);
  if (!models || !rec) return null;
  const active = models.models.find((m) => m.id === models.active);
  const pct =
    rec.progress?.stage === "downloading" && Number(rec.progress.total) > 0
      ? Math.round((Number(rec.progress.done) / Number(rec.progress.total)) * 100)
      : null;
  return (
    <section className="flex flex-col gap-2" aria-label={t("dictate.setup.engine")} data-testid="dictate-setup-engine">
      <h3 className="text-sm font-semibold text-fg">{t("dictate.setup.engine")}</h3>
      <EngineControl />
      {active ? (
        <span className="text-sm text-fg">{t("dictate.setup.usingModel", { name: active.name })}</span>
      ) : (
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant="primary"
            disabled={rec.state === "downloading" || rec.state === "verifying" || rec.state === "loading"}
            onClick={() => void invoke("dictate_model_download", { modelId: rec.id }).catch((e: IpcError) => show({ tone: "error", title: e.message }))}
            data-testid="dictate-setup-download"
          >
            {t("dictate.setup.download", { name: rec.name, size: Math.round(Number(rec.bytes) / 1e6) })}
          </Button>
          {pct !== null && <span className="text-sm text-fg-muted">{t("dictate.models.progressPct", { pct })}</span>}
          {rec.state === "failed" && <span className="text-sm text-[var(--tn-danger)]">{rec.error}</span>}
        </div>
      )}
    </section>
  );
}

function Output() {
  const value = useSetting<string>("dictate_output", "paste");
  return (
    <section className="flex flex-col gap-2" aria-label={t("dictate.setup.output")}>
      <h3 className="text-sm font-semibold text-fg">{t("dictate.setup.output")}</h3>
      <Select
        ariaLabel={t("dictate.setup.output")}
        size="sm"
        value={value}
        onValueChange={(v) => void useSettings.getState().set("dictate_output", v)}
        options={[
          { value: "paste", label: t("dictate.output.paste") },
          { value: "clipboard", label: t("dictate.output.clipboard") },
        ]}
        className="w-64"
      />
    </section>
  );
}

/** F-DI-11's steps on one page: permissions (macOS), the shortcut, Try it,
 * the engine and where text goes. Used by onboarding and Settings. */
export function DictateSetupBody() {
  const status = useDictate((s) => s.status);
  const level = useDictate((s) => s.level);
  const take = useDictate((s) => s.take);
  useEffect(() => {
    void useDictate.getState().loadStatus();
  }, []);
  return (
    <div className="flex flex-col gap-5" data-testid="dictate-setup">
      <div className="flex items-center gap-4">
        <WaveformRing size={120} state={take?.state === "listening" ? "listening" : "idle"} level={level} label={t("dictate.setup.ring")} />
        <p className="text-base text-fg">{t("dictate.setup.intro", { key: status?.hold.label ?? "" })}</p>
      </div>
      <Permissions />
      <ShortcutChoice />
      <section className="flex flex-col gap-2" aria-label={t("dictate.setup.try")}>
        <h3 className="text-sm font-semibold text-fg">{t("dictate.setup.try")}</h3>
        <TryIt />
        <TestField />
      </section>
      <section className="flex flex-col gap-2" aria-label={t("dictate.mic.label")}>
        <h3 className="text-sm font-semibold text-fg">{t("dictate.mic.label")}</h3>
        <MicSelect k="dictate_mic" label={t("dictate.mic.label")} none={t("dictate.mic.none")} />
      </section>
      <Engine />
      <Output />
    </div>
  );
}

/** The setup, re-run from Settings › Dictate ("Set up" / "Try it"). */
export function DictateSetup({ open, onClose }: { open: boolean; onClose: () => void }) {
  if (!open) return null;
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()} title={t("dictate.setup.title")} size="lg">
      <DictateSetupBody />
    </Dialog>
  );
}
