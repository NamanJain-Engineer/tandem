// Links in terminal panes (F-CD-09, ARCHITECTURE §5.9): URLs (the web-links
// addon) and the paths programs print, with `:line[:col]`, which the core
// resolves against the pane's folder (its shell's last OSC 7, so they follow
// `cd`). Mod+Click opens a URL in the app's browser ("Open links in
// Tandem") or the system browser, and a file in the Files tab at its line,
// read-only outside every workspace.
import type { IBuffer, ILink, ILinkProvider, Terminal } from "@xterm/xterm";
import type { ResolvedLink } from "@tandem/protocol";
import { invoke } from "../../ipc/invoke";
import { contextKey, useFiles } from "../../stores/files";
import { useSettings } from "../../stores/settings";
import { useSidebar } from "../../stores/sidebar";
import { useTerminals } from "../../stores/terminals";
import { activeFiles } from "../files/context";

/** A path as programs print them: absolute, `./`, `../` or `dir/`, then
 * `:line[:col]` (ARCHITECTURE §5.9). */
const PATH_RE = /(?:[A-Za-z]:\\|\/|\.{1,2}\/|[\w.-]+\/)[^\s:'"]+(?::\d+(?::\d+)?)?/g;
/** URLs are the web-links addon's; a path inside one isn't a path. */
const URL_RE = /[a-z][a-z0-9+.-]*:\/\/[^\s'"<>]+/gi;
/** What a sentence can put after a path: `see src/a.rs.` or `(src/a.rs)`. */
const TRAILING = /[.,;:)\]}>'"!?]+$/;
/** Longer than any line worth scanning. */
const MAX_LINE = 4096;
/** A resolution is reused this long (a new cwd asks again at once). */
const CACHE_MS = 3000;

export type Candidate = { start: number; text: string };

/** The paths in a line of output, outside its URLs. */
export function pathCandidates(line: string): Candidate[] {
  const urls = Array.from(line.matchAll(URL_RE), (m) => [m.index, m.index + m[0].length] as const);
  const out: Candidate[] = [];
  for (const m of line.matchAll(PATH_RE)) {
    const text = m[0].replace(TRAILING, "");
    const start = m.index;
    const end = start + text.length;
    if (!text || urls.some(([a, b]) => start < b && end > a)) continue;
    out.push({ start, text });
  }
  return out;
}

/** Whether a click on a link opens it: Mod+Click (Cmd on macOS, Ctrl
 * elsewhere), and never while the pane is hidden or a pane is dragged. */
export function opensLink(
  event: MouseEvent,
  pane: { mac: boolean; visible: boolean; dragging: boolean },
): boolean {
  const mod = pane.mac ? event.metaKey : event.ctrlKey;
  return mod && pane.visible && !pane.dragging;
}

type Cell = { x: number; y: number };

/** The text of the wrapped line that holds buffer row `row` (0-based), with
 * each UTF-16 unit's cell (1-based, as link ranges are). */
export function logicalLine(buffer: IBuffer, row: number): { text: string; cells: Cell[] } | null {
  if (!buffer.getLine(row)) return null;
  let first = row;
  while (first > 0 && buffer.getLine(first)?.isWrapped) first--;
  let text = "";
  const cells: Cell[] = [];
  for (let y = first; text.length < MAX_LINE; y++) {
    const line = buffer.getLine(y);
    if (!line || (y > first && !line.isWrapped)) break;
    for (let x = 0; x < line.length; x++) {
      const cell = line.getCell(x);
      if (!cell) break;
      // The second half of a wide character has no text of its own.
      if (cell.getWidth() === 0) continue;
      const chars = cell.getChars() || " ";
      for (let i = 0; i < chars.length; i++) cells.push({ x: x + 1, y: y + 1 });
      text += chars;
    }
  }
  return { text, cells };
}

const cache = new Map<string, { at: number; link: Promise<ResolvedLink | null> }>();

/** What `text` in pane `paneId` opens, if anything (`pane_resolve_link`). */
export function resolveLink(paneId: string, text: string): Promise<ResolvedLink | null> {
  const cwd = useTerminals.getState().states[paneId]?.cwd ?? "";
  const key = `${paneId}\n${cwd}\n${text}`;
  const hit = cache.get(key);
  if (hit && Date.now() - hit.at < CACHE_MS) return hit.link;
  const link = invoke<ResolvedLink | null>("pane_resolve_link", { paneId, text }).catch(() => null);
  cache.set(key, { at: Date.now(), link });
  if (cache.size > 200) {
    const oldest = cache.keys().next().value;
    if (oldest !== undefined) cache.delete(oldest);
  }
  return link;
}

/** Tests: forget what was resolved. */
export function resetLinks(): void {
  cache.clear();
}

/** The paths in a terminal's output that name files, as links. */
export function pathLinkProvider(
  term: Terminal,
  resolve: (text: string) => Promise<ResolvedLink | null>,
  activate: (event: MouseEvent, link: ResolvedLink) => void,
): ILinkProvider {
  return {
    provideLinks(y, callback) {
      const line = logicalLine(term.buffer.active, y - 1);
      const found = (line ? pathCandidates(line.text) : []).flatMap((c) => {
        const start = line?.cells[c.start];
        const end = line?.cells[c.start + c.text.length - 1];
        return start && end && start.y <= y && end.y >= y ? [{ ...c, start, end }] : [];
      });
      if (found.length === 0) {
        callback(undefined);
        return;
      }
      void Promise.all(
        found.map(async (c): Promise<ILink | null> => {
          const link = await resolve(c.text);
          if (link?.kind !== "file") return null;
          return {
            range: { start: c.start, end: c.end },
            text: c.text,
            decorations: { underline: true, pointerCursor: true },
            activate: (event) => activate(event, link),
          };
        }),
      ).then((links) => {
        const real = links.filter((l): l is ILink => l !== null);
        callback(real.length > 0 ? real : undefined);
      });
    },
  };
}

/** A URL: the app's browser in the sidebar ("Open links in Tandem", on by
 * default), else the system browser. */
export function openUrl(url: string): void {
  if (useSettings.getState().values.open_links_in_tandem === false) {
    void invoke("app_open_external", { url }).catch(() => undefined);
    return;
  }
  const sidebar = useSidebar.getState();
  const id = sidebar.openTab("browser");
  sidebar.setPage(id, null, url);
}

/** A file: the Files tab opens it at its line (read-only outside every
 * workspace, as the editor says). False when no folder is showing. */
export function openFileLink(link: ResolvedLink): boolean {
  const active = activeFiles();
  if (active.kind !== "context") return false;
  useSidebar.getState().openTab("files");
  const at = link.line ? { line: link.line, col: link.col ?? undefined } : undefined;
  useFiles.getState().openLink(contextKey(active.context), link.target, at);
  return true;
}
