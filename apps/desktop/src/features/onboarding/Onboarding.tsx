import { useEffect, useRef, useState } from "react";
import { Check, ChevronDown, ChevronRight, Folder, RefreshCw } from "lucide-react";
import type { Account, EngineView, IpcError, WorkspaceSummary } from "@tandem/protocol";
import { Button, EngineGlyph, Spinner, Stepper, useToast } from "@tandem/ui";
import { t, type MessageKey } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useAccounts } from "../../stores/accounts";
import { useEngines } from "../../stores/engines";
import { defaultEngine } from "../../stores/launch";
import { useLayout } from "../../stores/layout";
import { useOnboarding } from "../../stores/onboarding";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { CopyCommand, installCommand } from "../code/LaunchGrid";
import { launchFor } from "../code/PanePicker";
import { LoginSheet, type LoginRequest } from "../settings/AccountsPage";
import { addWorkspace } from "../shell/actions";
import { useBoot } from "../shell/useBoot";

/** The stepper's names, and each step's title (DESIGN §9.3). */
const STEP_LABELS: Record<string, MessageKey> = {
  engines: "onboarding.step.engines",
  workspace: "onboarding.step.workspace",
  finish: "onboarding.step.finish",
};
const TITLES: Record<string, MessageKey> = {
  engines: "onboarding.engines.title",
  workspace: "onboarding.workspace.title",
  finish: "onboarding.finish.title",
};

/** Engines Vibezeo keeps accounts apart for sign in here, each with its own
 * login in its provider home (F-AC-02, D-114); the rest sign in by
 * themselves. */
function signsInApart(e: EngineView): boolean {
  return e.spec.chatAdapter !== null && e.spec.homeStrategy !== "none";
}

function defaultAccount(accounts: Account[], engineId: string): Account | undefined {
  return accounts.find((a) => a.engineId === engineId && a.isDefault);
}

/** What the wizard knows about this computer's engines: found ones first,
 * each one's sign-in, and the default (D-2649): the user's pick, else the
 * first signed-in engine, else the launch grid's default. */
function useFirstRunEngines() {
  const list = useEngines((s) => s.list);
  const loaded = useEngines((s) => s.loaded);
  const accounts = useAccounts((s) => s.list);
  const accountsLoaded = useAccounts((s) => s.loaded);
  const signIn = useAccounts((s) => s.signIn);
  const picked = useOnboarding((s) => s.engine);
  useEffect(() => {
    if (!loaded) void useEngines.getState().refresh();
    void useAccounts.getState().load();
  }, [loaded]);
  useEffect(() => {
    const { signIn: known, check } = useAccounts.getState();
    for (const a of accounts) if (known[a.id] === undefined) void check(a.id);
  }, [accounts]);
  const found = list.filter((e) => e.install !== null);
  const missing = list.filter((e) => e.install === null);
  const launchable = found.filter((e) => e.unsupportedReason === null);
  const state = (e: EngineView): "signed_in" | "checking" | "signed_out" | "apart_unknown" => {
    if (!signsInApart(e)) return "apart_unknown";
    const account = defaultAccount(accounts, e.spec.id);
    if (!account) return accountsLoaded ? "signed_out" : "checking";
    const s = signIn[account.id];
    return s === "signed_in" ? "signed_in" : s === undefined ? "checking" : "signed_out";
  };
  const firstSignedIn = launchable.find((e) => state(e) === "signed_in")?.spec.id;
  const chosen =
    (picked && launchable.some((e) => e.spec.id === picked) ? picked : null) ??
    firstSignedIn ??
    defaultEngine(launchable);
  return { loaded, found, missing, launchable, state, chosen, accounts };
}

