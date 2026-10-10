import type { IpcError } from "@tandem/protocol";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useUi, type Mode } from "../../stores/ui";
import { useSettings } from "../../stores/settings";
import { useSession } from "../../stores/session";
import { useWorkspaces } from "../../stores/workspaces";

type Toast = (t: { tone?: "info" | "success" | "warning" | "error"; title: string; body?: string }) => void;

export function switchMode(mode: Mode): void {
  useUi.getState().setMode(mode);
  if (useUi.getState().settingsOpen) useUi.getState().closeSettings();
  void useSettings.getState().set("last_mode", mode);
  invoke("telemetry_capture", { event: "mode_switched", props: { mode } }).catch(() => undefined);
}

/** Adds a folder; a plan limit opens the upgrade sheet (F-BL-04). */
export async function addWorkspace(root: string, toast: Toast): Promise<boolean> {
  const result = await useWorkspaces.getState().add(root);
  if (result.ok) {
    switchMode("code");
    if (!result.added.created) toast({ title: t("deeplink.opened", { name: result.added.workspace.name }) });
    return true;
  }
  if (result.limit) {
    const limit = useSession.getState().entitlement?.limits.workspaces ?? 3;
    invoke("telemetry_capture", { event: "limit_hit", props: { limit: "workspaces" } }).catch(
      () => undefined,
    );
    useUi.getState().openUpgrade({ kind: "workspaces", limit }, () => void addWorkspace(root, toast));
    return false;
  }
  toast({ tone: "error", title: t("code.addFailed"), body: (result.error as IpcError).message });
  return false;
}

export async function pickAndAddWorkspace(toast: Toast): Promise<void> {
  const picked = await invoke<string | null>("workspace_pick_folder", undefined, {
    timeoutMs: 24 * 3_600_000,
  }).catch(() => null);
  if (picked) await addWorkspace(picked, toast);
}

export async function copyDiagnostics(toast: Toast): Promise<void> {
  try {
    await invoke<string>("app_copy_diagnostics");
    toast({ tone: "success", title: t("settings.diagnosticsCopied") });
  } catch (e) {
    toast({ tone: "error", title: (e as IpcError).message });
  }
}

export async function openBilling(
  target: { type: "upgrade"; plan: string; interval: string } | { type: "portal" } | { type: "credits" } | { type: "account" },
  toast: Toast,
): Promise<boolean> {
  try {
    await invoke("billing_open", { target }, { timeoutMs: 30_000 });
    return true;
  } catch (e) {
    toast({ tone: "error", title: (e as IpcError).message });
    return false;
  }
}
