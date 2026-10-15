import { useCallback, useEffect, useState } from "react";
import type { InstructionsFile, InstructionsPlan, InstructionsReport, WorkspaceSummary } from "@tandem/protocol";
import { Badge, Banner, Button, CodeBlock, Dialog, DiffView, EmptyState, Select, Skeleton, useToast } from "@tandem/ui";
import { FileText } from "lucide-react";
import { t } from "../../i18n";
import {
  AGENTS,
  applyInstructions,
  copyInstructionsToWorktrees,
  existing,
  getInstructions,
  planInstructions,
  readsSummary,
  sources,
  undoInstructions,
} from "../../stores/instructions";

type Load =
  | { at: "loading" }
  | { at: "error"; message: string }
  | { at: "ready"; report: InstructionsReport }
  | { at: "plan"; report: InstructionsReport; plan: InstructionsPlan; source: string | null };

function gitLabel(f: InstructionsFile): string {
  if (f.git === "ignored") return t("instructions.git.ignored");
  if (f.git === "untracked") return t("instructions.git.untracked");
  if (f.git === "tracked") return t("instructions.git.tracked");
  return "";
}

function unsafeLabel(f: InstructionsFile): string | null {
  if (!f.unsafe) return null;
  return t(`instructions.unsafe.${f.unsafe}`);
}

/** UI-160 "Instructions" (F-EN-07): which instructions file each installed
 * engine reads in this repository, how they differ from AGENTS.md, and "Use
 * one file for all". Opened from a repository's menu. */