/** UI-04 step 1: the engines found, each with its sign-in; the rest folded. */
function EnginesStep() {
  const { loaded, found, missing, launchable, state, chosen, accounts } = useFirstRunEngines();
  const detecting = useEngines((s) => s.detecting);
  const platform = useBoot((s) => s.boot?.platform);
  const [login, setLogin] = useState<LoginRequest | null>(null);
  const [more, setMore] = useState(false);
  return (
    <div className="flex flex-col gap-6">
      <p className="text-md text-fg-muted">{t("onboarding.engines.body")}</p>
      {!loaded ? (
        <span className="flex items-center gap-2 text-sm text-fg-muted" role="status">
          <Spinner /> {t("onboarding.engines.checking")}
        </span>
      ) : found.length === 0 ? (
        <p
          className="rounded-md border border-border bg-surface-2 px-4 py-4 text-sm text-fg-muted shadow-e1"
          data-testid="onboarding-engines-empty"
        >
          {t("onboarding.engines.none")}
        </p>
      ) : (
        <ul
          className="flex flex-col divide-y divide-divider rounded-md border border-border bg-surface-2 shadow-e1"
          data-testid="onboarding-engines"
        >
          {found.map((e) => {
            const status = state(e);
            const isDefault = e.spec.id === chosen;
            const canLaunch = launchable.includes(e);
            const account = defaultAccount(accounts, e.spec.id);
            return (
              <li
                key={e.spec.id}
                className="group/row flex min-h-14 items-center gap-3 px-4 py-2"
                data-engine={e.spec.id}
                data-signed-in={status === "signed_in" ? "true" : "false"}
                data-default={isDefault ? "true" : "false"}
              >
                <EngineGlyph letters={e.spec.logo} engine={e.spec.id} size={20} />
                <span className="flex min-w-0 flex-1 items-baseline gap-2">
                  <span className="truncate text-base font-medium text-fg">{e.spec.label}</span>
                  {e.install?.version && (
                    <span className="font-mono text-xs text-fg-subtle tabular-nums">{e.install.version}</span>
                  )}
                </span>
                {isDefault ? (
                  <span className="text-sm text-fg-subtle" data-testid="onboarding-default">
                    {t("onboarding.engines.default")}
                  </span>
                ) : (
                  canLaunch && (
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label={t("onboarding.engines.makeDefaultFor", { engine: e.spec.label })}
                      className="opacity-0 group-hover/row:opacity-100 focus-visible:opacity-100"
                      onClick={() => useOnboarding.getState().setEngine(e.spec.id)}
                    >
                      {t("onboarding.engines.makeDefault")}
                    </Button>
                  )
                )}
                {status === "signed_in" && (
                  <span className="inline-flex items-center gap-1 text-sm text-fg-muted">
                    <Check size={14} aria-hidden="true" className="text-fg-subtle" />
                    {t("onboarding.engines.signedIn")}
                  </span>
                )}
                {status === "checking" && (
                  <span className="text-sm text-fg-subtle">{t("onboarding.engines.checking")}</span>
                )}
                {status === "signed_out" && (
                  <Button
                    size="sm"
                    variant="secondary"
                    data-testid={`onboarding-sign-in-${e.spec.id}`}
                    aria-label={t("onboarding.engines.signInTo", { engine: e.spec.label })}
                    onClick={() =>
                      setLogin(
                        account
                          ? { kind: "session", account, action: "sign_in" }
                          : { kind: "add", engineId: e.spec.id, nickname: t("onboarding.engines.nickname") },
                      )
                    }
                  >
                    {t("onboarding.engines.signIn")}
                  </Button>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {loaded && missing.length > 0 && (
        <div className="flex flex-col gap-2">
          <button
            type="button"
            aria-expanded={more}
            data-testid="onboarding-more"
            onClick={() => setMore((m) => !m)}
            className="inline-flex items-center gap-1 self-start rounded-sm text-sm text-fg-muted hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
          >
            {more ? (
              <ChevronDown size={14} aria-hidden="true" />
            ) : (
              <ChevronRight size={14} aria-hidden="true" />
            )}
            {t("onboarding.engines.more", { n: missing.length })}
          </button>
          {more && (
            <ul className="flex flex-col">
              {missing.map((e) => {
                const command = installCommand(e.spec.install, platform);
                return (
                  <li
                    key={e.spec.id}
                    data-testid={`onboarding-missing-${e.spec.id}`}
                    className="flex min-h-10 items-center gap-3 border-b border-divider py-1.5 pl-1 last:border-b-0"
                  >
                    <EngineGlyph letters={e.spec.logo} engine={e.spec.id} size={16} />
                    <span className="flex min-w-0 flex-1 flex-col">
                      <span className="truncate text-sm text-fg">{e.spec.label}</span>
                      {e.spec.note && <span className="text-xs text-fg-muted">{e.spec.note}</span>}
                    </span>
                    {command && (
                      <span className="flex min-w-0 max-w-[55%]">
                        <CopyCommand command={command} engine={e.spec.label} />
                      </span>
                    )}
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      )}
      <div className="flex items-center justify-between gap-3">
        <Button
          size="sm"
          variant="ghost"
          disabled={detecting}
          iconLeft={detecting ? <Spinner /> : <RefreshCw size={14} aria-hidden="true" />}
          onClick={() => void useEngines.getState().detect()}
        >
          {detecting ? t("onboarding.engines.checking") : t("onboarding.engines.rescan")}
        </Button>
        <span className="text-sm text-fg-subtle">{t("onboarding.engines.later")}</span>
      </div>
      {login && <LoginSheet request={login} onClose={() => setLogin(null)} />}
    </div>
  );
}

/** The folder Done works with: the one chosen this run, else (after a
 * relaunch) the newest local workspace. */
function useChosenFolder(): WorkspaceSummary | null {
  const folder = useOnboarding((s) => s.folder);
  return useWorkspaces((s) => {
    const local = s.list.filter((w) => w.kind === "local" && !w.parentId);
    if (folder) return local.find((w) => w.root === folder) ?? null;
    return local.reduce<WorkspaceSummary | null>((a, w) => (!a || w.createdAt > a.createdAt ? w : a), null);
  });
}

/** UI-04 step 2: a project folder becomes the first workspace. */
function WorkspaceStep({ onChosen }: { onChosen: () => void }) {
  const { show } = useToast();
  const chosen = useChosenFolder();
  const [busy, setBusy] = useState(false);
  const choose = async () => {
    setBusy(true);
    const picked = await invoke<string | null>("workspace_pick_folder", undefined, {
      timeoutMs: 24 * 3_600_000,
    }).catch(() => null);
    const added = picked ? await addWorkspace(picked, show) : false;
    setBusy(false);
    if (picked && added) {
      useOnboarding.getState().setFolder(picked);
      onChosen();
    }
  };
  return (
    <div className="flex flex-col gap-6">
      <p className="text-md text-fg-muted">{t("onboarding.workspace.body")}</p>
      <div className="flex flex-col items-center justify-center gap-4 rounded-lg border border-dashed border-border-strong px-8 py-10 text-center">
        {chosen ? (
          <span className="flex flex-col items-center gap-1" data-testid="onboarding-folder">
            <span className="inline-flex items-center gap-2 text-base font-medium text-fg">
              <Folder size={16} aria-hidden="true" className="text-fg-subtle" />
              {chosen.name}
            </span>
            <span className="font-mono text-xs text-fg-subtle">{chosen.root}</span>
          </span>
        ) : (
          <Folder size={20} aria-hidden="true" className="text-fg-subtle" />
        )}
        <Button
          variant={chosen ? "secondary" : "primary"}
          disabled={busy}
          data-testid="onboarding-choose-folder"
          onClick={() => void choose()}
        >
          {chosen ? t("onboarding.workspace.another") : t("onboarding.workspace.choose")}
        </Button>
      </div>
    </div>
  );
}

/** UI-04 step 3: what's set, the privacy line, and the first agent. */
function FinishStep({
  engine,
  folder,
  onPrivacy,
}: {
  engine: EngineView | null;
  folder: WorkspaceSummary | null;
  onPrivacy: () => void;
}) {
  return (
    <div className="mt-3 flex flex-col gap-6">
      {(engine || folder) && (
        <dl
          className="grid grid-cols-[auto_1fr] items-center gap-x-6 gap-y-3 rounded-md border border-border bg-surface-2 px-4 py-4 shadow-e1"
          data-testid="onboarding-summary"
        >
          {engine && (
            <>
              <dt className="text-sm text-fg-subtle">{t("onboarding.finish.agent")}</dt>
              <dd className="flex items-center gap-2 text-base text-fg">
                <EngineGlyph letters={engine.spec.logo} engine={engine.spec.id} size={20} />
                {engine.spec.label}
              </dd>
            </>
          )}
          {folder && (
            <>
              <dt className="text-sm text-fg-subtle">{t("onboarding.finish.folder")}</dt>
              <dd className="flex min-w-0 items-baseline gap-2 text-base text-fg">
                {folder.name}
                <span className="truncate font-mono text-xs text-fg-subtle">{folder.root}</span>
              </dd>
            </>
          )}
        </dl>
      )}
      {!folder && <p className="text-md text-fg-muted">{t("onboarding.finish.noFolder")}</p>}
      <p className="text-sm text-fg-muted" data-testid="onboarding-privacy">
        {t("onboarding.finish.privacy")}{" "}
        <button
          type="button"
          onClick={onPrivacy}
          className="rounded-xs text-fg underline decoration-border-strong underline-offset-2 hover:decoration-fg focus-visible:shadow-focus focus-visible:outline-none"
        >
          {t("onboarding.finish.privacyLink")}
        </button>
        {t("onboarding.finish.privacyAfter")}
      </p>
    </div>
  );
}

/** UI-04 onboarding wizard (F-SH-08): Engines, Your first folder, Done.
 * Every step has Back and Skip; the step persists. */
export function Onboarding() {
  const state = useOnboarding((s) => s.state);
  const heading = useRef<HTMLHeadingElement>(null);
  const { show } = useToast();
  const steps = state?.steps ?? [];
  const step = state?.step ?? steps[0] ?? "engines";
  const index = Math.max(0, steps.indexOf(step));
  const { launchable, chosen } = useFirstRunEngines();
  const folder = useChosenFolder();
  const engine = launchable.find((e) => e.spec.id === chosen) ?? null;
  const [starting, setStarting] = useState(false);
  useEffect(() => heading.current?.focus(), [step]);

  const next = () => {
    const following = steps[index + 1];
    if (following) void useOnboarding.getState().go(following);
    else void finish(null);
  };
  const back = () => {
    const previous = steps[index - 1];
    if (previous) void useOnboarding.getState().go(previous);
  };
  /** Code mode on the folder's first tab: with `engineId`, its agent
   * launches there through the launch grid's path (layout.split); without,
   * the tab keeps its launch grid. */
  const finish = async (engineId: string | null) => {
    const ws = folder;
    const tab = ws?.tabs[0];
    if (ws && tab) {
      useWorkspaces.getState().select(ws.id);
      useWorkspaces.getState().selectTab(ws.id, tab.id);
      useLayout.getState().markEmpty(tab.id);
      if (engineId) {
        setStarting(true);
        const launch = useLayout.getState().split(tab.id, null, "right", launchFor(engineId));
        useLayout.getState().expect(tab.id, launch);
        const res = await launch;
        setStarting(false);
        if (!res.ok) show({ tone: "error", title: (res.error as IpcError).message });
      }
    }
    useUi.getState().setMode("code");
    await useOnboarding.getState().finish();
  };
  const privacy = async () => {
    await finish(null);
    useUi.getState().openSettings("privacy");
  };

  const footer = (() => {
    if (step === "finish") {
      const canStart = folder !== null && engine !== null;
      return (
        <div className="flex gap-2">
          <Button
            variant={canStart ? "ghost" : "primary"}
            data-testid="onboarding-skip"
            disabled={starting}
            onClick={() => void finish(null)}
          >
            {canStart && folder
              ? t("onboarding.finish.open", { folder: folder.name })
              : t("onboarding.finish.done")}
          </Button>
          {canStart && folder && engine && (
            <Button
              variant="primary"
              autoFocus
              data-testid="onboarding-start"
              disabled={starting}
              iconLeft={
                starting ? (
                  <Spinner />
                ) : (
                  <EngineGlyph letters={engine.spec.logo} engine={engine.spec.id} size={16} />
                )
              }
              onClick={() => void finish(engine.spec.id)}
            >
              {t("onboarding.finish.start", { engine: engine.spec.label, folder: folder.name })}
            </Button>
          )}
        </div>
      );
    }
    return (
      <div className="flex gap-2">
        <Button variant="ghost" data-testid="onboarding-skip" onClick={next}>
          {t("onboarding.skip")}
        </Button>
        {(step !== "workspace" || folder !== null) && (
          <Button variant="primary" data-testid="onboarding-continue" onClick={next}>
            {t("onboarding.continue")}
          </Button>
        )}
      </div>
    );
  })();

  return (
    <main
      className="flex h-full flex-col items-center overflow-auto bg-bg px-6 pb-12 pt-24"
      data-testid="onboarding"
      data-step={step}
    >
      <div className="flex w-full max-w-[640px] flex-col gap-8">
        <Stepper
          steps={steps.map((s) => t(STEP_LABELS[s] ?? "onboarding.step.engines"))}
          current={index}
          label={t("onboarding.progress")}
        />
        <div key={step} className="tn-step-in flex flex-col gap-3">
          <h1
            ref={heading}
            tabIndex={-1}
            className="tn-step-title text-2xl font-semibold tracking-tight text-fg focus:outline-none"
          >
            {t(TITLES[step] ?? "onboarding.engines.title")}
          </h1>
          {step === "engines" && <EnginesStep />}
          {step === "workspace" && <WorkspaceStep onChosen={next} />}
          {step === "finish" && (
            <FinishStep engine={engine} folder={folder} onPrivacy={() => void privacy()} />
          )}
        </div>
        <footer className="flex items-center justify-between border-t border-divider pt-4">
          <Button variant="ghost" disabled={index === 0} data-testid="onboarding-back" onClick={back}>
            {t("onboarding.back")}
          </Button>
          {footer}
        </footer>
      </div>
    </main>
  );
}
