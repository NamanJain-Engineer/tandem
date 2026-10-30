import { useCallback, useEffect, useState } from "react";
import type { SetupState, WorkspaceSummary } from "@tandem/protocol";
import { Banner, Button, CodeBlock, Dialog, Input, Skeleton, Textarea, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { useSteadyBox } from "../secrets/ApprovalGuard";
import {
  formatEnv,
  getWorktreeSetup,
  parseEnv,
  patternLines,
  saveWorktreeSetup,
  secretLooking,
  trustAndRun,
  useWorktrees,
} from "../../stores/worktrees";

/** "" or 1–100 (F-WT-06). */
function parsePorts(text: string): { ports: number | null; ok: boolean } {
  const v = text.trim();
  if (!v) return { ports: null, ok: true };
  const n = Number(v);
  return Number.isInteger(n) && n >= 1 && n <= 100 ? { ports: n, ok: true } : { ports: null, ok: false };
}

type Load = { at: "loading" } | { at: "error"; message: string } | { at: "ready"; state: SetupState };

/** UI-150 "Worktree setup": the repository's `.vibezeo/worktree.json`
 * (F-WT-01). Opened from a repository's or a worktree's menu. */
export function WorktreeSetupDialog({ ws, onClose }: { ws: WorkspaceSummary; onClose: () => void }) {
  const [load, setLoad] = useState<Load>({ at: "loading" });
  const [copy, setCopy] = useState("");
  const [setup, setSetup] = useState("");
  const [teardown, setTeardown] = useState("");
  const [ports, setPorts] = useState("");
  const [envText, setEnvText] = useState("");
  const [secrets, setSecrets] = useState<string[]>([]);
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
      setPorts(config?.ports ? String(config.ports) : "");
      setEnvText(formatEnv(config?.env));
      setLoad({ at: "ready", state: out.value });
    });
  }, [ws.id]);
  useEffect(() => fetch(), [fetch]);

  const parsedEnv = parseEnv(envText);
  const parsedPorts = parsePorts(ports);
  // F-WT-06 AC6: values that look like secrets, checked by the core's
  // redactor as they're typed.
  const envKey = JSON.stringify(parsedEnv.env);
  useEffect(() => {
    let live = true;
    const timer = setTimeout(() => {
      void secretLooking(JSON.parse(envKey) as Record<string, string>).then((names) => {
        if (live) setSecrets(names);
      });
    }, 300);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [envKey]);

  const save = async () => {
    if (parsedEnv.badLine !== null || !parsedPorts.ok) return;
    setBusy(true);
    setError(null);
    // The dialog doesn't edit the preview (F-WT-09): saving keeps the
    // file's, which the core would otherwise write out (D-861).
    const preview = load.at === "ready" ? load.state.config?.preview : null;
    const out = await saveWorktreeSetup(ws.id, {
      copy: patternLines(copy),
      setup: setup.trim() || null,
      teardown: teardown.trim() || null,
      ports: parsedPorts.ports,
      env: parsedEnv.env,
      ...(preview ? { preview } : {}),
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
            disabled={load.at !== "ready" || parsedEnv.badLine !== null || !parsedPorts.ok}
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
              data-testid="worktree-setup-teardown"
            />
            <Input
              label={t("worktree.runtime.portsLabel")}
              hint={t("worktree.runtime.portsHint")}
              inputMode="numeric"
              value={ports}
              onChange={(e) => setPorts(e.target.value)}
              error={parsedPorts.ok ? undefined : t("worktree.runtime.portsBad")}
              data-testid="worktree-setup-ports"
            />
            <Textarea
              label={t("worktree.runtime.envLabel")}
              hint={secrets.length > 0 ? undefined : t("worktree.runtime.envHint")}
              monospace
              minRows={2}
              value={envText}
              onChange={(e) => setEnvText(e.target.value)}
              error={
                parsedEnv.badLine !== null
                  ? t("worktree.runtime.envBadLine", { line: parsedEnv.badLine })
                  : (error ?? undefined)
              }
              data-testid="worktree-setup-env"
            />
            {secrets.length > 0 && parsedEnv.badLine === null ? (
              <p data-testid="worktree-setup-secret" className="text-sm text-warning">
                {t("worktree.runtime.secret", { names: secrets.join(", ") })}
              </p>
            ) : null}
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
  after,
  onClose,
}: {
  worktreeIds: string[];
  state: SetupState;
  /** F-WT-09: the review came from Start preview. */
  after?: "preview";
  onClose: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const config = state.config;
  // It opens on its own after Launch, Retry or New worktree, maybe while
  // the person is clicking in the new panes, and one click trusts the
  // repository's setup for later worktrees too: Trust and run waits until
  // the dialog has held still, uncovered (F-TL-12.12).
  const box = useSteadyBox(!!state.digest, state.digest ? `${state.digest}\n${worktreeIds.join(",")}` : null);
  const trust = async () => {
    if (!state.digest) return;
    setBusy(true);
    setError(null);
    const out = await trustAndRun(worktreeIds, state.digest, after);
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
        <span ref={box.ref} className="flex gap-2">
          <Button variant="secondary" onClick={onClose} data-testid="worktree-review-later">
            {t("worktree.setup.notNow")}
          </Button>
          <Button
            variant="primary"
            loading={busy}
            disabled={!state.digest}
            waiting={!box.steady}
            title={box.steady ? undefined : t("approval.waitingTrustRun")}
            onClick={() => box.check() && void trust()}
            data-grant
            data-testid="worktree-review-trust"
          >
            {t("worktree.setup.trustRun")}
          </Button>
        </span>
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
        {config?.preview ? (
          <section className="flex flex-col gap-1">
            <h3 className="text-sm font-medium text-fg">{t("worktree.setup.reviewPreview")}</h3>
            <CodeBlock code={config.preview.command} language="bash" />
          </section>
        ) : null}
        {config && Object.keys(config.env ?? {}).length > 0 ? (
          <section className="flex flex-col gap-1">
            <h3 className="text-sm font-medium text-fg">{t("worktree.setup.reviewEnv")}</h3>
            <CodeBlock code={formatEnv(config.env)} language="bash" />
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
