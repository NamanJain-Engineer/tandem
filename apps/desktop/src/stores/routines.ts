// Routines (F-RT-01..04, UI-50..53): the list with what each row shows, the
// background runner's state, and the actions. Kept fresh from
// `routines:changed` and `routines:run`.
import { create } from "zustand";
import type {
  IpcError,
  LauncherStatus,
  Routine,
  RoutineInput,
  RoutineView,
  RunPage,
  SchedulePreview,
  ScheduleSpec,
} from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";

type Status = "idle" | "loading" | "ready" | "error";

type RoutinesState = {
  list: RoutineView[];
  status: Status;
  error: string | null;
  background: LauncherStatus | null;
  load: () => Promise<void>;
  save: (input: RoutineInput) => Promise<Routine | IpcError>;
  remove: (id: string) => Promise<IpcError | null>;
  pause: (id: string, paused: boolean) => Promise<IpcError | null>;
  runNow: (id: string) => Promise<IpcError | null>;
  setBackground: (enabled: boolean) => Promise<IpcError | null>;
};

export function asIpcError(e: unknown): IpcError {
  return !!e && typeof e === "object" && "code" in e && "message" in e
    ? (e as IpcError)
    : { code: "internal", message: String(e), stage: null, hint: null, retryable: false };
}

export function isIpcError(e: unknown): e is IpcError {
  return !!e && typeof e === "object" && "code" in e && "message" in e;
}

/** The schedule preview: words, the next 3 runs, or why it can't run. */
export async function previewSchedule(spec: ScheduleSpec): Promise<SchedulePreview | IpcError> {
  try {
    return await invoke<SchedulePreview>("routine_preview_schedule", { spec });
  } catch (e) {
    return asIpcError(e);
  }
}

export async function routineRuns(id: string, before: number | null): Promise<RunPage | IpcError> {
  try {
    return await invoke<RunPage>("routine_runs", { id, before });
  } catch (e) {
    return asIpcError(e);
  }
}

let listening: Promise<void> | null = null;
let unlisten: Array<() => void> = [];

async function act(cmd: string, args: Record<string, unknown>): Promise<IpcError | null> {
  try {
    await invoke(cmd, args);
    await useRoutines.getState().load();
    return null;
  } catch (e) {
    return asIpcError(e);
  }
}

export const useRoutines = create<RoutinesState>((set, get) => ({
  list: [],
  status: "idle",
  error: null,
  background: null,
  load: async () => {
    listening ??= Promise.all([
      listen("routines:changed", () => void get().load()),
      listen("routines:run", () => void get().load()),
    ]).then((u) => {
      unlisten = u;
    });
    if (get().status !== "ready") set({ status: "loading" });
    try {
      const [list, background] = await Promise.all([
        invoke<RoutineView[]>("routines_list"),
        invoke<LauncherStatus>("routines_background_status").catch(() => null),
      ]);
      set({ list: list ?? [], background, status: "ready", error: null });
    } catch (e) {
      set({ status: "error", error: asIpcError(e).message });
    }
  },
  save: async (input) => {
    try {
      const r = await invoke<Routine>("routine_save", { input });
      await get().load();
      return r;
    } catch (e) {
      return asIpcError(e);
    }
  },
  remove: (id) => act("routine_delete", { id }),
  pause: (id, paused) => act("routine_pause", { id, paused }),
  runNow: (id) => act("routine_run_now", { id }),
  setBackground: async (enabled) => {
    try {
      const background = await invoke<LauncherStatus>("routines_background_set", { enabled });
      set({ background });
      return null;
    } catch (e) {
      return asIpcError(e);
    }
  },
}));

/** Tests: a fresh list. */
export function resetRoutines(): void {
  for (const u of unlisten) u();
  unlisten = [];
  listening = null;
  useRoutines.setState({ list: [], status: "idle", error: null, background: null });
}
