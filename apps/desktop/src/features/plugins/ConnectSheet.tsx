import { useEffect, useRef, useState } from "react";
import { Check, Circle, Loader2, X } from "lucide-react";
import type {
  ConnectOp,
  ConnectProgress,
  IpcError,
  PluginCard,
  Step,
  StepState,
  TeammateSummary,
} from "@tandem/protocol";
import { Banner, Button, Checkbox, CodeBlock, SecretInput, SegmentedControl, Sheet, cx } from "@tandem/ui";
import { t, type MessageKey } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { listen } from "../../ipc/events";
import { methods } from "./model";

const STEPS: Step[] = ["discovery", "registration", "sign_in", "token_exchange", "verification"];

type Phase =
  | { kind: "idle" }
  | { kind: "running"; opId: string | null; steps: Partial<Record<Step, StepState>>; url: string | null }
  | { kind: "failed"; step: Step; error: string; steps: Partial<Record<Step, StepState>> }
  | { kind: "done"; card: PluginCard };

/** UI-91: connect (or reconnect) one plugin. OAuth shows each step live;
 * an API key is masked and tested before it's saved; success offers the
 * teammates to enable it for (none checked). */
export function ConnectSheet({
  card,
  reconnect,
  open,
  onOpenChange,
}: {
  card: PluginCard;
  /** The connection to reconnect; its credential changes only on success. */
  reconnect?: string | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const ways = methods(card);
  const [way, setWay] = useState(ways[0] ?? "oauth");
  const [phase, setPhase] = useState<Phase>({ kind: "idle" });
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const op = useRef<string | null>(null);
  // Events that arrive before the op id does (a fast failure can).
  const early = useRef<ConnectProgress[] | null>(null);

  const apply = (p: ConnectProgress) => {
    if (p.card) {
      op.current = null;
      setPhase({ kind: "done", card: p.card });
      return;
    }
    setPhase((prev) => {
      const steps = prev.kind === "running" || prev.kind === "failed" ? { ...prev.steps } : {};
      steps[p.step] = p.state;
      if (p.state === "failed") {
        op.current = null;
        return { kind: "failed", step: p.step, error: p.error ?? "", steps };
      }
      return {
        kind: "running",
        opId: op.current,
        steps,
        url: p.authorizeUrl ?? (prev.kind === "running" ? prev.url : null),
      };
    });
  };
  const applyRef = useRef(apply);
  applyRef.current = apply;

  // Progress of the running OAuth attempt.
  useEffect(() => {
    let off: (() => void) | undefined;
    let gone = false;
    void listen<ConnectProgress>("plugins:connect", (p) => {
      if (!op.current) {
        early.current?.push(p);
        return;
      }
      if (p.opId === op.current) applyRef.current(p);
    }).then((u) => (gone ? u() : (off = u)));
    return () => {
      gone = true;
      off?.();
    };
  }, []);

  const startOAuth = () => {
    setPhase({ kind: "running", opId: null, steps: {}, url: null });
    early.current = [];
    void invoke<ConnectOp>("plugin_connect_oauth", {
      pluginId: card.pluginId,
      connectionId: reconnect ?? null,
    }).then(
      (r) => {
        op.current = r.opId;
        const seen = early.current ?? [];
        early.current = null;
        for (const p of seen) if (p.opId === r.opId && op.current) apply(p);
      },
      (e: IpcError) => setPhase({ kind: "failed", step: "discovery", error: e.message, steps: {} }),
    );
  };

  const cancel = () => {
    if (op.current) void invoke("plugin_connect_cancel", { opId: op.current }).catch(() => undefined);
    op.current = null;
    early.current = null;
    setKey("");
    setPhase({ kind: "idle" });
    onOpenChange(false);
  };

  const submitKey = async () => {
    setBusy(true);
    try {
      const card2 = await invoke<PluginCard | null>("plugin_connect_api_key", {
        pluginId: card.pluginId,
        key,
        connectionId: reconnect ?? null,
      });
      setKey("");
      if (card2) setPhase({ kind: "done", card: card2 });
      else cancel();
    } catch (e) {
      setPhase({ kind: "failed", step: "verification", error: (e as IpcError).message, steps: {} });
    } finally {
      setBusy(false);
    }
  };

  const connectNone = async () => {
    setBusy(true);
    try {
      const c = await invoke<PluginCard>("plugin_connect_local_port", {
        pluginId: card.pluginId,
        port: null,
      });
      setPhase({ kind: "done", card: c });
    } catch (e) {
      setPhase({ kind: "failed", step: "verification", error: (e as IpcError).message, steps: {} });
    } finally {
      setBusy(false);
    }
  };

  const title = t("plugins.connect.title", { service: card.label });
  if (phase.kind === "done") {
    return (
      <Sheet open={open} onOpenChange={(o) => !o && cancel()} title={title}>
        <ChooseTeammates card={phase.card} onDone={() => onOpenChange(false)} />
      </Sheet>
    );
  }

  const steps = phase.kind === "running" || phase.kind === "failed" ? phase.steps : {};
  return (
    <Sheet
      open={open}
      onOpenChange={(o) => !o && cancel()}
      title={title}
      footer={
        <div className="flex w-full items-center gap-2">
          <Button variant="secondary" onClick={cancel} data-testid="connect-cancel">
            {t("plugins.connect.cancel")}
          </Button>
          {phase.kind === "running" && phase.url && (
            <>
              <span className="text-sm text-fg-muted">{t("plugins.connect.didntOpen")}</span>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => void navigator.clipboard?.writeText(phase.url ?? "").catch(() => undefined)}
              >
                {t("plugins.connect.copyLink")}
              </Button>
            </>
          )}
          <span className="flex-1" />
          {way === "key" && (
            <Button
              variant="primary"
              disabled={busy || !key.trim()}
              onClick={() => void submitKey()}
              data-testid="connect-key"
            >
              {t("plugins.connect.testAndConnect")}
            </Button>
          )}
          {way === "oauth" && phase.kind !== "running" && (
            <Button variant="primary" onClick={startOAuth} data-testid="connect-start">
              {phase.kind === "failed" ? t("plugins.connect.retry") : t("plugins.connect.signIn")}
            </Button>
          )}
          {way === "none" && (
            <Button
              variant="primary"
              disabled={busy}
              onClick={() => void connectNone()}
              data-testid="connect-none"
            >
              {t("plugins.connect.connect")}
            </Button>
          )}
        </div>
      }
    >
      <div className="flex flex-col gap-4" data-testid="connect-sheet">
        {reconnect && <p className="text-sm text-fg-muted">{t("plugins.connect.reconnectNote")}</p>}
        {card.unavailableReason && (
          // Why only a token is offered (GitHub, Slack: D-961).
          <p className="text-sm text-fg-muted" data-testid="connect-why-token">
            {card.unavailableReason}
          </p>
        )}
        {ways.length > 1 && (
          <SegmentedControl
            label={t("plugins.connect.method")}
            value={way}
            onValueChange={(v) => {
              setPhase({ kind: "idle" });
              setWay(v as typeof way);
            }}
            items={ways.map((w) => ({ id: w, label: t(`plugins.method.${w}` as MessageKey) }))}
          />
        )}
        {phase.kind === "failed" && (
          <div className="flex flex-col gap-2" data-testid="connect-error">
            <Banner tone="error">
              {phase.step === "timeout"
                ? t("plugins.connect.timeout")
                : t("plugins.connect.failed", { step: t(`plugins.step.${phase.step}` as MessageKey) })}
            </Banner>
            {phase.error && phase.step !== "timeout" && (
              <CodeBlock code={phase.error} copyLabel={t("thread.copy")} copiedLabel={t("thread.copied")} />
            )}
          </div>
        )}
        {way === "oauth" && (
          <ol className="flex flex-col gap-2" aria-label={t("plugins.connect.steps")}>
            {STEPS.map((s, i) => {
              const st = steps[s];
              const timedOut = phase.kind === "failed" && phase.step === "timeout" && s === "sign_in";
              const state = timedOut ? "failed" : st;
              return (
                <li
                  key={s}
                  data-testid={`step-${s}`}
                  data-state={state ?? "pending"}
                  className="flex items-center gap-2 text-sm"
                >
                  <StepIcon state={state} />
                  <span
                    className={cx(state === "failed" ? "text-error" : state ? "text-fg" : "text-fg-muted")}
                  >
                    {t("plugins.connect.stepN", { n: i + 1, step: t(`plugins.step.${s}` as MessageKey) })}
                  </span>
                  {s === "sign_in" && state === "active" && (
                    <span className="text-fg-subtle">{t("plugins.connect.waiting")}</span>
                  )}
                </li>
              );
            })}
          </ol>
        )}
        {way === "key" && (
          <div className="flex flex-col gap-2">
            <SecretInput
              aria-label={t("plugins.connect.apiKey")}
              placeholder={t("plugins.connect.apiKey")}
              value={key}
              onChange={(e) => setKey(e.target.value)}
              data-testid="connect-key-input"
              showLabel={t("plugins.connect.show")}
              hideLabel2={t("plugins.connect.hide")}
            />
            {card.keyHint && <p className="text-sm text-fg-muted">{card.keyHint}</p>}
          </div>
        )}
        {way === "none" && <p className="text-sm text-fg-muted">{t("plugins.connect.noSignIn")}</p>}
        <p className="text-sm text-fg-subtle">{t("plugins.keychainNote")}</p>
      </div>
    </Sheet>
  );
}

