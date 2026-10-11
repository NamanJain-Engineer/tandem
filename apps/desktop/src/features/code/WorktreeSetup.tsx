import { useCallback, useEffect, useState } from "react";
import type { SetupState, WorkspaceSummary } from "@tandem/protocol";
import { Banner, Button, CodeBlock, Dialog, Input, Skeleton, Textarea, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import {
  getWorktreeSetup,
  patternLines,
  saveWorktreeSetup,
  trustAndRun,
  useWorktrees,
} from "../../stores/worktrees";

type Load = { at: "loading" } | { at: "error"; message: string } | { at: "ready"; state: SetupState };

/** UI-150 "Worktree setup": the repository's `.vibezeo/worktree.json`
 * (F-WT-01). Opened from a repository's or a worktree's menu. */
export function WorktreeSetupDialog({ ws, onClose }: { ws: WorkspaceSummary; onClose: () => void }) {
  const [load, setLoad] = useState<Load>({ at: "loading" });
  const [copy, setCopy] = useState("");
  const [setup, setSetup] = useState("");
  const [teardown, setTeardown] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { show } = useToast();

  const fetch = useCallback(() => {
    setLoad({ at: "loading" });
    void getWorktreeSetup(ws.id).then((out) => {
      if (!out.ok) {
        setLoad({ at: "error", message: out.error.message });
        return;
      }
      const config = out.value.config;
      setCopy(config?.copy.join("\n") ?? "");
      setSetup(config?.setup ?? "");
      setTeardown(config?.teardown ?? "");
      setLoad({ at: "ready", state: out.value });
    });
  }, [ws.id]);
  useEffect(() => fetch(), [fetch]);

  const save = async () => {
    setBusy(true);
    setError(null);
    const out = await saveWorktreeSetup(ws.id, {
      copy: patternLines(copy),
      setup: setup.trim() || null,
      teardown: teardown.trim() || null,
    });
    setBusy(false);
    if (!out.ok) {
      setError(out.error.message);
      return;
    }
    show({ tone: "success", title: t("worktree.setup.saved") });
    onClose();
  };

  const state = load.at === "ready" ? load.state : null;
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("worktree.setup.title")}
      description={t("worktree.setup.description", { folder: ws.name })}
      closeLabel={t("common.close")}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            loading={busy}
            disabled={load.at !== "ready"}
            onClick={() => void save()}
            data-testid="worktree-setup-save"
          >
            {t("worktree.setup.save")}
          </Button>
        </>
      }
    >
      <div data-testid="worktree-setup" className="flex flex-col gap-3">
        {load.at === "loading" ? (
          <div className="flex flex-col gap-2" aria-busy="true">
            <Skeleton height={72} />
            <Skeleton height={32} />
            <Skeleton height={32} />
          </div>
        ) : load.at === "error" ? (
          <Banner tone="error" action={{ label: t("worktree.setup.retry"), onClick: fetch }}>
            {t("worktree.setup.loadFailed", { reason: load.message })}
          </Banner>
        ) : (
          <>
            {state?.error ? (
              <Banner tone="warning">{t("worktree.setup.invalidSave", { reason: state.error })}</Banner>
            ) : !state?.exists ? (
              <p data-testid="worktree-setup-empty" className="text-sm text-fg-subtle">
                {t("worktree.setup.empty")}
              </p>
            ) : null}
            <Textarea
              label={t("worktree.setup.copyLabel")}
              hint={t("worktree.setup.copyHint")}
              monospace
              minRows={3}
              value={copy}
              onChange={(e) => setCopy(e.target.value)}
              data-testid="worktree-setup-copy"
            />
            <Input
              label={t("worktree.setup.setupLabel")}
              hint={t("worktree.setup.setupHint")}
              value={setup}
              maxLength={4000}
              onChange={(e) => setSetup(e.target.value)}
              data-testid="worktree-setup-command"
            />
            <Input
              label={t("worktree.setup.teardownLabel")}
              hint={t("worktree.setup.teardownHint")}
              value={teardown}
              maxLength={4000}
              onChange={(e) => setTeardown(e.target.value)}
              error={error ?? undefined}
              data-testid="worktree-setup-teardown"
            />
            <p className="text-xs text-fg-subtle">{t("worktree.setup.trustNote")}</p>
          </>
        )}
      </div>
    </Dialog>
  );
}

/** UI-151 "Review worktree setup": commands from the repository's file that
 * aren't trusted on this computer yet. Nothing runs until "Trust and run". */
export function ReviewSetupDialog({
  worktreeIds,
  state,
  onClose,
}: {
  worktreeIds: string[];
  state: SetupState;
  onClose: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const config = state.config;
  const trust = async () => {
    if (!state.digest) return;
    setBusy(true);
    setError(null);
    const out = await trustAndRun(worktreeIds, state.digest);
    setBusy(false);
    if (out.ok) onClose();
    else setError(out.error.message);
  };
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("worktree.setup.reviewTitle")}
      description={t("worktree.setup.reviewBody", { n: worktreeIds.length })}
      closeLabel={t("common.close")}
      footer={
        <>
          <Button variant="secondary" onClick={onClose} data-testid="worktree-review-later">
            {t("worktree.setup.notNow")}
          </Button>
          <Button
            variant="primary"
            loading={busy}
            disabled={!state.digest}
            onClick={() => void trust()}
            data-testid="worktree-review-trust"
          >
            {t("worktree.setup.trustRun")}
          </Button>
        </>
      }
    >
      <div data-testid="worktree-review" className="flex flex-col gap-3">
        {config?.setup ? (
          <section className="flex flex-col gap-1">
            <h3 className="text-sm font-medium text-fg">{t("worktree.setup.reviewSetup")}</h3>
            <CodeBlock code={config.setup} language="bash" />
          </section>
        ) : null}
        {config?.teardown ? (
          <section className="flex flex-col gap-1">
            <h3 className="text-sm font-medium text-fg">{t("worktree.setup.reviewTeardown")}</h3>
            <CodeBlock code={config.teardown} language="bash" />
          </section>
        ) : null}
        {error ? <Banner tone="error">{error}</Banner> : null}
      </div>
    </Dialog>
  );
}

/** Shows toasts the worktree store raised outside React. */
export function useWorktreeNotices() {
  const { show } = useToast();
  const pending = useWorktrees((s) => s.notices.length);
  useEffect(() => {
    if (pending === 0) return;
    for (const n of useWorktrees.getState().takeNotices()) show(n);
  }, [pending, show]);
}
