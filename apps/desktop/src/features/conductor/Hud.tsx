import { useEffect, useState } from "react";
import {
  ClipboardCheck,
  AppWindow,
  Bell,
  Check,
  CircleHelp,
  Diamond,
  FolderSearch,
  ListChecks,
  Mic,
  MicOff,
  Minus,
  PanelsTopLeft,
  ScrollText,
  Settings2,
  Users,
  X,
} from "lucide-react";
import type { CallView, ChipView, VoiceToolGroup } from "@tandem/protocol";
import { Button, IconButton, Popover, Spinner, Tooltip, WaveformRing, cx } from "@tandem/ui";
import { t } from "../../i18n";
import { confirm, endCall, setMuted, startCall, useConductor } from "../../stores/conductor";
import { availableCredits, useSession } from "../../stores/session";
import { useUi } from "../../stores/ui";
import { clock, CREDITS_PER_MINUTE, foldChips, phaseLabel, ringState } from "./ring";
import { useAppActive } from "./useAppActive";

const GROUP_ICON: Record<VoiceToolGroup, typeof Check> = {
  navigation: AppWindow,
  awareness: Bell,
  panes: PanelsTopLeft,
  threads: ScrollText,
  teammates: Users,
  tasks: ListChecks,
  find: FolderSearch,
  app: Settings2,
  review: ClipboardCheck,
};

function ChipMark({ chip }: { chip: ChipView }) {
  if (chip.state === "running") return <Spinner size={12} label={t("conductor.chip.running")} />;
  if (chip.state === "waiting")
    return <Diamond size={12} aria-label={t("conductor.chip.waiting")} className="text-warning" />;
  if (chip.state === "done")
    return <Check size={12} aria-label={t("conductor.chip.done")} className="text-success" />;
  return <X size={12} aria-label={t("conductor.chip.failed")} className="text-danger" />;
}

/** One action chip: icon by group, middle-truncated label, ✓ ✗ spinner or ◆. */
export function ActionChip({ chip }: { chip: ChipView }) {
  const Icon = GROUP_ICON[chip.group] ?? CircleHelp;
  const body = (
    <span
      tabIndex={chip.reason ? 0 : undefined}
      data-testid="conductor-chip"
      data-state={chip.state}
      data-tool={chip.tool}
      className="inline-flex h-6 max-w-[220px] shrink-0 items-center gap-1 rounded-full border border-border bg-surface-4 px-2 text-xs text-fg focus-visible:shadow-focus focus-visible:outline-none"
    >
      <Icon size={12} aria-hidden className="text-fg-muted" />
      <span className="truncate">{chip.label}</span>
      <ChipMark chip={chip} />
    </span>
  );
  return chip.reason ? <Tooltip label={chip.reason}>{body}</Tooltip> : body;
}

function ErrorAction({ view }: { view: CallView }) {
  const ui = useUi.getState();
  const code = view.error?.code;
  const action =
    code === "mic_not_delivering"
      ? { label: t("conductor.error.switchMic"), run: () => ui.openSettings("dictate") }
      : code === "signed_out"
        ? { label: t("conductor.error.signIn"), run: () => ui.openSettings("account") }
        : code === "time_cap"
          ? { label: t("conductor.error.newCall"), run: () => void startCall() }
          : // Nothing to buy, so running out has no action (D-2914).
            code === "turned_off" ||
              code === "not_in_region" ||
              code === "needs_pro" ||
              code === "out_of_credits"
            ? null
            : { label: t("conductor.error.tryAgain"), run: () => void startCall() };
  if (!action) return null;
  return (
    <Button size="sm" variant="secondary" onClick={action.run} data-testid="conductor-error-action">
      {action.label}
    </Button>
  );
}