function StepIcon({ state }: { state?: StepState }) {
  if (state === "done") return <Check size={14} aria-hidden="true" className="text-done-text" />;
  if (state === "failed") return <X size={14} aria-hidden="true" className="text-error" />;
  if (state === "active")
    return <Loader2 size={14} aria-hidden="true" className="animate-spin text-fg-muted" />;
  return <Circle size={14} aria-hidden="true" className="text-fg-subtle" />;
}

/** "Connected as {account}" → choose teammates (none checked) → Done. */
function ChooseTeammates({ card, onDone }: { card: PluginCard; onDone: () => void }) {
  const [teammates, setTeammates] = useState<TeammateSummary[] | null>(null);
  const [picked, setPicked] = useState<string[]>([]);
  useEffect(() => {
    void invoke<TeammateSummary[]>("teammate_list").then(setTeammates, () => setTeammates([]));
  }, []);
  const done = async () => {
    for (const id of picked) {
      await invoke("grants_set", {
        subject: { type: "teammate", teammateId: id },
        connectionId: card.connectionId,
        granted: true,
      }).catch(() => undefined);
    }
    onDone();
  };
  return (
    <div className="flex flex-col gap-3" data-testid="connect-done">
      <p className="text-base text-fg" role="status">
        {card.accountLabel
          ? t("plugins.connect.connectedAs", { account: card.accountLabel })
          : t("plugins.connect.connected")}
      </p>
      <p className="text-sm font-medium text-fg">{t("plugins.connect.chooseTeammates")}</p>
      {teammates === null ? null : teammates.length === 0 ? (
        <p className="text-sm text-fg-muted">{t("plugins.connect.noTeammates")}</p>
      ) : (
        <div className="flex flex-col gap-1">
          {teammates.map((s) => (
            <Checkbox
              key={s.teammate.id}
              label={s.teammate.name}
              checked={picked.includes(s.teammate.id)}
              onCheckedChange={(v) =>
                setPicked((p) => (v ? [...p, s.teammate.id] : p.filter((x) => x !== s.teammate.id)))
              }
            />
          ))}
        </div>
      )}
      <p className="text-sm text-fg-muted">{t("plugins.connect.changeLater")}</p>
      <div>
        <Button variant="primary" onClick={() => void done()} data-testid="connect-finish">
          {t("plugins.connect.done")}
        </Button>
      </div>
    </div>
  );
}
