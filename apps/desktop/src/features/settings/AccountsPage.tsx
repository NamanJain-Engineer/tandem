import { useEffect, useMemo, useRef, useState } from "react";
import { MoreHorizontal, Plus, X } from "lucide-react";
import type { Account, IpcError, LoginAction, LoginSession } from "@tandem/protocol";
import {
  Badge,
  Button,
  Callout,
  ConfirmDialog,
  EmptyState,
  IconButton,
  Input,
  Menu,
  Select,
  SettingsGroup,
  Sheet,
  Skeleton,
  SkeletonGate,
  useToast,
  type MenuEntry,
} from "@tandem/ui";
import { t } from "../../i18n";
import {
  beginAdd,
  beginSession,
  cancelLogin,
  finishLogin,
  makeDefault,
  onLoginEnd,
  removeAccount,
  renameAccount,
  setOverride,
  useAccounts,
} from "../../stores/accounts";
import { defaultEngine } from "../../stores/launch";
import { useEngines } from "../../stores/engines";
import { useWorkspaces } from "../../stores/workspaces";
import { MiniTerminal } from "../code/MiniTerminal";

export type LoginRequest =
  | { kind: "add"; engineId?: string; nickname?: string }
  | { kind: "session"; account: Account; action: LoginAction };

export function SignInState({ account }: { account: Account }) {
  const state = useAccounts((s) => s.signIn[account.id]);
  if (state === undefined) return <span className="text-sm text-fg-subtle">{t("accounts.checking")}</span>;
  if (state === "signed_in") return <span className="text-sm text-fg-muted">{t("accounts.signedIn")}</span>;
  if (state === "signed_out")
    return <span className="text-sm text-warning-text">{t("accounts.signedOut")}</span>;
  return null;
}

function RenameField({ account, onDone }: { account: Account; onDone: () => void }) {
  const [value, setValue] = useState(account.label);
  const [error, setError] = useState<string | null>(null);
  const save = async () => {
    if (value.trim() === account.label) return onDone();
    const out = await renameAccount(account.id, value);
    if (out.ok) onDone();
    else setError(out.error.message);
  };
  return (
    <form
      className="flex min-w-0 flex-1 items-center gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <Input
        autoFocus
        label={t("accounts.nickname")}
        hideLabel
        value={value}
        error={error ?? undefined}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => e.key === "Escape" && onDone()}
        data-testid="account-rename"
      />
    </form>
  );
}

function AccountRow({
  account,
  onLogin,
  onRemove,
}: {
  account: Account;
  onLogin: (a: LoginAction) => void;
  onRemove: () => void;
}) {
  const [renaming, setRenaming] = useState(false);
  const { show } = useToast();
  const setDefault = async () => {
    const out = await makeDefault(account.id);
    if (!out.ok) show({ tone: "error", title: out.error.message });
  };
  const menu: MenuEntry[] = [
    { id: "rename", label: t("accounts.rename"), onSelect: () => setRenaming(true) },
    ...(account.isDefault
      ? []
      : [{ id: "default", label: t("accounts.makeDefault"), onSelect: () => void setDefault() }]),
    { id: "sign-in", label: t("accounts.signInAgain"), onSelect: () => onLogin("sign_in") },
    { id: "sign-out", label: t("accounts.signOut"), onSelect: () => onLogin("sign_out") },
    { id: "sep", type: "separator" },
    { id: "remove", label: t("accounts.remove"), danger: true, onSelect: onRemove },
  ];
  return (
    <div
      data-testid="account-row"
      data-account-id={account.id}
      data-label={account.label}
      data-default={account.isDefault ? "true" : "false"}
      className="flex min-h-[40px] items-center gap-2 border-b border-divider px-3 py-1.5 last:border-b-0"
    >
      {renaming ? (
        <RenameField account={account} onDone={() => setRenaming(false)} />
      ) : (
        <span className="min-w-0 truncate text-base font-medium text-fg">{account.label}</span>
      )}
      {account.isDefault && <Badge variant="accent">{t("accounts.default")}</Badge>}
      <SignInState account={account} />
      <span className="flex-1" />
      <span className="hidden text-xs text-fg-subtle md:inline">{t("accounts.usageNotReported")}</span>
      {!account.isDefault && (
        <Button
          size="sm"
          variant="secondary"
          onClick={() => void setDefault()}
          data-testid="account-make-default"
        >
          {t("accounts.makeDefault")}
        </Button>
      )}
      <Menu
        align="end"
        trigger={
          <IconButton
            size="sm"
            icon={<MoreHorizontal size={14} />}
            label={t("accounts.menu", { name: account.label })}
          />
        }
        entries={menu}
      />
    </div>
  );
}