/** The credits indicator: "◈ 4:12 · ~26 min" (UI-61). */
function Meter({ view }: { view: CallView }) {
  const balance = useSession((s) => availableCredits(s.entitlement));
  const minutes = balance === null ? null : Math.floor(balance / CREDITS_PER_MINUTE);
  const low = minutes !== null && minutes < 5;
  return (
    <Tooltip label={t("conductor.meter.tooltip", { balance: (balance ?? 0).toLocaleString("en-US") })}>
      <span
        tabIndex={0}
        data-testid="conductor-meter"
        className={cx(
          "text-xs tabular-nums focus-visible:shadow-focus focus-visible:outline-none",
          low ? "text-warning" : "text-fg-muted",
        )}
      >
        {t("conductor.meter.elapsed", { time: clock(view.elapsedMs) })}
        {minutes !== null &&
          (low
            ? ` · ${t("conductor.meter.low", { n: minutes })}`
            : ` · ${t("conductor.meter.left", { n: minutes })}`)}
      </span>
    </Tooltip>
  );
}

/** The Conductor HUD (UI-61): top-centre over the current surface while a
 * call runs, with its ring, state, last utterance, action chips, a question,
 * and Mute / Call log / End. After a call, a 3 s receipt; after a failure,
 * the reason and what to do. */
