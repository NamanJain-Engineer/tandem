import { create } from "zustand";
import type { AuthState, EntitlementView } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";

type SessionState = {
  auth: AuthState | null;
  entitlement: EntitlementView | null;
  load: () => Promise<void>;
  refreshEntitlement: () => Promise<EntitlementView | null>;
};

export const useSession = create<SessionState>((set) => ({
  auth: null,
  entitlement: null,
  load: async () => {
    const [auth, entitlement] = await Promise.all([
      invoke<AuthState>("auth_state"),
      invoke<EntitlementView>("entitlement_get"),
    ]);
    set({ auth, entitlement });
  },
  refreshEntitlement: async () => {
    try {
      const entitlement = await invoke<EntitlementView>("entitlement_refresh", undefined, {
        timeoutMs: 30_000,
      });
      set({ entitlement });
      return entitlement;
    } catch {
      return null;
    }
  },
}));

export async function watchSession(): Promise<() => void> {
  const offAuth = await listen<AuthState>("auth:changed", (auth) => useSession.setState({ auth }));
  const offEnt = await listen<EntitlementView>("entitlement:changed", (entitlement) =>
    useSession.setState({ entitlement }),
  );
  return () => {
    offAuth();
    offEnt();
  };
}

/** Signed in, or using Tandem offline: either way the app is usable. */
export function canUseApp(auth: AuthState | null): boolean {
  return !!auth && (auth.signedIn || auth.offline);
}

export function totalCredits(e: EntitlementView | null): number | null {
  if (!e?.credits) return null;
  return e.credits.monthly + e.credits.purchased;
}

/** What can be spent now: the balance, held to a Team member's remaining cap (F-BL-06). */
export function availableCredits(e: EntitlementView | null): number | null {
  const total = totalCredits(e);
  if (total === null || !e?.credits) return total;
  const { cap, spent } = e.credits;
  return cap === null ? total : Math.min(total, Math.max(0, cap - (spent ?? 0)));
}

/** The ◈ display's state (UI_SPEC §3, F-SH-01): warning at 10% of the monthly grant, danger at 0. */
export function creditLevel(e: EntitlementView | null): "normal" | "warning" | "danger" | null {
  const left = availableCredits(e);
  if (left === null) return null;
  if (left <= 0) return "danger";
  const grant = e?.credits?.grant ?? e?.monthlyGrant ?? 0;
  return grant > 0 && left * 10 <= grant ? "warning" : "normal";
}
