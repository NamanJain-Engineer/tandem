import { useCallback, useEffect, useRef, useState } from "react";
import type { RiskClass, HeldDraft, IpcError, ToolResultView } from "@tandem/protocol";
import {
  offscreenRow,
  Badge,
  Banner,
  Button,
  Checkbox,
  CodeBlock,
  CodeEditor,
  Dialog,
  EmptyState,
  EngineGlyph,
  Skeleton,
  useToast,
} from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { listen } from "../../ipc/events";
import { useSteadyBox } from "../secrets/ApprovalGuard";
import { riskLabel, riskVariant } from "./model";

/** F-PL-08 "Held actions": what unattended runs wanted to do, waiting for
 * the user. Approve & run, Edit & run (checked against the tool's schema),
 * Discard, or approve or discard the selected ones together. */
export function HeldActions({ originId, compact = false }: { originId?: string; compact?: boolean }) {
  const [drafts, setDrafts] = useState<HeldDraft[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [picked, setPicked] = useState<string[]>([]);
  const [editing, setEditing] = useState<HeldDraft | null>(null);
  const [busy, setBusy] = useState(false);
  // Approve & run waits until its place has held still, like Allow
  // (F-TL-12.12): a row above that runs or goes can't slide another under
  // the pointer.
  const selectedBox = useSteadyBox(picked.length > 0);
  const { show } = useToast();
  const load = useCallback(() => {
    void invoke<HeldDraft[]>("held_drafts_list", { state: "held", originId: originId ?? null }).then(
      (d) => {
        setDrafts(d);
        setError(null);
        setPicked((p) => p.filter((id) => d.some((x) => x.id === id)));
      },
      (e: IpcError) => setError(e.message),
    );
  }, [originId]);
  useEffect(() => {
    load();
    let off: (() => void) | undefined;
    let gone = false;
    void listen("held:changed", load).then((u) => (gone ? u() : (off = u)));
    return () => {
      gone = true;
      off?.();
    };
  }, [load]);

  const run = async (d: HeldDraft, editedArgs?: unknown) => {
    const r = await invoke<ToolResultView>("held_draft_run", {
      draftId: d.id,
      editedArgs: editedArgs ?? null,
    });
    show({
      tone: r.isError ? "error" : "success",
      title: r.isError
        ? t("held.failed", { id: `D-${d.displayNo}` })
        : t("held.ran", { id: `D-${d.displayNo}` }),
    });
  };
  const act = async (f: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await f();
    } catch (e) {
      show({ tone: "error", title: (e as IpcError).message });
    } finally {
      setBusy(false);
      load();
    }
  };

  if (error) {
    return (
      <Banner tone="error" action={{ label: t("plugins.retry"), onClick: load }}>
        {error}
      </Banner>
    );
  }
  if (!drafts) return <Skeleton height={64} />;
  if (drafts.length === 0) {
    return compact ? null : <EmptyState title={t("held.empty")} body={t("held.emptyBody")} />;
  }
  const chosen = drafts.filter((d) => picked.includes(d.id));
  return (
    <section className="flex flex-col gap-2" aria-label={t("held.title")} data-testid="held-actions">
      <div className="flex items-center gap-2">
        <h3 className="text-base font-medium text-fg">{t("held.titleN", { n: drafts.length })}</h3>
        <span className="flex-1" />
        <span ref={selectedBox.ref} className="flex">
          <Button
            size="sm"
            variant="secondary"
            disabled={busy || chosen.length === 0}
            waiting={!selectedBox.steady}
            title={chosen.length > 0 && !selectedBox.steady ? t("approval.waitingApprove") : undefined}
            onClick={() =>
              selectedBox.check() &&
              void act(async () => {
                for (const d of chosen) await run(d);
              })
            }
            data-grant
            data-testid="held-run-selected"
          >
            {t("held.runSelected", { n: chosen.length })}
          </Button>
        </span>
        <Button
          size="sm"
          variant="ghost"
          disabled={busy || chosen.length === 0}
          onClick={() => void act(() => invoke("held_drafts_discard", { draftIds: chosen.map((d) => d.id) }))}
          data-testid="held-discard-selected"
        >
          {t("held.discardSelected", { n: chosen.length })}
        </Button>
      </div>
      <ul className="flex flex-col gap-2">
        {drafts.map((d) => (
          <HeldRow
            key={d.id}
            draft={d}
            busy={busy}
            picked={picked.includes(d.id)}
            onPick={(v) => setPicked((p) => (v ? [...p, d.id] : p.filter((x) => x !== d.id)))}
            onRun={() => void act(() => run(d))}
            onEdit={() => setEditing(d)}
            onDiscard={() => void act(() => invoke("held_drafts_discard", { draftIds: [d.id] }))}
          />
        ))}
      </ul>
      {editing && (
        <EditAndRun
          draft={editing}
          onClose={() => setEditing(null)}
          onRun={async (args) => {
            await run(editing, args);
            setEditing(null);
            load();
          }}
        />
      )}
    </section>
  );
}