type Phase =
  | { at: "form"; error: IpcError | null }
  | { at: "running"; session: LoginSession }
  | { at: "done"; state: "signed_in" | "signed_out" }
  | { at: "failed"; message: string | null };

/** UI-35 "Add account" (and Sign in again / Sign out): the engine's own
 * login runs in a terminal inside the sheet, under the account's home. */
export function LoginSheet({ request, onClose }: { request: LoginRequest; onClose: () => void }) {
  const engines = useEngines((s) => s.list);
  const label = useEngines((s) => s.label);
  const installed = useMemo(
    () => engines.filter((e) => e.install !== null && e.unsupportedReason === null),
    [engines],
  );
  const [engineId, setEngineId] = useState(
    request.kind === "add" ? (request.engineId ?? defaultEngine(engines) ?? "") : request.account.engineId,
  );
  const [nickname, setNickname] = useState(
    request.kind === "session" ? request.account.label : (request.nickname ?? ""),
  );
  const [phase, setPhase] = useState<Phase>({ at: "form", error: null });
  const [busy, setBusy] = useState(false);
  const stop = useRef<(() => void) | null>(null);
  const engine = label(engineId) ?? "";

  const follow = (session: LoginSession) => {
    setPhase({ at: "running", session });
    return onLoginEnd(session.opId, (e) => {
      if (e.state === "cancelled") return;
      if (e.state === "failed") setPhase({ at: "failed", message: e.message });
      else setPhase({ at: "done", state: e.state === "signed_out" ? "signed_out" : "signed_in" });
      void useAccounts.getState().load();
      void useAccounts.getState().check(e.accountId);
    });
  };

  const start = async () => {
    setBusy(true);
    const out =
      request.kind === "add"
        ? await beginAdd(engineId, nickname)
        : await beginSession(request.account.id, request.action);
    setBusy(false);
    if (!out.ok) return setPhase({ at: "form", error: out.error });
    stop.current?.();
    stop.current = follow(out.value);
  };

  // Sign in again and Sign out start at once; adding asks for a name first.
  useEffect(() => {
    if (request.kind === "session") void start();
    return () => stop.current?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const cancel = () => {
    if (phase.at === "running") void cancelLogin(phase.session.opId);
    onClose();
  };
  const title =
    request.kind === "add"
      ? t("accounts.addTitle")
      : request.action === "sign_in"
        ? t("accounts.signInTitle", { name: request.account.label })
        : t("accounts.signOutTitle", { name: request.account.label });
  const limit = phase.at === "form" && phase.error?.code === "limit_reached";

  return (
    <Sheet
      open
      size="lg"
      onOpenChange={(o) => !o && cancel()}
      title={title}
      closeLabel={t("common.close")}
      footer={
        <div className="flex w-full items-center justify-end gap-2">
          {phase.at === "done" ? (
            <Button variant="primary" onClick={onClose} data-testid="login-done">
              {t("accounts.done")}
            </Button>
          ) : (
            <>
              <Button variant="secondary" onClick={cancel} data-testid="login-cancel">
                {t("common.cancel")}
              </Button>
              {phase.at === "form" && request.kind === "add" && (
                <Button
                  variant="primary"
                  loading={busy}
                  disabled={!engineId || !nickname.trim() || limit}
                  onClick={() => void start()}
                  data-testid="login-start"
                >
                  {t("accounts.signIn")}
                </Button>
              )}
              {phase.at === "running" && !phase.session.reports && (
                <Button
                  variant="primary"
                  onClick={() => void finishLogin(phase.session.opId)}
                  data-testid="login-finish"
                >
                  {request.kind === "session" && request.action === "sign_out"
                    ? t("accounts.signedOutManual")
                    : t("accounts.signedInManual")}
                </Button>
              )}
              {phase.at === "failed" && (
                <Button
                  variant="primary"
                  onClick={() =>
                    request.kind === "add" ? setPhase({ at: "form", error: null }) : void start()
                  }
                >
                  {t("accounts.tryAgain")}
                </Button>
              )}
            </>
          )}
        </div>
      }
    >
      <div
        className="flex flex-col gap-4"
        data-testid="login-sheet"
        data-op-id={phase.at === "running" ? phase.session.opId : undefined}
        data-account-id={phase.at === "running" ? phase.session.accountId : undefined}
      >
        {phase.at === "form" && request.kind === "add" && (
          <>
            <div className="grid grid-cols-2 gap-3">
              <Select
                label={t("accounts.engine")}
                value={engineId}
                onValueChange={setEngineId}
                options={installed.map((e) => ({ value: e.spec.id, label: e.spec.label }))}
              />
              <Input
                label={t("accounts.nickname")}
                placeholder={t("accounts.nicknamePlaceholder")}
                value={nickname}
                maxLength={40}
                onChange={(e) => setNickname(e.target.value)}
                data-testid="login-nickname"
              />
            </div>
            {limit ? (
              <Callout tone="info">{phase.error?.message}</Callout>
            ) : (
              phase.error && <Callout tone="error">{phase.error.message}</Callout>
            )}
          </>
        )}
        {phase.at === "running" && (
          <>
            <p className="text-sm text-fg-muted">{t("accounts.ownLogin", { engine })}</p>
            <MiniTerminal paneId={phase.session.paneId} label={t("accounts.terminal", { engine })} />
            {!phase.session.reports && (
              <p className="text-sm text-fg-subtle">{t("accounts.manualHint", { engine })}</p>
            )}
          </>
        )}
        {phase.at === "done" && (
          <Callout tone="success">
            {phase.state === "signed_in"
              ? t("accounts.signedInAs", { name: nickname })
              : t("accounts.signedOutOf", { name: nickname })}
          </Callout>
        )}
        {phase.at === "failed" && (
          <Callout tone="warning">{t("accounts.failed", { line: phase.message ?? "" })}</Callout>
        )}
      </div>
    </Sheet>
  );
}

/** Settings › Accounts (UI-35, UI-106; F-AC-02, F-AC-03). */
export function AccountsPage() {
  const list = useAccounts((s) => s.list);
  const overrides = useAccounts((s) => s.overrides);
  const loaded = useAccounts((s) => s.loaded);
  const failed = useAccounts((s) => s.failed);
  const engines = useEngines((s) => s.list);
  const enginesLoaded = useEngines((s) => s.loaded);
  const label = useEngines((s) => s.label);
  const workspaces = useWorkspaces((s) => s.list);
  const [login, setLogin] = useState<LoginRequest | null>(null);
  const [removing, setRemoving] = useState<Account | null>(null);
  const { show } = useToast();

  useEffect(() => {
    void useAccounts.getState().load();
    if (!enginesLoaded) void useEngines.getState().refresh();
  }, [enginesLoaded]);
  useEffect(() => {
    const { signIn, check } = useAccounts.getState();
    for (const a of list) if (signIn[a.id] === undefined) void check(a.id);
  }, [list]);

  // Installed engines, and any engine that still has accounts.
  const groups = useMemo(() => {
    const ids = new Set([
      ...engines.filter((e) => e.install !== null).map((e) => e.spec.id),
      ...list.map((a) => a.engineId),
    ]);
    return [...ids].map((id) => ({
      id,
      label: label(id) ?? id,
      accounts: list.filter((a) => a.engineId === id),
    }));
  }, [engines, list, label]);

  const remove = async (a: Account) => {
    const out = await removeAccount(a.id);
    show(
      out.ok
        ? { title: t("accounts.removed", { name: a.label }) }
        : { tone: "error", title: out.error.message },
    );
  };
  const wsName = (id: string) => workspaces.find((w) => w.id === id)?.name ?? id;
  const accountName = (id: string) => list.find((a) => a.id === id)?.label ?? "";

  return (
    <div className="flex flex-col gap-4" data-testid="accounts-page">
      <div className="flex items-start justify-between gap-3">
        <p className="text-sm text-fg-muted">{t("accounts.intro")}</p>
        <Button
          size="sm"
          variant="secondary"
          iconLeft={<Plus size={14} />}
          onClick={() => setLogin({ kind: "add" })}
          data-testid="account-add"
        >
          {t("accounts.add")}
        </Button>
      </div>
      <SkeletonGate
        loading={!loaded}
        label={t("accounts.loading")}
        skeleton={
          <div className="flex flex-col gap-2">
            <Skeleton height={40} />
            <Skeleton height={40} />
          </div>
        }
      >
        {failed ? (
          <Callout tone="error">{t("accounts.loadFailed")}</Callout>
        ) : groups.length === 0 ? (
          <EmptyState title={t("accounts.emptyTitle")} body={t("accounts.emptyBody")} />
        ) : (
          groups.map((g) => (
            <SettingsGroup key={g.id} label={g.label}>
              {g.accounts.length === 0 ? (
                <div className="flex items-center gap-2 px-3 py-2 text-sm text-fg-muted">
                  <span>{t("accounts.noneYet", { engine: g.label })}</span>
                  <Button size="sm" variant="ghost" onClick={() => setLogin({ kind: "add", engineId: g.id })}>
                    {t("accounts.add")}
                  </Button>
                </div>
              ) : (
                g.accounts.map((a) => (
                  <AccountRow
                    key={a.id}
                    account={a}
                    onLogin={(action) => setLogin({ kind: "session", account: a, action })}
                    onRemove={() => setRemoving(a)}
                  />
                ))
              )}
            </SettingsGroup>
          ))
        )}
        {overrides.length > 0 && (
          <SettingsGroup label={t("accounts.overrides")}>
            {overrides.map((o) => (
              <div
                key={`${o.workspaceId}:${o.engineId}`}
                data-testid="account-override"
                className="flex min-h-[36px] items-center gap-2 border-b border-divider px-3 py-1 text-sm last:border-b-0"
              >
                <span className="min-w-0 flex-1 truncate text-fg">
                  {t("accounts.overrideRow", {
                    workspace: wsName(o.workspaceId),
                    engine: label(o.engineId) ?? o.engineId,
                    account: accountName(o.accountId),
                  })}
                </span>
                <IconButton
                  size="sm"
                  icon={<X size={14} />}
                  label={t("accounts.overrideRemove", { workspace: wsName(o.workspaceId) })}
                  onClick={() => void setOverride(o.workspaceId, o.engineId, null)}
                />
              </div>
            ))}
          </SettingsGroup>
        )}
      </SkeletonGate>
      {login && (
        <LoginSheet
          request={login}
          onClose={() => {
            setLogin(null);
            void useAccounts.getState().load();
          }}
        />
      )}
      {removing && (
        <ConfirmDialog
          open
          danger
          onOpenChange={(o) => !o && setRemoving(null)}
          title={t("accounts.removeTitle", { name: removing.label })}
          body={t("accounts.removeBody")}
          confirmLabel={t("accounts.removeConfirm")}
          onConfirm={() => {
            const a = removing;
            setRemoving(null);
            void remove(a);
          }}
        />
      )}
    </div>
  );
}