export function InstructionsDialog({ ws, onClose }: { ws: WorkspaceSummary; onClose: () => void }) {
  const [load, setLoad] = useState<Load>({ at: "loading" });
  const [source, setSource] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const { show } = useToast();

  const fetch = useCallback(() => {
    setLoad({ at: "loading" });
    setError(null);
    void getInstructions(ws.id).then((out) =>
      setLoad(out.ok ? { at: "ready", report: out.value } : { at: "error", message: out.error.message }),
    );
  }, [ws.id]);
  useEffect(() => fetch(), [fetch]);

  const report = load.at === "ready" || load.at === "plan" ? load.report : null;
  const files = report ? existing(report.files) : [];
  const agentsExists = files.some((f) => f.path === AGENTS);
  const choices = report ? sources(report.files) : [];
  const chosen = agentsExists ? null : (source ?? choices[0]?.path ?? null);

  const makePlan = async () => {
    if (!report) return;
    setBusy(true);
    setError(null);
    const out = await planInstructions(ws.id, chosen);
    setBusy(false);
    if (!out.ok) setError(out.error.message);
    else setLoad({ at: "plan", report, plan: out.value, source: chosen });
  };

  const confirm = async () => {
    if (load.at !== "plan") return;
    setBusy(true);
    setError(null);
    const out = await applyInstructions(ws.id, load.source, load.plan.digest);
    setBusy(false);
    if (!out.ok) {
      setError(out.error.message);
      return;
    }
    const changeId = out.value.changeId;
    show({
      tone: "success",
      title: t("instructions.applied", { n: out.value.written.length }),
      action: {
        label: t("instructions.undo"),
        onClick: () =>
          void undoInstructions(changeId).then((u) => {
            if (!u.ok) show({ tone: "error", title: u.error.message });
            else show({ tone: "success", title: t("instructions.undone") });
            fetch();
          }),
      },
    });
    fetch();
  };

  const copyIn = async (path: string) => {
    const out = await copyInstructionsToWorktrees(ws.id, path);
    if (!out.ok) show({ tone: "error", title: out.error.message });
    else show({ tone: "success", title: t("instructions.copied", { name: path }) });
    fetch();
  };

  const planning = load.at === "plan" ? load.plan : null;
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("instructions.title")}
      description={t("instructions.description", { folder: ws.name })}
      closeLabel={t("common.close")}
      size="lg"
      footer={
        planning ? (
          <>
            <Button variant="secondary" onClick={() => report && setLoad({ at: "ready", report })}>
              {t("instructions.back")}
            </Button>
            <Button
              variant="primary"
              loading={busy}
              disabled={planning.changes.length === 0}
              onClick={() => void confirm()}
              data-testid="instructions-confirm"
            >
              {t("instructions.confirm", { n: planning.changes.length })}
            </Button>
          </>
        ) : (
          <>
            <Button variant="secondary" onClick={onClose}>
              {t("common.close")}
            </Button>
            <Button
              variant="primary"
              loading={busy}
              disabled={!report || (!agentsExists && !chosen)}
              onClick={() => void makePlan()}
              data-testid="instructions-one-file"
            >
              {t("instructions.oneFile")}
            </Button>
          </>
        )
      }
    >
      <div data-testid="instructions" className="flex flex-col gap-3">
        {error && <Banner tone="error">{error}</Banner>}
        {load.at === "loading" ? (
          <div className="flex flex-col gap-2" aria-busy="true" aria-label={t("instructions.loading")}>
            <Skeleton height={56} />
            <Skeleton height={32} />
            <Skeleton height={32} />
          </div>
        ) : load.at === "error" ? (
          <Banner tone="error" action={{ label: t("instructions.retry"), onClick: fetch }}>
            {t("instructions.loadFailed", { reason: load.message })}
          </Banner>
        ) : planning ? (
          <section className="flex flex-col gap-2" data-testid="instructions-plan">
            <p className="text-sm text-fg-muted">{t("instructions.planIntro")}</p>
            {planning.changes.length === 0 && (
              <p className="text-sm text-fg-subtle" data-testid="instructions-plan-empty">
                {t("instructions.planEmpty")}
              </p>
            )}
            <ul className="flex flex-col gap-2">
              {planning.changes.map((c) => (
                <li
                  key={c.path}
                  data-testid="instructions-change"
                  data-path={c.path}
                  className="flex flex-col gap-1.5 rounded-md border border-border bg-surface-2 p-3"
                >
                  <span className="flex items-center gap-2">
                    <span className="font-mono text-base text-fg">{c.path}</span>
                    <Badge variant={c.action === "create" ? "accent" : "neutral"}>
                      {c.action === "create" ? t("instructions.create") : t("instructions.replace")}
                    </Badge>
                  </span>
                  {c.textLost && (
                    <span className="text-sm text-warning" role="status" data-testid="instructions-text-lost">
                      {t("instructions.textLost")}
                    </span>
                  )}
                  <CodeBlock code={c.after} language="markdown" />
                  {c.before !== null && c.before !== undefined && (
                    <details className="text-sm text-fg-muted">
                      <summary>{t("instructions.before")}</summary>
                      <CodeBlock code={c.before} language="markdown" />
                    </details>
                  )}
                </li>
              ))}
            </ul>
            {planning.skipped.map((s) => (
              <p key={s.path} className="text-sm text-fg-muted" data-testid="instructions-skipped">
                {t("instructions.skipped", { name: s.path, reason: s.reason })}
              </p>
            ))}
          </section>
        ) : report && files.length === 0 && report.engines.length === 0 ? (
          <EmptyState
            icon={<FileText size={20} />}
            title={t("instructions.emptyTitle")}
            body={t("instructions.emptyBody")}
          />
        ) : report ? (
          <>
            <section className="flex flex-col gap-1.5">
              <h3 className="text-2xs font-semibold uppercase tracking-caps text-fg-subtle">
                {t("instructions.engines")}
              </h3>
              {report.engines.length === 0 && (
                <p className="text-sm text-fg-subtle">{t("instructions.noEngines")}</p>
              )}
              <ul className="flex flex-col rounded-md border border-border bg-surface-2">
                {report.engines.map((e) => {
                  const s = readsSummary(e, agentsExists);
                  const text =
                    s.kind === "unknown"
                      ? t("instructions.reads.unknown")
                      : s.kind === "nothing"
                        ? t("instructions.reads.nothing")
                        : s.kind === "agents"
                          ? s.others.length
                            ? t("instructions.reads.agentsAnd", { files: s.others.join(", ") })
                            : t("instructions.reads.agents")
                          : s.notAgents
                            ? t("instructions.reads.otherNotAgents", { files: s.files.join(", ") })
                            : t("instructions.reads.other", { files: s.files.join(", ") });
                  return (
                    <li
                      key={e.engineId}
                      data-testid="instructions-engine"
                      data-engine={e.engineId}
                      data-reads-agents={e.readsAgents ? "true" : "false"}
                      className="flex items-center gap-3 border-b border-divider px-3 py-2 last:border-b-0"
                    >
                      <span className="min-w-0 flex-1 text-base text-fg">{e.label}</span>
                      <span className="text-sm text-fg-muted" data-testid="instructions-reads">
                        {text}
                      </span>
                    </li>
                  );
                })}
              </ul>
            </section>
            <section className="flex flex-col gap-1.5">
              <h3 className="text-2xs font-semibold uppercase tracking-caps text-fg-subtle">
                {t("instructions.files")}
              </h3>
              {files.length === 0 && <p className="text-sm text-fg-subtle">{t("instructions.noFiles")}</p>}
              <ul className="flex flex-col gap-1.5">
                {files.map((f) => (
                  <li
                    key={f.path}
                    data-testid="instructions-file"
                    data-path={f.path}
                    className="flex flex-col gap-1.5 rounded-md border border-border bg-surface-2 px-3 py-2"
                  >
                    <span className="flex flex-wrap items-center gap-2">
                      <span className="font-mono text-base text-fg">{f.path}</span>
                      {gitLabel(f) && <Badge variant="outline">{gitLabel(f)}</Badge>}
                      {f.pointsToAgents && <Badge variant="done">{t("instructions.pointer")}</Badge>}
                      {unsafeLabel(f) && <Badge variant="error">{unsafeLabel(f)}</Badge>}
                      {f.inWorktrees === false && (
                        <span className="flex items-center gap-2" data-testid="instructions-not-in-worktrees">
                          <span className="text-sm text-fg-muted">{t("instructions.notInWorktrees")}</span>
                          <Button size="sm" variant="secondary" onClick={() => void copyIn(f.path)}>
                            {t("instructions.copyIn")}
                          </Button>
                        </span>
                      )}
                      {f.diff && (
                        <Button
                          size="sm"
                          variant="ghost"
                          onClick={() => setOpen(open === f.path ? null : f.path)}
                          aria-expanded={open === f.path}
                          data-testid="instructions-diff-toggle"
                        >
                          {open === f.path ? t("instructions.hideDiff") : t("instructions.showDiff")}
                        </Button>
                      )}
                    </span>
                    {f.diff && open === f.path && <DiffView files={[f.diff]} />}
                  </li>
                ))}
              </ul>
            </section>
            {!agentsExists && choices.length > 0 && (
              <Select
                label={t("instructions.source")}
                value={chosen ?? ""}
                onValueChange={setSource}
                options={choices.map((c) => ({ value: c.path, label: c.path }))}
              />
            )}
          </>
        ) : null}
      </div>
    </Dialog>
  );
}
