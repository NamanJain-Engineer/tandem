import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { create } from "zustand";
import { FolderPlus, RefreshCw, Sparkles, X } from "lucide-react";
import type { EngineView, IpcError, Teammate, TeammateDraft, TeammateTemplate } from "@tandem/protocol";
import {
  AVATAR_HUES,
  Avatar,
  Button,
  Callout,
  EngineGlyph,
  IconButton,
  Input,
  MicButton,
  Sheet,
  Skeleton,
  Textarea,
  cx,
  insertText,
  isMac,
  useToast,
  type ComposerHandle,
} from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useEngines } from "../../stores/engines";
import { useSession } from "../../stores/session";
import { useTeammates } from "../../stores/teammates";
import { useBoot } from "../shell/useBoot";
import { useInstall } from "../code/InstallSheet";
import { useComposerDictation } from "../dictate/useComposerDictation";

/** Opens the New teammate sheet from anywhere (rail +, palette, and
 * Conductor's `create_teammate`, which fills in the description). */
export const useNewTeammate = create<{
  open: boolean;
  prefill: string | null;
  /** A template picked before the sheet opened (Agent mode's first run, D-2603). */
  template: TeammateTemplate | null;
  show: (o: boolean, prefill?: string, template?: TeammateTemplate) => void;
}>((set) => ({
  open: false,
  prefill: null,
  template: null,
  show: (open, prefill, template) => set({ open, prefill: prefill ?? null, template: template ?? null }),
}));

const NAME_MAX = 40;

/** Chat engines first (they carry memory and skills), then terminal-only. */
export function engineGroups(list: EngineView[]): { chat: EngineView[]; terminal: EngineView[] } {
  return { chat: list.filter((e) => e.chatCapable), terminal: list.filter((e) => !e.chatCapable) };
}

/** F-AG-04: whether Create with AI is offered: it is hosted, so it needs a
 * Vibezeo account that carries it, signed in and online (D-2903). */
export function canDraft(): boolean {
  const { auth, entitlement } = useSession.getState();
  return (
    !!auth?.signedIn &&
    !auth.offline &&
    !!auth.hosted &&
    !!entitlement &&
    entitlement.plan !== "free" &&
    entitlement.online
  );
}

function EngineTile({ e, selected, onPick }: { e: EngineView; selected: boolean; onPick: () => void }) {
  const installed = !!e.install;
  return (
    <div
      className={cx(
        "flex min-h-14 items-center gap-2 rounded-md border px-3 py-2",
        selected ? "border-accent bg-accent-soft" : "border-border bg-surface-2",
        !installed && "opacity-60",
      )}
    >
      <button
        type="button"
        role="radio"
        aria-checked={selected}
        data-testid="engine-tile"
        data-engine={e.spec.id}
        data-installed={installed ? "true" : "false"}
        onClick={onPick}
        className="flex min-w-0 flex-1 items-center gap-2 rounded-sm text-left focus-visible:shadow-focus focus-visible:outline-none"
      >
        <EngineGlyph letters={e.spec.logo} engine={e.spec.id} size={20} />
        <span className="flex min-w-0 flex-col">
          <span className="truncate text-base text-fg">{e.spec.label}</span>
          <span className="truncate text-sm text-fg-muted">
            {installed
              ? e.chatCapable
                ? t("agent.new.engineChat")
                : t("agent.new.engineTerminal")
              : t("agent.new.notInstalled")}
          </span>
        </span>
      </button>
      {!installed && (
        <Button size="sm" variant="ghost" onClick={() => useInstall.getState().open(e, null)}>
          {t("agent.new.install")}
        </Button>
      )}
    </div>
  );
}

/** UI-72: a new teammate. Nothing exists until Create. */
export function NewTeammateSheet() {
  const open = useNewTeammate((s) => s.open);
  const show = useNewTeammate((s) => s.show);
  return open ? <SheetBody onClose={() => show(false)} /> : null;
}

