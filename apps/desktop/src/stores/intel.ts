// Code intelligence (F-CD-23): the editor's text goes to the folder's
// language server (debounced), whose diagnostics come back per file; go to
// definition opens the place, find references fills the References panel.
// Diffs ask the same for a name on a new-side line, in the worktree.
import { create } from "zustand";
import type { FileContent, FileContext, IpcError, LspDocument, LspLocation } from "@tandem/protocol";
import type { DiffNameTarget, DiffNames } from "@tandem/ui";
import { t } from "../i18n";
import { invoke } from "../ipc/invoke";
import { activeFiles } from "../features/files/context";
import { contextKey, contextRoot, useFiles } from "./files";
import { useSidebar } from "./sidebar";

/** The editor's text reaches the server this long after the last edit. */
export const SYNC_MS = 500;

export type At = { line: number; character: number };

export type References = {
  /** The Files context whose editor shows the panel. */
  key: string;
  name: string;
  status: "loading" | "ready" | "error";
  items: LspLocation[];
  error: string | null;
};

type IntelStore = {
  /** Per `${contextKey}\n${path}`: the server's answer for the file. */
  docs: Record<string, LspDocument>;
  /** A sync is on its way (no answer yet). */
  pending: Record<string, boolean>;
  /** A passing note in the status line ("rust-analyzer didn't answer"). */
  notice: Record<string, string>;
  refs: References | null;
  sync: (context: FileContext, path: string, text: string) => void;
  definition: (context: FileContext, path: string, text: string, at: At) => Promise<void>;
  references: (context: FileContext, path: string, text: string, at: At) => Promise<void>;
  hover: (context: FileContext, path: string, text: string, at: At) => Promise<string | null>;
  closeReferences: () => void;
  /** Trusts the file's repository for its language server (native confirm),
   * then asks again. */
  trust: (context: FileContext, path: string, text: string) => Promise<void>;
};

export function resetIntel(): void {
  for (const t of timers.values()) clearTimeout(t);
  timers.clear();
  useIntel.setState({ docs: {}, pending: {}, notice: {}, refs: null });
}

export const docId = (context: FileContext, path: string): string => `${contextKey(context)}\n${path}`;

const timers = new Map<string, ReturnType<typeof setTimeout>>();

function message(e: unknown): string {
  return (e as IpcError)?.message ?? String(e);
}

/** The name at a place in the text (as the server would read it). */
export function nameAt(text: string, at: At): string {
  const line = text.split("\n")[at.line] ?? "";
  const word = /[\p{L}\p{N}_$]/u;
  let start = Math.min(at.character, line.length);
  let end = start;
  while (start > 0 && word.test(line[start - 1] ?? "")) start--;
  while (end < line.length && word.test(line[end] ?? "")) end++;
  return line.slice(start, end);
}

/** Where a context's file opens in the Files editor: the Files tab's
 * context when the file's folder is that one or inside it, else its own. */
export function filesTarget(context: FileContext, path: string): { key: string; path: string } {
  const own = contextKey(context);
  const active = activeFiles();
  if (active.kind !== "context") return { key: own, path };
  const shown = contextKey(active.context);
  if (shown === own) return { key: own, path };
  const outer = contextRoot(active.context)?.replace(/[\\/]$/, "");
  const inner = contextRoot(context)?.replace(/[\\/]$/, "");
  // A thread in the worktree shows the same folder.
  if (outer && inner === outer) return { key: shown, path };
  if (outer && inner && inner.startsWith(`${outer}/`) && !path.startsWith("/")) {
    return { key: shown, path: `${inner.slice(outer.length + 1)}/${path}` };
  }
  return { key: own, path };
}

/** Opens a place (from a diff, or a reference) in the Files editor. */
export function openInFiles(context: FileContext, loc: LspLocation): void {
  const target = filesTarget(context, loc.path);
  useSidebar.getState().openTab("files");
  useFiles.getState().openLink(target.key, target.path, { line: loc.line + 1, col: loc.character + 1 });
}

/** A location in the open Files context: the editor's own `open`. */
function openHere(loc: LspLocation): void {
  void useFiles.getState().open(loc.path, { line: loc.line + 1, col: loc.character + 1 });
}

