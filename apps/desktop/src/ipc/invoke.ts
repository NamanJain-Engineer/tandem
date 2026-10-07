import type { IpcError } from "@tandem/protocol";

export const DEFAULT_TIMEOUT_MS = 10_000;

type InvokeFn = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;

let backend: InvokeFn | null = null;

/** Tests and the browser preview replace the Tauri bridge with a fake. */
export function setInvokeBackend(fn: InvokeFn | null): void {
  backend = fn;
}

/** True when a test or preview backend replaced the Tauri bridge. */
export function usingFakeBackend(): boolean {
  return backend !== null;
}

export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function resolveBackend(): Promise<InvokeFn> {
  if (backend) return backend;
  if (inTauri()) {
    const core = await import("@tauri-apps/api/core");
    return (cmd, args) => core.invoke(cmd, args);
  }
  throw toIpcError({ code: "unavailable", message: "Vibezeo's core isn't running." });
}

function toIpcError(e: unknown): IpcError {
  if (e && typeof e === "object" && "code" in e && "message" in e) {
    const err = e as Partial<IpcError>;
    return {
      code: err.code ?? "internal",
      message: err.message ?? "Something went wrong.",
      stage: err.stage ?? null,
      hint: err.hint ?? null,
      retryable: err.retryable ?? false,
    };
  }
  return { code: "internal", message: String(e), stage: null, hint: null, retryable: false };
}

/** Calls a Tauri command with a timeout (PRODUCT_SPEC §5.2). Rejects with an IpcError. */
export async function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
  { timeoutMs = DEFAULT_TIMEOUT_MS }: { timeoutMs?: number } = {},
): Promise<T> {
  const call = await resolveBackend();
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(
      () => reject(toIpcError({ code: "timeout", message: `${cmd} took too long.`, retryable: true })),
      timeoutMs,
    );
  });
  try {
    return (await Promise.race([call(cmd, args), timeout])) as T;
  } catch (e) {
    throw toIpcError(e);
  } finally {
    clearTimeout(timer);
  }
}