function SheetBody({ onClose }: { onClose: () => void }) {
  const engines = useEngines((s) => s.list);
  const enginesLoaded = useEngines((s) => s.loaded);
  // Nothing else may have listed the engines yet (a launch straight into Agent mode).
  useEffect(() => {
    if (!enginesLoaded) void useEngines.getState().refresh();
  }, [enginesLoaded]);
  const detecting = useEngines((s) => s.detecting);
  const detect = useEngines((s) => s.detect);
  const teammates = useTeammates((s) => s.list);
  const dataDir = useBoot((s) => s.boot?.dataDirDisplay ?? "~/.vibezeo");
  const { show } = useToast();
  const drafting = canDraft();
  const prefill = useNewTeammate((s) => s.prefill);
  useEffect(() => {
    if (!prefill) return;
    if (drafting) setDescription(prefill);
    else setBrief(prefill);
  }, [prefill, drafting]);
  const [name, setName] = useState("");
  const [brief, setBrief] = useState("");
  const [engineId, setEngineId] = useState<string | null>(null);
  const [hue, setHue] = useState<number | null>(null);
  const [customize, setCustomize] = useState(false);
  const [places, setPlaces] = useState<string[]>([]);
  const [description, setDescription] = useState("");
  const [draftState, setDraftState] = useState<"idle" | "drafting" | "done" | "error">("idle");
  const [draftError, setDraftError] = useState<string | null>(null);
  const [templates, setTemplates] = useState<TeammateTemplate[]>([]);
  const [nameError, setNameError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const picked = useNewTeammate((s) => s.template);
  useEffect(() => {
    if (!picked) return;
    setName(picked.name);
    setBrief(picked.brief);
  }, [picked]);

  const { chat, terminal } = useMemo(() => engineGroups(engines), [engines]);
  // The first installed chat engine (Claude Code first) is the default pick.
  useEffect(() => {
    if (engineId) return;
    const first =
      chat.find((e) => e.spec.id === "claude" && e.install) ?? chat.find((e) => e.install) ?? chat[0];
    if (first) setEngineId(first.spec.id);
  }, [chat, engineId]);
  useEffect(() => {
    if (!drafting)
      void invoke<TeammateTemplate[]>("teammate_templates").then(setTemplates, () => setTemplates([]));
  }, [drafting]);

  const engine = engines.find((e) => e.spec.id === engineId) ?? null;
  const trimmed = name.trim();
  const taken = teammates.some((s) => s.teammate.name.trim().toLowerCase() === trimmed.toLowerCase());
  const nameMessage = taken ? t("agent.new.nameTaken", { name: trimmed }) : nameError;
  const ready = !!trimmed && trimmed.length <= NAME_MAX && !taken && !!engine?.install && !creating;
  const dirty = !!(name || brief || description || places.length);

  const draft = async () => {
    setDraftState("drafting");
    setDraftError(null);
    try {
      const d = await invoke<TeammateDraft>("teammate_draft_with_ai", { description }, { timeoutMs: 30_000 });
      setName(d.name);
      setBrief(d.brief);
      if (d.engineId) setEngineId(d.engineId);
      setDraftState("done");
    } catch (e) {
      setDraftState("error");
      setDraftError((e as IpcError).message);
    }
  };

  const createTeammate = async () => {
    if (!ready || !engineId) return;
    setCreating(true);
    try {
      const made = await invoke<Teammate>("teammate_create", {
        input: { name: trimmed, brief, engineId, places, avatarHue: hue },
      });
      await useTeammates.getState().refresh();
      useTeammates.getState().select(made.id);
      onClose();
    } catch (e) {
      const err = e as IpcError;
      if (err.code === "conflict") setNameError(t("agent.new.nameTaken", { name: trimmed }));
      else show({ tone: "error", title: err.message });
    } finally {
      setCreating(false);
    }
  };

  return (
    <Sheet
      open
      onOpenChange={(o) => !o && onClose()}
      size="lg"
      dirty={dirty}
      title={t("agent.new.title")}
      subtitle={t("agent.new.subtitle")}
      discardPrompt={{
        title: t("agent.new.discard"),
        confirm: t("agent.new.discardOk"),
        cancel: t("agent.new.keep"),
      }}
      footer={
        <div className="flex items-center justify-between gap-2">
          <span className="text-sm text-fg-muted">{t("agent.new.nothingSaved")}</span>
          <span className="flex gap-2">
            <Button variant="ghost" onClick={onClose}>
              {t("agent.new.cancel")}
            </Button>
            <Button
              variant="primary"
              disabled={!ready}
              onClick={() => void createTeammate()}
              data-testid="teammate-create"
            >
              {t("agent.new.create")}
            </Button>
          </span>
        </div>
      }
    >
      <div
        className="flex flex-col gap-4"
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            void createTeammate();
          }
        }}
      >
        {drafting ? (
          <section
            className="flex flex-col gap-2 rounded-md border border-border bg-surface-2 p-3"
            data-testid="draft-card"
          >
            <h3 className="flex items-center gap-1.5 text-base font-medium text-fg">
              <Sparkles size={14} aria-hidden="true" />
              {t("agent.new.draftTitle")}
            </h3>
            <DraftDescription
              value={description}
              onChange={setDescription}
              onError={(message) => show({ tone: "error", title: message })}
            />
            <div className="flex items-center gap-2">
              <span className="min-w-0 flex-1 text-sm text-fg-muted">
                {draftState === "drafting"
                  ? t("agent.new.drafting")
                  : draftState === "done"
                    ? t("agent.new.drafted")
                    : draftState === "error"
                      ? draftError
                      : t("agent.new.draftCost")}
              </span>
              <Button
                size="sm"
                variant="secondary"
                disabled={!description.trim() || draftState === "drafting"}
                onClick={() => void draft()}
                data-testid="draft-run"
              >
                {t("agent.new.draft")}
              </Button>
            </div>
          </section>
        ) : (
          templates.length > 0 && (
            <section className="flex flex-col gap-2" data-testid="templates">
              <h3 className="text-sm text-fg-muted">{t("agent.new.templates")}</h3>
              <div className="flex flex-wrap gap-2">
                {templates.map((tpl) => (
                  <Button
                    key={tpl.id}
                    size="sm"
                    variant="secondary"
                    onClick={() => {
                      setName(tpl.name);
                      setBrief(tpl.brief);
                    }}
                  >
                    {tpl.name}
                  </Button>
                ))}
              </div>
            </section>
          )
        )}
        {draftState === "drafting" ? (
          <div className="flex flex-col gap-2" aria-busy="true" aria-label={t("agent.new.drafting")}>
            <Skeleton height={32} />
            <Skeleton height={96} />
          </div>
        ) : (
          <>
            <Input
              label={t("agent.new.name")}
              value={name}
              maxLength={NAME_MAX}
              onChange={(e) => {
                setName(e.target.value);
                setNameError(null);
              }}
              hint={t("agent.new.count", { n: trimmed.length, max: NAME_MAX })}
              error={nameMessage ?? undefined}
              data-testid="teammate-name-input"
              className={draftState === "done" ? "bg-accent-soft" : undefined}
            />
            <div className="flex flex-col gap-2">
              <span className="text-base text-fg">{t("agent.new.avatar")}</span>
              <div className="flex items-center gap-3">
                <Avatar name={trimmed || "?"} id={`preview-${trimmed}`} hue={hue} size={48} />
                <span className="text-sm text-fg-muted">{t("agent.new.avatarHint")}</span>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => setCustomize(!customize)}
                  aria-expanded={customize}
                >
                  {t("agent.new.customize")}
                </Button>
              </div>
              {customize && <HuePicker value={hue} onChange={setHue} />}
            </div>
            <div className="flex flex-col gap-2">
              <span className="flex items-center gap-2">
                <span className="text-base text-fg">{t("agent.new.engine")}</span>
                <Button
                  size="sm"
                  variant="ghost"
                  iconLeft={<RefreshCw size={14} />}
                  disabled={detecting}
                  onClick={() => void detect()}
                  data-testid="engines-recheck"
                >
                  {t("agent.new.recheck")}
                </Button>
              </span>
              <div role="radiogroup" aria-label={t("agent.new.engine")} className="grid grid-cols-2 gap-2">
                {chat.map((e) => (
                  <EngineTile
                    key={e.spec.id}
                    e={e}
                    selected={e.spec.id === engineId}
                    onPick={() => setEngineId(e.spec.id)}
                  />
                ))}
              </div>
              {terminal.length > 0 && (
                <>
                  <span className="text-sm text-fg-muted">{t("agent.new.terminalGroup")}</span>
                  <div
                    role="radiogroup"
                    aria-label={t("agent.new.terminalGroup")}
                    className="grid grid-cols-2 gap-2"
                  >
                    {terminal.map((e) => (
                      <EngineTile
                        key={e.spec.id}
                        e={e}
                        selected={e.spec.id === engineId}
                        onPick={() => setEngineId(e.spec.id)}
                      />
                    ))}
                  </div>
                </>
              )}
              {engine && !engine.install && (
                <Callout tone="warning">{t("agent.new.installFirst", { engine: engine.spec.label })}</Callout>
              )}
              {engine && engine.install && !engine.chatCapable && (
                <Callout tone="info">{t("agent.new.terminalNote")}</Callout>
              )}
              <span className="text-sm text-fg-muted">{t("agent.new.engineHelp")}</span>
            </div>
            <Textarea
              label={t("agent.new.brief")}
              placeholder={t("agent.new.briefPlaceholder")}
              value={brief}
              minRows={4}
              maxRows={12}
              maxLength={8000}
              onChange={(e) => setBrief(e.target.value)}
              data-testid="teammate-brief"
              className={draftState === "done" ? "bg-accent-soft" : undefined}
            />
            <div className="flex flex-col gap-1">
              <span className="text-base text-fg">{t("agent.new.home")}</span>
              <span className="font-mono text-sm text-fg-muted">
                {t("agent.new.homePath", { dir: dataDir })}
              </span>
            </div>
            <Places places={places} onChange={setPlaces} />
          </>
        )}
      </div>
    </Sheet>
  );
}

