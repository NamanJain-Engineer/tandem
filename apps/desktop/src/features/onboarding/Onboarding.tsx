import { useEffect, useRef, useState } from "react";
import {
  Badge,
  Button,
  EngineGlyph,
  SettingsGroup,
  SettingsRow,
  Spinner,
  Stepper,
  Switch,
  useToast,
} from "@tandem/ui";
import { t, type MessageKey } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useAccounts } from "../../stores/accounts";
import { useEngines } from "../../stores/engines";
import { useLayout } from "../../stores/layout";
import { useOnboarding } from "../../stores/onboarding";
import { useSetting, useSettings } from "../../stores/settings";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { LoginSheet, SignInState, type LoginRequest } from "../settings/AccountsPage";
import { addWorkspace } from "../shell/actions";
import { DictateSetupBody } from "../dictate/DictateSetup";

const TITLES: Record<string, MessageKey> = {
  engines: "onboarding.engines.title",
  accounts: "onboarding.accounts.title",
  dictate: "onboarding.dictate.title",
  privacy: "onboarding.privacy.title",
  workspace: "onboarding.workspace.title",
};

/** UI-05 embedded: what's installed; installs and sign-ins live in Settings › Engines. */
function EnginesStep() {
  const list = useEngines((s) => s.list);
  const loaded = useEngines((s) => s.loaded);
  const detecting = useEngines((s) => s.detecting);
  useEffect(() => {
    if (!loaded) void useEngines.getState().refresh();
  }, [loaded]);
  const installed = list.filter((e) => e.install);
  const missing = list.filter((e) => !e.install);
  return (
    <div className="flex flex-col gap-4">
      <p className="text-sm text-fg-muted">{t("onboarding.engines.body")}</p>
      {!loaded ? (
        <span className="flex items-center gap-2 text-sm text-fg-muted">
          <Spinner /> {t("onboarding.engines.checking")}
        </span>
      ) : (
        <ul
          className="flex flex-col divide-y divide-divider rounded-md border border-border"
          data-testid="onboarding-engines"
        >
          {[...installed, ...missing].map((e) => (
            <li
              key={e.spec.id}
              className="flex items-center gap-3 px-3 py-2 text-sm"
              data-engine={e.spec.id}
              data-installed={e.install ? "true" : "false"}
            >
              <EngineGlyph letters={e.spec.logo} engine={e.spec.id} size={20} />
              <span className="flex-1 text-fg">{e.spec.label}</span>
              {e.install ? (
                <Badge variant="done">
                  {t("onboarding.engines.installed", { version: e.install.version ?? "" })}
                </Badge>
              ) : (
                <span className="text-fg-muted">{t("onboarding.engines.notInstalled")}</span>
              )}
            </li>
          ))}
        </ul>
      )}
      <div className="flex items-center justify-between gap-3">
        <Button size="sm" disabled={detecting} onClick={() => void useEngines.getState().detect()}>
          {detecting ? t("onboarding.engines.checking") : t("onboarding.engines.rescan")}
        </Button>
        <span className="text-sm text-fg-muted">{t("onboarding.engines.later")}</span>
      </div>
    </div>
  );
}

/** F-SH-08 step 3: each installed chat engine's own login, run in its
 * default account's provider home (F-AC-02). Vibezeo never sees the token. */
function AccountsStep() {
  const engines = useEngines((s) => s.list);
  const enginesLoaded = useEngines((s) => s.loaded);
  const accounts = useAccounts((s) => s.list);
  const loaded = useAccounts((s) => s.loaded);
  const signIn = useAccounts((s) => s.signIn);
  const [login, setLogin] = useState<LoginRequest | null>(null);
  useEffect(() => {
    void useAccounts.getState().load();
    if (!enginesLoaded) void useEngines.getState().refresh();
  }, [enginesLoaded]);
  useEffect(() => {
    const { signIn: known, check } = useAccounts.getState();
    for (const a of accounts) if (known[a.id] === undefined) void check(a.id);
  }, [accounts]);
  // Chat engines whose accounts Vibezeo keeps apart (D-114).
  const chat = engines.filter(
    (e) =>
      e.install !== null &&
      e.unsupportedReason === null &&
      e.spec.chatAdapter !== null &&
      e.spec.homeStrategy !== "none",
  );
  return (
    <div className="flex flex-col gap-4">
      <p className="text-sm text-fg-muted">{t("onboarding.accounts.body")}</p>
      {!enginesLoaded || !loaded ? (
        <span className="flex items-center gap-2 text-sm text-fg-muted">
          <Spinner /> {t("onboarding.engines.checking")}
        </span>
      ) : chat.length === 0 ? (
        <p
          className="rounded-md border border-border px-3 py-4 text-sm text-fg-muted"
          data-testid="onboarding-accounts-empty"
        >
          {t("onboarding.accounts.none")}
        </p>
      ) : (
        <ul
          className="flex flex-col divide-y divide-divider rounded-md border border-border"
          data-testid="onboarding-accounts"
        >
          {chat.map((e) => {
            const account = accounts.find((a) => a.engineId === e.spec.id && a.isDefault);
            const signedIn = account !== undefined && signIn[account.id] === "signed_in";
            return (
              <li
                key={e.spec.id}
                className="flex items-center gap-3 px-3 py-2 text-sm"
                data-engine={e.spec.id}
                data-signed-in={signedIn ? "true" : "false"}
              >
                <EngineGlyph letters={e.spec.logo} engine={e.spec.id} size={20} />
                <span className="flex-1 text-fg">{e.spec.label}</span>
                {account ? (
                  <SignInState account={account} />
                ) : (
                  <span className="text-fg-muted">{t("onboarding.accounts.notSignedIn")}</span>
                )}
                {!signedIn && (
                  <Button
                    size="sm"
                    variant="secondary"
                    data-testid={`onboarding-sign-in-${e.spec.id}`}
                    onClick={() =>
                      setLogin(
                        account
                          ? { kind: "session", account, action: "sign_in" }
                          : { kind: "add", engineId: e.spec.id, nickname: t("onboarding.accounts.nickname") },
                      )
                    }
                  >
                    {t("onboarding.accounts.signIn", { engine: e.spec.label })}
                  </Button>
                )}
              </li>
            );
          })}
        </ul>
      )}
      <p className="text-sm text-fg-muted">{t("onboarding.accounts.later")}</p>
      {login && <LoginSheet request={login} onClose={() => setLogin(null)} />}
    </div>
  );
}

