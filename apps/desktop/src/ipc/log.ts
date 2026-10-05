import type { LogField, LogLevel } from "@tandem/protocol";
import { invoke } from "./invoke";

/** The UI logger (F-SH-11): codes and scalar fields only, through Rust's
 * redacting logger. Never pass prompt text, terminal output or paths. */
export function log(level: LogLevel, code: string, fields: Record<string, LogField> = {}): void {
  invoke("app_log", { level, target: "ui", code, fields }).catch(() => undefined);
}

/** Routes uncaught errors to the log by type only. */
export function installGlobalErrorLogging(): void {
  window.addEventListener("error", (e) => log("error", "ui.uncaught", { kind: e.error?.name ?? "Error" }));
  window.addEventListener("unhandledrejection", (e) =>
    log("error", "ui.unhandled_rejection", {
      kind: e.reason instanceof Error ? e.reason.name : typeof e.reason,
    }),
  );
}