/** The 12-hue palette (F-AG-01). */
export function HuePicker({
  value,
  onChange,
}: {
  value: number | null;
  onChange: (h: number | null) => void;
}) {
  return (
    <div role="radiogroup" aria-label={t("agent.new.hue")} className="flex flex-wrap items-center gap-1.5">
      <Button
        size="sm"
        variant={value === null ? "secondary" : "ghost"}
        role="radio"
        aria-checked={value === null}
        onClick={() => onChange(null)}
      >
        {t("agent.new.hueAuto")}
      </Button>
      {AVATAR_HUES.map((h, i) => (
        <button
          key={h}
          type="button"
          role="radio"
          aria-checked={value === i}
          aria-label={t("agent.new.hueN", { n: i + 1 })}
          onClick={() => onChange(i)}
          className={cx(
            "size-6 rounded-md focus-visible:shadow-focus focus-visible:outline-none",
            value === i && "ring-2 ring-accent ring-offset-1 ring-offset-surface-2",
          )}
          style={{ background: `linear-gradient(135deg, hsl(${h} 65% 48%), hsl(${(h + 40) % 360} 70% 38%))` }}
        />
      ))}
    </div>
  );
}

/** Folders beyond Home the teammate may work in. */
export function Places({ places, onChange }: { places: string[]; onChange: (p: string[]) => void }) {
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-base text-fg">{t("agent.new.places")}</span>
      {places.length === 0 && <span className="text-sm text-fg-muted">{t("agent.new.noPlaces")}</span>}
      <ul className="flex flex-col gap-1">
        {places.map((p) => (
          <li key={p} className="flex items-center gap-2 rounded-sm border border-border px-2 py-1">
            <span className="min-w-0 flex-1 truncate font-mono text-sm text-fg">{p}</span>
            <IconButton
              size="sm"
              icon={<X size={14} />}
              label={t("agent.new.removePlace", { place: p })}
              onClick={() => onChange(places.filter((x) => x !== p))}
            />
          </li>
        ))}
      </ul>
      <Button
        size="sm"
        variant="ghost"
        className="self-start"
        iconLeft={<FolderPlus size={14} />}
        onClick={async () => {
          const picked = await invoke<string | null>("workspace_pick_folder").catch(() => null);
          if (picked && !places.includes(picked)) onChange([...places, picked]);
        }}
      >
        {t("agent.new.addPlace")}
      </Button>
      <span className="text-sm text-fg-muted">{t("agent.new.placesHelp")}</span>
    </div>
  );
}