export const useIntel = create<IntelStore>((set, get) => {
  const note = (id: string, text: string | null) =>
    set((s) => {
      const notice = { ...s.notice };
      if (text) notice[id] = text;
      else delete notice[id];
      return { notice };
    });

  const run = async (context: FileContext, path: string, text: string) => {
    const id = docId(context, path);
    set((s) => ({ pending: { ...s.pending, [id]: true } }));
    try {
      const doc = await invoke<LspDocument | null>("lsp_sync", { context, path, text });
      if (doc) set((s) => ({ docs: { ...s.docs, [id]: doc } }));
    } catch (e) {
      note(id, message(e));
    } finally {
      set((s) => {
        const pending = { ...s.pending };
        delete pending[id];
        return { pending };
      });
    }
  };

  return {
    docs: {},
    pending: {},
    notice: {},
    refs: null,

    sync: (context, path, text) => {
      const id = docId(context, path);
      clearTimeout(timers.get(id));
      // The first sync of a file goes at once; edits wait for a pause.
      const wait = get().docs[id] ? SYNC_MS : 0;
      timers.set(
        id,
        setTimeout(() => {
          timers.delete(id);
          void run(context, path, text);
        }, wait),
      );
    },

    definition: async (context, path, text, at) => {
      const id = docId(context, path);
      note(id, null);
      try {
        const found = await invoke<LspLocation[]>("lsp_definition", { context, path, text, ...at });
        const first = found[0];
        if (!first) {
          note(id, t("intel.noDefinition", { name: nameAt(text, at) }));
          return;
        }
        if (contextKey(context) === useFiles.getState().key) openHere(first);
        else openInFiles(context, first);
      } catch (e) {
        note(id, message(e));
      }
    },

    references: async (context, path, text, at) => {
      const here = contextKey(context) === useFiles.getState().key;
      const key = here ? contextKey(context) : filesTarget(context, path).key;
      const name = nameAt(text, at);
      set({ refs: { key, name, status: "loading", items: [], error: null } });
      try {
        const found = await invoke<LspLocation[]>("lsp_references", { context, path, text, ...at });
        const items = here
          ? found
          : found.map((l) => (l.inFolder ? { ...l, path: filesTarget(context, l.path).path } : l));
        if (get().refs?.key === key) set({ refs: { key, name, status: "ready", items, error: null } });
      } catch (e) {
        if (get().refs?.key === key)
          set({ refs: { key, name, status: "error", items: [], error: message(e) } });
      }
    },

    hover: async (context, path, text, at) => {
      try {
        return await invoke<string | null>("lsp_hover", { context, path, text, ...at });
      } catch {
        return null;
      }
    },

    closeReferences: () => set({ refs: null }),

    trust: async (context, path, text) => {
      const id = docId(context, path);
      try {
        await invoke("lsp_trust", { context, path });
      } catch (e) {
        if ((e as IpcError)?.code !== "cancelled") note(id, message(e));
        return;
      }
      note(id, null);
      await run(context, path, text);
    },
  };
});

/** Opens a reference in the Files editor showing the panel. */
export function openReference(loc: LspLocation): void {
  openHere(loc);
}

/** A diff's file text from disk (the new side is the worktree's file). */
export async function diskText(context: FileContext, path: string): Promise<string | null> {
  try {
    const c = await invoke<FileContent>("fs_read", { context, path });
    return c.text;
  } catch {
    return null;
  }
}

/** A diff's Go to definition and Find references (F-CD-23), in the folder
 * of `context`; `onLeave` closes what covers the Files editor. */
export function diffNames(context: FileContext, onLeave?: () => void): DiffNames {
  const ask = async (which: "definition" | "references", tg: DiffNameTarget) => {
    const text = await diskText(context, tg.path);
    if (text === null) return;
    onLeave?.();
    const at = { line: tg.line - 1, character: tg.character };
    // The references panel sits under the clicked line's file.
    if (which === "references")
      openInFiles(context, {
        path: tg.path,
        inFolder: true,
        line: at.line,
        character: at.character,
        endLine: at.line,
        endCharacter: at.character,
        preview: "",
      });
    await useIntel.getState()[which](context, tg.path, text, at);
  };
  return {
    definition: t("intel.diff.definition"),
    references: t("intel.diff.references"),
    onDefinition: (tg) => void ask("definition", tg),
    onReferences: (tg) => void ask("references", tg),
  };
}
