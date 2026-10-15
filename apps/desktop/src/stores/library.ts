import { create } from "zustand";
import type {
  IpcError,
  LibraryListing,
  LibraryOpened,
  LibrarySession,
  TranscriptItem,
} from "@tandem/protocol";
import { invoke } from "../ipc/invoke";
import type { Outcome } from "./launch";

async function run<T>(cmd: string, args: Record<string, unknown>): Promise<Outcome<T>> {
  try {
    return { ok: true, value: await invoke<T>(cmd, args) };
  } catch (e) {
    return { ok: false, error: e as IpcError };
  }
}

/** The session library (F-TH-17, UI-162). */
type LibraryState = {
  open: boolean;
  show: () => void;
  hide: () => void;
};

export const useLibrary = create<LibraryState>((set) => ({
  open: false,
  show: () => set({ open: true }),
  hide: () => set({ open: false }),
}));

export const listLibrary = () => run<LibraryListing>("library_list", {});
export const readTranscript = (key: string) => run<TranscriptItem[]>("library_transcript", { key });
export const resumeSession = (key: string) => run<LibraryOpened>("library_resume", { key });
export const searchLibrary = (query: string) => run<string[]>("library_search", { query });
export const setLibrarySearch = (on: boolean) => run<null>("library_search_set", { on });

/** Sessions grouped by repository, groups in order of their newest
 * session; sessions without a folder go last. */
export function groups(
  sessions: LibrarySession[],
): Array<{ repository: string | null; sessions: LibrarySession[] }> {
  const out = new Map<string | null, LibrarySession[]>();
  for (const s of sessions) {
    const key = s.repository ?? null;
    const list = out.get(key) ?? [];
    list.push(s);
    out.set(key, list);
  }
  return [...out.entries()]
    .map(([repository, list]) => ({ repository, sessions: list }))
    .sort((a, b) =>
      a.repository === null
        ? 1
        : b.repository === null
          ? -1
          : b.sessions[0]!.modifiedAt - a.sessions[0]!.modifiedAt,
    );
}

/** A repository's name for its group header. */
export const repoName = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;