/** "What's the job?" with the mic (F-DI-12): dictation lands at the caret,
 * as in every composer, and ⌥Space held dictates on macOS. */
function DraftDescription({
  value,
  onChange,
  onError,
}: {
  value: string;
  onChange: (next: string) => void;
  onError: (message: string) => void;
}) {
  const area = useRef<HTMLTextAreaElement | null>(null);
  const current = useRef(value);
  current.current = value;
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const handle = useRef<ComposerHandle | null>({
    focus: () => area.current?.focus(),
    insertAtCaret: (text) => {
      const el = area.current;
      const at = el ? [el.selectionStart, el.selectionEnd] : [current.current.length, current.current.length];
      const next = insertText(current.current, at[0] ?? 0, at[1] ?? 0, text);
      onChangeRef.current(next.value);
      requestAnimationFrame(() => area.current?.setSelectionRange(next.caret, next.caret));
    },
  });
  const { dictation, onFocusChange } = useComposerDictation("teammate-draft", handle, onError);
  const holding = useRef(false);
  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (isMac() && !dictation.disabledReason && e.code === "Space" && e.altKey && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      if (!holding.current && !e.repeat) {
        holding.current = true;
        dictation.onHoldKey?.(true);
      }
    }
  };
  const onKeyUp = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (holding.current && (e.code === "Space" || e.key === "Alt")) {
      holding.current = false;
      dictation.onHoldKey?.(false);
    }
  };
  return (
    <div className="flex items-start gap-1">
      <div className="min-w-0 flex-1">
        <Textarea
          ref={area}
          label={t("agent.new.draftLabel")}
          hideLabel
          placeholder={
            dictation.state === "listening" ? dictation.listeningPlaceholder : t("agent.new.draftPlaceholder")
          }
          value={value}
          minRows={2}
          maxRows={5}
          onChange={(e) => onChange(e.target.value)}
          onFocus={() => onFocusChange(true)}
          onBlur={() => onFocusChange(false)}
          onKeyDown={onKeyDown}
          onKeyUp={onKeyUp}
          data-testid="draft-description"
        />
      </div>
      <MicButton dictation={dictation} label={t("agent.new.dictate")} testId="draft-mic" />
    </div>
  );
}