/** F-TL-01: asked once, both off by default. */
function PrivacyStep() {
  const usage = useSetting("telemetry_usage", false);
  const crash = useSetting("telemetry_crash", false);
  const save = (key: "telemetry_usage" | "telemetry_crash", value: boolean) =>
    void invoke("settings_set", { key, value })
      .then(() => useSettings.setState({ values: { ...useSettings.getState().values, [key]: value } }))
      .catch(() => undefined);
  return (
    <div className="flex flex-col gap-4">
      <p className="text-sm text-fg-muted">{t("onboarding.privacy.body")}</p>
      <SettingsGroup>
        <SettingsRow
          label={t("settings.usageStats")}
          description={t("settings.usageStats.desc")}
          settingKey="telemetry_usage"
          control={(ids) => (
            <Switch
              id={ids.id}
              aria-describedby={ids.describedBy}
              checked={usage}
              aria-label={t("settings.usageStats")}
              onCheckedChange={(v) => save("telemetry_usage", v)}
            />
          )}
        />
        <SettingsRow
          label={t("settings.crashReports")}
          description={t("settings.crashReports.desc")}
          settingKey="telemetry_crash"
          control={(ids) => (
            <Switch
              id={ids.id}
              aria-describedby={ids.describedBy}
              checked={crash}
              aria-label={t("settings.crashReports")}
              onCheckedChange={(v) => save("telemetry_crash", v)}
            />
          )}
        />
      </SettingsGroup>
      <p className="text-sm text-fg-muted">{t("onboarding.privacy.later")}</p>
    </div>
  );
}

/** Picks the first workspace; Finish opens it in Code mode on the launch grid. */
function WorkspaceStep({ onAdded }: { onAdded: (root: string) => void }) {
  const { show } = useToast();
  const choose = async () => {
    const picked = await invoke<string | null>("workspace_pick_folder", undefined, {
      timeoutMs: 24 * 3_600_000,
    }).catch(() => null);
    if (picked && (await addWorkspace(picked, show))) onAdded(picked);
  };
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col items-center justify-center gap-3 rounded-lg border border-dashed border-border-strong p-8 text-center">
        <p className="text-sm text-fg">{t("onboarding.workspace.body")}</p>
        <Button variant="primary" data-testid="onboarding-choose-folder" onClick={() => void choose()}>
          {t("onboarding.workspace.choose")}
        </Button>
      </div>
    </div>
  );
}

/** UI-04 onboarding wizard (F-SH-08 slice): Engines, Accounts, Privacy, First workspace.
 * Every step has Back and Skip; completion persists. */
export function Onboarding() {
  const state = useOnboarding((s) => s.state);
  const heading = useRef<HTMLHeadingElement>(null);
  const steps = state?.steps ?? [];
  const step = state?.step ?? steps[0] ?? "engines";
  const index = Math.max(0, steps.indexOf(step));
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
  /** Code mode, on the chosen workspace's launch grid when there is one. */
  const finish = async (root: string | null) => {
    if (root) {
      await useWorkspaces.getState().refresh();
      const ws = useWorkspaces.getState().list.find((w) => w.root === root);
      const tab = ws?.tabs[0];
      if (ws && tab) {
        useLayout.getState().markEmpty(tab.id);
        useWorkspaces.getState().select(ws.id);
      }
    }
    useUi.getState().setMode("code");
    await useOnboarding.getState().finish();
  };

  return (
    <main
      className="flex h-full justify-center overflow-auto bg-bg px-6 py-10"
      data-testid="onboarding"
      data-step={step}
    >
      <div className="flex w-full max-w-[720px] flex-col gap-6">
        <Stepper
          steps={steps.map((s) => t(TITLES[s] ?? "onboarding.engines.title"))}
          current={index}
          label={t("onboarding.progress")}
        />
        <h1 ref={heading} tabIndex={-1} className="text-2xl font-semibold text-fg focus:outline-none">
          {t(TITLES[step] ?? "onboarding.engines.title")}
        </h1>
        {step === "engines" && <EnginesStep />}
        {step === "accounts" && <AccountsStep />}
        {step === "dictate" && <DictateSetupBody />}
        {step === "privacy" && <PrivacyStep />}
        {step === "workspace" && <WorkspaceStep onAdded={(root) => void finish(root)} />}
        <footer className="flex items-center justify-between border-t border-divider pt-4">
          <Button variant="ghost" disabled={index === 0} data-testid="onboarding-back" onClick={back}>
            {t("onboarding.back")}
          </Button>
          <div className="flex gap-2">
            <Button variant="ghost" data-testid="onboarding-skip" onClick={next}>
              {t("onboarding.skip")}
            </Button>
            {step !== "workspace" && (
              <Button variant="primary" data-testid="onboarding-continue" onClick={next}>
                {t("onboarding.continue")}
              </Button>
            )}
          </div>
        </footer>
      </div>
    </main>
  );
}