/** One held draft. Its Approve & run waits for the row to hold still. */
function HeldRow({
  draft: d,
  busy,
  picked,
  onPick,
  onRun,
  onEdit,
  onDiscard,
}: {
  draft: HeldDraft;
  busy: boolean;
  picked: boolean;
  onPick: (v: boolean) => void;
  onRun: () => void;
  onEdit: () => void;
  onDiscard: () => void;
}) {
  // Two boxes, one key: the pick and the buttons each wait on their own
  // place, so a row taller than the list still works once each is scrolled
  // into view. A draft arriving on top moves both, and both wait again (a
  // pick feeds Approve & run {n}).
  const key = `${d.id}\n${JSON.stringify(d.args)}`;
  const pickBox = useSteadyBox(true, key);
  const box = useSteadyBox(true, key);
  return (
    <li
      data-testid={`held-${d.displayNo}`}
      // Off screen, a row skips layout, paint and hit tests (up to 500 drafts).
      style={offscreenRow(220)}
      className="flex flex-col gap-2 rounded-md border border-border bg-surface-2 p-3"
    >
      <div className="flex items-center gap-2">
        {/* Unpicking never waits; picking does. */}
        <span ref={pickBox.ref} data-grant className="flex">
          <Checkbox
            label={<span className="sr-only">{t("held.select", { id: `D-${d.displayNo}` })}</span>}
            checked={picked}
            waiting={!picked && !pickBox.steady}
            waitingReason={t("approval.waitingPick")}
            onCheckedChange={(v) => (v ? pickBox.check() && onPick(true) : onPick(false))}
          />
        </span>
        <EngineGlyph letters={d.logo} size={16} />
        <span className="text-sm font-medium text-fg">{t("held.id", { n: d.displayNo })}</span>
        <span className="min-w-0 flex-1 truncate text-sm text-fg">
          {t("held.action", { plugin: d.pluginLabel, action: d.action })}
        </span>
        <Badge variant={riskVariant(d.riskClass as RiskClass)}>{riskLabel(d.riskClass as RiskClass)}</Badge>
      </div>
      <p className="text-sm text-fg-muted">{t(`held.from.${d.originKind}`, { id: d.originId })}</p>
      <CodeBlock
        code={JSON.stringify(d.args, null, 2)}
        copyLabel={t("thread.copy")}
        copiedLabel={t("thread.copied")}
        // Long args scroll within the row.
        className="[&_pre]:max-h-64 [&_pre]:overflow-y-auto"
      />
      <div ref={box.ref} className="flex gap-2">
        <Button
          size="sm"
          variant="primary"
          disabled={busy}
          waiting={!box.steady}
          title={box.steady ? undefined : t("approval.waitingApprove")}
          onClick={() => box.check() && onRun()}
          data-grant
          data-testid={`held-run-${d.displayNo}`}
        >
          {t("held.approveRun")}
        </Button>
        <Button size="sm" variant="secondary" disabled={busy} onClick={onEdit}>
          {t("held.editRun")}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          disabled={busy}
          onClick={onDiscard}
          data-testid={`held-discard-${d.displayNo}`}
        >
          {t("held.discard")}
        </Button>
      </div>
    </li>
  );
}

/** Edit & run: a JSON editor; the core checks the result against the
 * tool's input schema before anything runs. */
function EditAndRun({
  draft,
  onClose,
  onRun,
}: {
  draft: HeldDraft;
  onClose: () => void;
  onRun: (args: unknown) => Promise<void>;
}) {
  const [text, setText] = useState(() => JSON.stringify(draft.args, null, 2));
  const [problem, setProblem] = useState<string | null>(null);
  // Run starts the tool with these arguments, so it waits like Allow until
  // the dialog has held still (F-TL-12 AC 18): an error shown above moves
  // it. Keyed by the draft, not the text, so typing doesn't restart it.
  const box = useSteadyBox(true, draft.id);
  // One run at a time: a double-click on a steady Run sent the draft twice
  // (the review of ce0d5815). A ref, so the second click in the same frame
  // sees the first.
  const running = useRef(false);
  const [busy, setBusy] = useState(false);
  const submit = () => {
    if (running.current) return;
    let parsed: unknown;
    try {
      parsed = JSON.parse(text);
    } catch {
      setProblem(t("held.badJson"));
      return;
    }
    setProblem(null);
    running.current = true;
    setBusy(true);
    // A schema problem comes back as the error and nothing runs.
    onRun(parsed)
      .catch((e: IpcError) => setProblem(e.message))
      .finally(() => {
        running.current = false;
        setBusy(false);
      });
  };
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("held.editTitle", { id: `D-${draft.displayNo}` })}
      size="lg"
      footer={
        <span ref={box.ref} className="flex items-center gap-2">
          <Button variant="secondary" onClick={onClose}>
            {t("plugins.connect.cancel")}
          </Button>
          <Button
            variant="primary"
            data-grant
            loading={busy}
            waiting={!box.steady}
            title={box.steady ? undefined : t("approval.waitingRun")}
            onClick={() => {
              if (box.check()) submit();
            }}
            data-testid="held-edit-run"
          >
            {t("held.runEdited")}
          </Button>
        </span>
      }
    >
      <div className="flex flex-col gap-2">
        {problem && (
          <Banner tone="error">
            <span data-testid="held-edit-error">{problem}</span>
          </Banner>
        )}
        <div className="h-64 overflow-hidden rounded-sm border border-border">
          <CodeEditor
            value={text}
            path="arguments.json"
            docKey={`held:${draft.id}`}
            label={t("held.editTitle", { id: `D-${draft.displayNo}` })}
            onChange={setText}
          />
        </div>
      </div>
    </Dialog>
  );
}
