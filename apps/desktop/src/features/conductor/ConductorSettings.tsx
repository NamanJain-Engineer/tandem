import { useCallback, useEffect, useState, type KeyboardEvent } from "react";
import type { ConductorMemory, MemoryProposal, WakeStatus } from "@tandem/protocol";
import {
  Badge,
  Button,
  Callout,
  ConfirmDialog,
  EmptyState,
  Input,
  SegmentedControl,
  Skeleton,
  Switch,
  Textarea,
  useToast,
} from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { listen } from "../../ipc/events";
import { useSetting } from "../../stores/settings";
import { SavedHint, useSave } from "../settings/controls";

type Ids = { id: string; describedBy?: string };

/** "Name" (UI-67): empty goes back to "Conductor"; the wake phrase stays "Hey Conductor". */
export function NameControl({ ids }: { ids: Ids }) {
  const value = useSetting<string>("conductor_name", "Conductor");
  const save = useSave();
  const [draft, setDraft] = useState(value);
  const [saved, setSaved] = useState(false);
  useEffect(() => setDraft(value), [value]);
  const commit = async () => {
    const next = draft.trim() || "Conductor";
    setDraft(next);
    if (next !== value && (await save("conductor_name", next))) {
      setSaved(true);
      setTimeout(() => setSaved(false), 1000);
    }
  };
  return (
    <span className="inline-flex items-center gap-2">
      <SavedHint shown={saved} />
      <Input
        id={ids.id}
        aria-describedby={ids.describedBy}
        aria-label={t("conductor.persona.name")}
        className="w-56"
        maxLength={40}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => void commit()}
        onKeyDown={(e: KeyboardEvent) => e.key === "Enter" && void commit()}
      />
    </span>
  );
}

const SPEEDS = [1, 2, 3, 4, 5] as const;

/** "Speech speed": five steps, 3 is normal (also "talk faster/slower" in a call). */
export function SpeedControl() {
  const value = useSetting<number>("conductor_speed", 3);
  const save = useSave();
  return (
    <SegmentedControl
      label={t("conductor.speed")}
      size="sm"
      value={String(value)}
      items={SPEEDS.map((s) => ({ id: String(s), label: t(`conductor.speed.${s}`) }))}
      onValueChange={(v) => void save("conductor_speed", Number(v))}
    />
  );
}

export const ABOUT_MAX = 1000;

/** "Tell it about you": up to 1,000 characters. */
export function AboutYou() {
  const value = useSetting<string | null>("conductor_about_you", null);
  const save = useSave();
  const [draft, setDraft] = useState(value ?? "");
  const [saved, setSaved] = useState(false);
  useEffect(() => setDraft(value ?? ""), [value]);
  const commit = async () => {
    const next = draft.trim() === "" ? null : draft.trim();
    if (next !== value && (await save("conductor_about_you", next))) {
      setSaved(true);
      setTimeout(() => setSaved(false), 1000);
    }
  };
  return (
    <div className="flex flex-col gap-1" data-testid="conductor-about">
      <Textarea
        label={t("conductor.about")}
        hint={t("conductor.about.count", { n: draft.length, max: ABOUT_MAX })}
        placeholder={t("conductor.about.placeholder")}
        maxLength={ABOUT_MAX}
        minRows={3}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => void commit()}
      />
      <SavedHint shown={saved} />
    </div>
  );
}

type Load = { state: "loading" } | { state: "error"; message: string } | { state: "ready"; memory: ConductorMemory };

function message(e: unknown): string {
  return e && typeof e === "object" && "message" in e ? String((e as { message: unknown }).message) : String(e);
}

/** Memory (UI-67): view, edit and clear `memory.md`, and Keep / Discard
 * proposed memories. Off by default; with it off nothing is written. */
