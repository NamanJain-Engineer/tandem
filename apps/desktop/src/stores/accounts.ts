import { create } from "zustand";
import type {
  Account,
  AccountOverride,
  IpcError,
  LoginAction,
  LoginEvent,
  LoginSession,
  SignInState,
} from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";
import type { Outcome } from "./launch";

type AccountsState = {
  list: Account[];
  overrides: AccountOverride[];
  loaded: boolean;
  failed: boolean;
  /** Sign-in state by account id ("Checking…" while missing). */
  signIn: Record<string, SignInState>;
  load: () => Promise<void>;
  check: (accountId: string) => Promise<void>;
};

export const useAccounts = create<AccountsState>((set) => ({
  list: [],
  overrides: [],
  loaded: false,
  failed: false,
  signIn: {},
  load: async () => {
    try {
      const [list, overrides] = await Promise.all([
        invoke<Account[]>("accounts_list", {}),
        invoke<AccountOverride[]>("accounts_overrides", {}),
      ]);
      set({ list: list ?? [], overrides: overrides ?? [], loaded: true, failed: false });
    } catch {
      set({ loaded: true, failed: true });
    }
  },
  check: async (accountId) => {
    const state = await invoke<SignInState>("accounts_status", { accountId }, { timeoutMs: 30_000 }).catch(
      () => "unknown" as SignInState,
    );
    set((s) => ({ signIn: { ...s.signIn, [accountId]: state } }));
  },
}));

export const accountsFor = (list: Account[], engineId: string | null) =>
  list.filter((a) => a.engineId === engineId);

/** The account a new `engineId` session in `workspaceId` uses (F-AC-03):
 * the workspace's override, else the engine's default. */
export function accountForNext(
  list: Account[],
  overrides: AccountOverride[],
  engineId: string,
  workspaceId: string,
): Account | null {
  const pick = overrides.find((o) => o.workspaceId === workspaceId && o.engineId === engineId)?.accountId;
  const mine = accountsFor(list, engineId);
  return mine.find((a) => a.id === pick) ?? mine.find((a) => a.isDefault) ?? null;
}

async function outcome<T>(run: () => Promise<T>): Promise<Outcome<T>> {
  try {
    const value = await run();
    await useAccounts.getState().load();
    return { ok: true, value };
  } catch (e) {
    return { ok: false, error: e as IpcError };
  }
}

export const renameAccount = (accountId: string, label: string) =>
  outcome(() => invoke<Account>("accounts_update", { accountId, label }));
export const makeDefault = (accountId: string) =>
  outcome(() => invoke<void>("accounts_set_default", { accountId }));
export const removeAccount = (accountId: string) =>
  outcome(() => invoke<void>("accounts_remove", { accountId }));
export const setOverride = (workspaceId: string, engineId: string, accountId: string | null) =>
  outcome(() => invoke<void>("workspace_set_account_override", { id: workspaceId, engineId, accountId }));

export async function beginAdd(engineId: string, label: string): Promise<Outcome<LoginSession>> {
  try {
    return { ok: true, value: await invoke<LoginSession>("accounts_begin_add", { engineId, label }) };
  } catch (e) {
    return { ok: false, error: e as IpcError };
  }
}

export async function beginSession(accountId: string, action: LoginAction): Promise<Outcome<LoginSession>> {
  try {
    return { ok: true, value: await invoke<LoginSession>("accounts_begin_session", { accountId, action }) };
  } catch (e) {
    return { ok: false, error: e as IpcError };
  }
}

export const cancelLogin = (opId: string) =>
  invoke<void>("accounts_cancel_login", { opId }).catch(() => undefined);
export const finishLogin = (opId: string) =>
  invoke<void>("accounts_finish_login", { opId }).catch(() => undefined);

/** Calls `onEnd` when the sign-in `opId` ends; returns the unsubscribe. */
export function onLoginEnd(opId: string, onEnd: (e: LoginEvent) => void): () => void {
  let stop: (() => void) | null = null;
  let done = false;
  void listen<LoginEvent>("accounts:login-finished", (e) => {
    if (e.opId === opId) onEnd(e);
  }).then((un) => {
    if (done) un();
    else stop = un;
  });
  return () => {
    done = true;
    stop?.();
  };
}