export function Hud() {
  const view = useConductor((s) => s.view);
  const level = useConductor((s) => s.level);
  const openLog = useConductor((s) => s.openLog);
  const active = useAppActive();
  const [collapsed, setCollapsed] = useState(false);
  const [refused, setRefused] = useState<string | null>(null);
  useEffect(() => setRefused(null), [view.question?.requestId]);
  if (view.phase === "idle") return null;

  const card =
    "fixed left-1/2 top-[calc(var(--titlebar-h)+12px)] z-[var(--z-hud)] w-[520px] max-w-[calc(100vw-24px)] -translate-x-1/2 rounded-lg border border-border bg-surface-3 shadow-e3";

  if (view.phase === "ended" && view.receipt) {
    const r = view.receipt;
    return (
      <div
        role="status"
        data-testid="conductor-receipt"
        className={cx(card, "flex items-center gap-3 px-4 py-3 text-sm text-fg")}
      >
        <span className="flex-1">
          {t("conductor.receipt", {
            time: clock(r.seconds * 1000),
            credits: r.credits.toLocaleString("en-US"),
            actions: r.actions,
          })}
        </span>
        <Button size="sm" variant="ghost" onClick={() => openLog(true)}>
          {t("conductor.callLog")}
        </Button>
      </div>
    );
  }

  if (view.phase === "error") {
    return (
      <div
        role="alert"
        data-testid="conductor-error"
        data-code={view.error?.code}
        className={cx(card, "flex items-center gap-3 px-4 py-3")}
      >
        <WaveformRing state="error" size={28} label={t("conductor.phase.error")} paused />
        <div className="flex min-w-0 flex-1 flex-col">
          <span className="text-2xs font-semibold uppercase tracking-caps text-danger">
            {t("conductor.phase.error")}
          </span>
          <span className="text-sm text-fg" data-testid="conductor-error-message">
            {view.error?.message}
          </span>
        </div>
        <ErrorAction view={view} />
        <IconButton
          size="sm"
          icon={<X size={14} />}
          label={t("conductor.dismiss")}
          onClick={() => void endCall()}
          data-testid="conductor-dismiss"
        />
      </div>
    );
  }

  const { visible, folded } = foldChips(view.chips);
  const q = view.question;

  if (collapsed) {
    const newest = visible[visible.length - 1];
    return (
      <div
        data-testid="conductor-hud"
        data-phase={view.phase}
        className={cx(card, "flex h-9 items-center gap-2 px-2")}
      >
        <WaveformRing
          state={ringState(view)}
          level={level}
          size={28}
          label={phaseLabel(view)}
          paused={!active}
        />
        <span className="text-2xs font-semibold uppercase tracking-caps text-fg-muted">
          {phaseLabel(view)}
        </span>
        {newest && <ActionChip chip={newest} />}
        <span className="flex-1" />
        <IconButton
          size="sm"
          icon={<PanelsTopLeft size={14} />}
          label={t("conductor.expand")}
          onClick={() => setCollapsed(false)}
        />
      </div>
    );
  }

  return (
    <div data-testid="conductor-hud" data-phase={view.phase} className={cx(card, "flex flex-col gap-2 p-3")}>
      <div className="flex items-center gap-3">
        <WaveformRing
          state={ringState(view)}
          level={level}
          size={56}
          label={phaseLabel(view)}
          paused={!active}
        />
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <span
            data-testid="conductor-phase"
            className="text-2xs font-semibold uppercase tracking-caps text-fg-muted"
          >
            {phaseLabel(view)}
          </span>
          {view.utterance ? (
            <span data-testid="conductor-utterance" className="line-clamp-2 text-md text-fg">
              {t("conductor.quoted", { text: view.utterance })}
            </span>
          ) : (
            <span className="text-sm text-fg-muted">
              {view.phase === "connecting"
                ? t("conductor.connectingHint")
                : view.muted
                  ? t("conductor.mutedHint")
                  : t("conductor.listeningHint")}
            </span>
          )}
          {view.caption && view.phase === "speaking" && (
            <span className="line-clamp-2 text-sm text-fg-muted">{view.caption}</span>
          )}
        </div>
        <div className="flex flex-col items-end gap-1 self-start">
          <Meter view={view} />
          <IconButton
            size="sm"
            icon={<Minus size={14} />}
            label={t("conductor.collapse")}
            onClick={() => setCollapsed(true)}
          />
        </div>
      </div>

      {view.notice && (
        <span role="status" data-testid="conductor-notice" className="text-sm text-warning">
          {view.notice}
        </span>
      )}

      {view.chips.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5" data-testid="conductor-chips">
          {visible.map((c) => (
            <ActionChip key={c.id} chip={c} />
          ))}
          {folded.length > 0 && (
            <Popover
              label={t("conductor.olderActions")}
              trigger={
                <button
                  type="button"
                  data-testid="conductor-more"
                  className="inline-flex h-6 items-center rounded-full border border-border bg-surface-4 px-2 text-xs text-fg-muted hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
                >
                  {t("conductor.more", { n: folded.length })}
                </button>
              }
            >
              <div className="flex max-w-[320px] flex-col gap-1 p-2">
                {folded.map((c) => (
                  <ActionChip key={c.id} chip={c} />
                ))}
                <Button size="sm" variant="link" onClick={() => openLog(true)}>
                  {t("conductor.openCallLog")}
                </Button>
              </div>
            </Popover>
          )}
        </div>
      )}

      {q && (
        <div
          role="group"
          aria-label={t("conductor.question")}
          data-testid="conductor-question"
          data-request-id={q.requestId}
          data-review={q.review}
          className="flex items-center gap-2 rounded-md border border-border bg-surface-2 px-3 py-2"
        >
          <Diamond size={14} aria-hidden className="shrink-0 text-warning" />
          <div className="flex min-w-0 flex-1 flex-col">
            <span className="text-sm text-fg">{q.summary}</span>
            {q.review === "screen_only" && (
              <span className="text-xs text-fg-muted">{t("conductor.confirmOnScreen")}</span>
            )}
            {refused && <span className="text-xs text-danger">{refused}</span>}
          </div>
          <Button
            size="sm"
            variant="secondary"
            data-testid="conductor-no"
            onClick={() => void confirm(q.requestId, false).then(setRefused)}
          >
            {t("conductor.no")}
          </Button>
          <Button
            size="sm"
            variant="primary"
            data-testid="conductor-yes"
            onClick={() => void confirm(q.requestId, true).then(setRefused)}
          >
            {t("conductor.yes", { verb: q.verb })}
          </Button>
        </div>
      )}

      <div className="flex items-center gap-1">
        <Button
          size="sm"
          variant="ghost"
          iconLeft={view.muted ? <MicOff size={14} /> : <Mic size={14} />}
          data-testid="conductor-mute"
          aria-pressed={view.muted}
          onClick={() => setMuted(!view.muted)}
        >
          {view.muted ? t("conductor.unmute") : t("conductor.mute")}
        </Button>
        <Button size="sm" variant="ghost" iconLeft={<ScrollText size={14} />} onClick={() => openLog(true)}>
          {t("conductor.callLog")}
        </Button>
        <span className="flex-1" />
        <Button
          size="sm"
          variant="danger"
          iconLeft={<X size={14} />}
          data-testid="conductor-end"
          onClick={() => void endCall()}
        >
          {t("conductor.end")}
        </Button>
      </div>
    </div>
  );
}