export function MemoryPanel() {
  const enabled = useSetting<boolean>("conductor_memory", false);
  const [load, setLoad] = useState<Load>({ state: "loading" });
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState(false);
  const { show } = useToast();

  const apply = useCallback((memory: ConductorMemory) => {
    setLoad({ state: "ready", memory });
    setDraft(memory.text);
    setError(null);
  }, []);
  const refresh = useCallback(() => {
    setLoad({ state: "loading" });
    invoke<ConductorMemory | null>("conductor_memory_get")
      .then((m) => (m ? apply(m) : setLoad({ state: "error", message: t("conductor.memory.loadFailed") })))
      .catch((e: unknown) => setLoad({ state: "error", message: message(e) }));
  }, [apply]);
  useEffect(refresh, [refresh, enabled]);

  const saveText = async () => {
    try {
      apply(await invoke<ConductorMemory>("conductor_memory_save", { text: draft }));
      show({ tone: "success", title: t("conductor.memory.saved") });
    } catch (e) {
      setError(message(e));
    }
  };
  const clear = async () => {
    try {
      apply(await invoke<ConductorMemory>("conductor_memory_clear"));
      show({ tone: "success", title: t("conductor.memory.cleared") });
    } catch (e) {
      setError(message(e));
    }
  };
  const resolve = async (p: MemoryProposal, keep: boolean) => {
    try {
      apply(await invoke<ConductorMemory>("conductor_memory_resolve", { proposalIds: [p.id], keep }));
    } catch (e) {
      setError(message(e));
    }
  };

  if (load.state === "loading")
    return (
      <section aria-label={t("conductor.memory")} aria-busy="true" className="flex flex-col gap-2" data-testid="conductor-memory-loading">
        <Skeleton width="40%" />
        <Skeleton height={80} />
      </section>
    );
  if (load.state === "error")
    return (
      <Callout tone="error" title={t("conductor.memory.loadFailed")} actions={<Button size="sm" variant="secondary" onClick={refresh}>{t("settings.retry")}</Button>}>
        {load.message}
      </Callout>
    );
  const m = load.memory;
  const dirty = draft !== m.text;
  return (
    <section aria-label={t("conductor.memory")} className="flex flex-col gap-3" data-testid="conductor-memory">
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-md font-semibold text-fg">{t("conductor.memory.file")}</h3>
        <Badge variant={enabled ? "accent" : "neutral"}>{enabled ? t("conductor.memory.on") : t("conductor.memory.off")}</Badge>
      </div>
      {m.proposals.length > 0 && (
        <ul className="flex flex-col gap-2" aria-label={t("conductor.memory.proposals")}>
          {m.proposals.map((p) => (
            <li key={p.id} className="flex items-center justify-between gap-2 rounded-md border border-border bg-surface-2 px-3 py-2">
              <span className="text-base text-fg">{p.text}</span>
              <span className="flex gap-1">
                <Button size="sm" variant="secondary" onClick={() => void resolve(p, true)}>{t("conductor.memory.keep")}</Button>
                <Button size="sm" variant="ghost" onClick={() => void resolve(p, false)}>{t("conductor.memory.discard")}</Button>
              </span>
            </li>
          ))}
        </ul>
      )}
      {!enabled && m.text === "" ? (
        <EmptyState title={t("conductor.memory.emptyOff")} body={t("conductor.memory.emptyOff.body")} />
      ) : (
        <>
          {m.facts.length === 0 && m.text === "" && (
            <p className="text-sm text-fg-subtle">{t("conductor.memory.empty")}</p>
          )}
          <Textarea
            label={t("conductor.memory.editor")}
            hint={m.path}
            monospace
            minRows={4}
            maxRows={16}
            value={draft}
            disabled={!enabled}
            error={error ?? undefined}
            onChange={(e) => setDraft(e.target.value)}
          />
          <div className="flex flex-wrap gap-2">
            <Button size="sm" variant="primary" disabled={!enabled || !dirty} onClick={() => void saveText()}>
              {t("conductor.memory.save")}
            </Button>
            <Button size="sm" variant="danger" disabled={m.text === "" && m.proposals.length === 0} onClick={() => setConfirm(true)}>
              {t("conductor.memory.forgetAll")}
            </Button>
          </div>
        </>
      )}
      <ConfirmDialog
        open={confirm}
        onOpenChange={setConfirm}
        danger
        title={t("conductor.memory.forgetAll.title")}
        body={t("conductor.memory.forgetAll.body")}
        confirmLabel={t("conductor.memory.forgetAll.confirm")}
        onConfirm={() => void clear()}
      />
    </section>
  );
}

/** Live status of the wake listener, from `conductor_wake_status` and
 * `conductor:wake-state`. */
export function useWakeStatus(): WakeStatus | null {
  const [status, setStatus] = useState<WakeStatus | null>(null);
  useEffect(() => {
    let off: (() => void) | undefined;
    let alive = true;
    invoke<WakeStatus>("conductor_wake_status")
      .then((s) => alive && setStatus(s ?? null))
      .catch(() => alive && setStatus(null));
    void listen<WakeStatus>("conductor:wake-state", (s) => setStatus(s)).then((u) => {
      if (alive) off = u;
      else u();
    });
    return () => {
      alive = false;
      off?.();
    };
  }, []);
  return status;
}

function wakeLine(s: WakeStatus | null): string | null {
  if (!s) return null;
  switch (s.state.state) {
    case "listening":
      return t("conductor.wake.listening");
    case "paused":
      return t(`conductor.wake.paused.${s.state.reason}`);
    case "mic_error":
      return t("conductor.wake.micError");
    default:
      return null;
  }
}

/** "Wake phrase "Hey Conductor"": off by default; unavailable builds say why. */
export function WakeControl({ ids }: { ids: Ids }) {
  const value = useSetting<boolean>("conductor_wake", false);
  const save = useSave();
  const status = useWakeStatus();
  const unavailable = status?.unavailable ?? null;
  const line = unavailable ? t(`conductor.wake.unavailable.${unavailable}`) : value ? wakeLine(status) : null;
  return (
    <span className="inline-flex items-center gap-2">
      {line && (
        <span role="status" className="text-sm text-fg-subtle" data-testid="conductor-wake-status">
          {line}
        </span>
      )}
      <Switch
        id={ids.id}
        aria-describedby={ids.describedBy}
        aria-label={t("conductor.wake")}
        checked={value && !unavailable}
        disabled={!!unavailable}
        onCheckedChange={(v) => void save("conductor_wake", v)}
      />
    </span>
  );
}
