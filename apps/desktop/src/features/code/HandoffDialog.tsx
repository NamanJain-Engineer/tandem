import { useEffect, useMemo, useState } from "react";
import type { BriefSource, HandoffPlace, IpcError } from "@tandem/protocol";
import {
  Button,
  Callout,
  Checkbox,
  Dialog,
  Select,
  Skeleton,
  SkeletonGate,
  Textarea,
  useToast,
  type SelectOption,
} from "@tandem/ui";
import { t } from "../../i18n";
import { useEngines } from "../../stores/engines";
import { useSession } from "../../stores/session";
import {
  defaultTarget,
  generateBrief,
  startHandoff,
  startThreadHandoff,
  useHandoff,
  type HandoffSource,
} from "../../stores/handoff";
import { isRepo } from "../../stores/launch";
import { isWorktree } from "../../stores/worktrees";
import { useWorkspaces } from "../../stores/workspaces";

type Summary = { state: "loading" } | { state: "ready"; source: BriefSource } | { state: "failed" };

function HandoffBody({ source }: { source: HandoffSource }) {
  const engines = useEngines((s) => s.list);
  const ws = useWorkspaces((s) => s.list.find((w) => w.id === source.workspaceId));
  const label = useEngines((s) => s.label);
  const { show } = useToast();
  const [engineId, setEngineId] = useState<string | null>(() => defaultTarget(engines, source.engineId));
  // A pane beside the source, a new worktree, or a Thread (chat engines).
  const [where, setWhere] = useState<HandoffPlace | "thread">("same");
  const [text, setText] = useState("");
  const [summary, setSummary] = useState<Summary>({ state: "loading" });
  const [closeSource, setCloseSource] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<IpcError | null>(null);
  const close = () => useHandoff.getState().close();

  // The summary is written once, on open, for the first target (UI-13).
  const [first] = useState(engineId);
  useEffect(() => {
    if (!first) {
      setSummary({ state: "failed" });
      return;
    }
    let live = true;
    void generateBrief(source.paneId, first).then((out) => {
      if (!live) return;
      if (out.ok) {
        setText(out.value.text);
        setSummary({ state: "ready", source: out.value.source });
      } else setSummary({ state: "failed" });
    });
    return () => {
      live = false;
    };
  }, [source.paneId, first]);

  const targets: SelectOption[] = useMemo(
    () =>
      engines
        .filter((e) => e.install !== null && e.unsupportedReason === null)
        .map((e) => ({ value: e.spec.id, label: e.spec.label })),
    [engines],
  );
  const target = (engineId && label(engineId)) ?? "";
  const current = source.kind === "agent" ? (label(source.engineId) ?? t("handoff.thisAgent")) : null;
  const inWorktree = ws ? isWorktree(ws) : false;
  const chatEngines = useSession((s) => s.entitlement?.limits.chatEngines ?? null);
  // Free threads run Claude Code and Codex only (PRODUCT_SPEC §3.2).
  const onPlan = chatEngines === null || (engineId !== null && chatEngines.includes(engineId));
  const chatCapable = engines.some((e) => e.spec.id === engineId && e.chatCapable) && onPlan;
  const place = where === "thread" && !chatCapable ? "same" : where;
  const places: SelectOption[] = [
    { value: "same", label: inWorktree ? t("handoff.sameWorktree") : t("handoff.sameFolder") },
    {
      value: "new_worktree",
      label:
        ws?.kind === "wsl"
          ? t("handoff.newWorktreeRemote")
          : isRepo(ws)
            ? t("handoff.newWorktree")
            : t("handoff.newWorktreeNeedsRepo"),
      disabled: !isRepo(ws) || ws?.kind === "wsl",
    },
    {
      value: "thread",
      label: chatCapable
        ? t("handoff.thread")
        : onPlan
          ? t("handoff.threadUnsupported", { target })
          : t("handoff.threadPro", { target }),
      disabled: !chatCapable,
    },
  ];
  const loading = summary.state === "loading";
  const ready = Boolean(engineId) && text.trim().length > 0 && !loading && !busy;

  const start = async () => {
    if (!engineId || !ready) return;
    setBusy(true);
    setError(null);
    if (place === "thread") {
      const out = await startThreadHandoff(source, engineId, text, closeSource);
      setBusy(false);
      if (!out.ok) return setError(out.error);
      if (!out.value.sourceClosed)
        useHandoff.getState().note(source.paneId, t("handoff.noteThread", { engine: target }));
      show({ tone: "success", title: t("handoff.startedThread", { engine: target }) });
      return close();
    }
    const out = await startHandoff(source, engineId, text, place, closeSource);
    setBusy(false);
    if (!out.ok) return setError(out.error);
    const name = out.value.pane.humanName ?? target;
    if (!out.value.sourceClosed)
      useHandoff.getState().note(source.paneId, t("handoff.note", { name, engine: target }));
    show({ tone: "success", title: t("handoff.started", { name }) });
    close();
  };

  return (
    <Dialog
      open
      size="md"
      onOpenChange={(o) => !o && close()}
      title={t("handoff.title")}
      closeLabel={t("common.close")}
      footer={
        <>
          <Button variant="secondary" onClick={close}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            disabled={!ready}
            loading={busy}
            onClick={() => void start()}
            data-testid="handoff-start"
          >
            {t("handoff.start", { target })}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4" data-testid="handoff-dialog">
        <p className="text-base text-fg-muted">
          {current ? t("handoff.body", { current, target }) : t("handoff.bodyTerminal", { target })}
        </p>
        {targets.length === 0 && <Callout tone="warning">{t("launch.noEngines")}</Callout>}
        <div className="grid grid-cols-2 gap-3">
          <Select
            label={t("handoff.target")}
            value={engineId ?? ""}
            onValueChange={setEngineId}
            options={targets}
          />
          <Select
            label={t("handoff.where")}
            value={place}
            onValueChange={(v) => setWhere(v as HandoffPlace | "thread")}
            options={places}
          />
        </div>
        <SkeletonGate
          loading={loading}
          label={t("handoff.writing")}
          skeleton={
            <div className="flex flex-col gap-2" data-testid="handoff-writing">
              <span className="text-sm text-fg-muted">{t("handoff.writing")}</span>
              <Skeleton height={14} width="60%" />
              <Skeleton height={14} />
              <Skeleton height={14} width="80%" />
            </div>
          }
        >
          {summary.state === "failed" && <Callout tone="warning">{t("handoff.failed")}</Callout>}
          <Textarea
            label={t("handoff.summary")}
            value={text}
            minRows={8}
            maxRows={16}
            onChange={(e) => setText(e.target.value)}
            data-testid="handoff-summary"
          />
          {summary.state === "ready" && summary.source === "fallback" && (
            <p className="text-sm text-fg-subtle" data-testid="handoff-fallback">
              {t("handoff.fallbackNote")}
            </p>
          )}
        </SkeletonGate>
        <Checkbox label={t("handoff.closeSource")} checked={closeSource} onCheckedChange={setCloseSource} />
        {error && <Callout tone="error">{error.message}</Callout>}
      </div>
    </Dialog>
  );
}

/** UI-13 "Hand off to another agent" (F-CD-16), from a pane's ⋯ menu or a
 * Dashboard card. */
export function HandoffDialog() {
  const source = useHandoff((s) => s.source);
  // The target list and default need the engines; after a restore nothing
  // else may have loaded them yet.
  const loaded = useEngines((s) => s.loaded);
  useEffect(() => {
    if (source && !loaded) void useEngines.getState().refresh();
  }, [source, loaded]);
  return source && loaded ? <HandoffBody key={source.paneId} source={source} /> : null;
}
