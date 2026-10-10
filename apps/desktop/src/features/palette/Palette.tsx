import { useEffect, useState } from "react";
import { Folder, MessagesSquare, Settings, SquareTerminal } from "lucide-react";
import type { FindMatch, FindWorkResult, IpcError } from "@tandem/protocol";
import { CommandPalette, type PaletteGroup, type PaletteItem } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { showSource } from "../../stores/focus";
import { useLayout } from "../../stores/layout";
import { useSettings } from "../../stores/settings";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { COMMANDS, bindingFor, commandTitle, isEnabled, runCommand } from "../commands/commands";
import { keysOf } from "../commands/keys";
import { switchMode } from "../shell/actions";
import { settingsPages } from "../settings/pages";
import { useBoot } from "../shell/useBoot";

/** Waits this long after typing stops before searching. */
const FIND_DEBOUNCE_MS = 120;

type Found = { query: string; result: FindWorkResult } | { query: string; error: string };

/** "Find work…" (F-CN-05): searches the open panes and threads by meaning
 * as the user types, on this computer. */
function useFindWork(active: boolean) {
  const [query, setQuery] = useState("");
  const [found, setFound] = useState<Found | null>(null);
  const q = query.trim();
  useEffect(() => {
    if (!active || !q) {
      setFound(null);
      return;
    }
    let current = true;
    const timer = setTimeout(() => {
      invoke<FindWorkResult>("find_work", { query: q })
        .then((result) => current && setFound({ query: q, result }))
        .catch((e: IpcError) => current && setFound({ query: q, error: e.message }));
    }, FIND_DEBOUNCE_MS);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [active, q]);
  const settled = found !== null && found.query === q;
  return { query: q, setQuery, found: settled ? found : null };
}

function matchItem(m: FindMatch): PaletteItem {
  const key =
    m.source.type === "pane" ? m.source.paneId : m.source.type === "thread" ? m.source.threadId : m.label;
  const location = m.workspaceId && m.tabId ? { workspaceId: m.workspaceId, tabId: m.tabId } : null;
  return {
    id: `find.${key}`,
    label: m.label,
    description: m.detail ?? undefined,
    icon: m.kind === "pane" ? <SquareTerminal size={16} /> : <MessagesSquare size={16} />,
    onSelect: () => void showSource(m.source, location),
  };
}

/** Mod+K (UI_SPEC §2.22). Registry commands run through `runCommand`, the
 * same path as their shortcuts (F-SH-09 AC4). */
export function Palette() {
  const { paletteOpen, setPalette, openSettings } = useUi();
  const paletteMode = useUi((s) => s.paletteMode);
  const finding = paletteOpen && paletteMode === "find";
  const find = useFindWork(finding);
  const workspaces = useWorkspaces();
  // Enabled commands depend on the focused pane and mode.
  useLayout((s) => s.tabs);
  useUi((s) => s.mode);
  const platform = useBoot((s) => s.boot?.platform ?? "linux");
  const mac = platform === "macos";
  // Rebound shortcuts show as soon as they change.
  useSettings((s) => s.values.shortcut_overrides);
  const item = (id: string): PaletteItem | null => {
    const spec = COMMANDS.find((c) => c.id === id);
    if (!spec || !isEnabled(spec)) return null;
    const keys = bindingFor(spec, { mac, terminalFirst: false });
    return {
      id,
      label: commandTitle(id),
      kbd: keys ? keysOf(keys) : undefined,
      onSelect: () => void runCommand(id),
    };
  };
  const listed = (ids: string[]) => ids.map(item).filter((i): i is PaletteItem => i !== null);
  const navigation = COMMANDS.filter((c) => c.category === "navigation").map((c) => c.id);
  // The palette doesn't list itself.
  const actions = COMMANDS.filter(
    (c) => c.category !== "navigation" && c.id !== "palette.open" && c.id !== "settings.open",
  ).map((c) => c.id);
  const groups: PaletteGroup[] = [
    { heading: t("palette.goto"), items: listed(navigation) },
    {
      heading: t("palette.workspaces"),
      prefix: "/",
      items: workspaces.list.map((w) => ({
        id: `ws.${w.id}`,
        label: w.name,
        description: w.branch ?? undefined,
        icon: <Folder size={16} />,
        onSelect: () => {
          workspaces.select(w.id);
          switchMode("code");
        },
      })),
    },
    { heading: t("palette.actions"), prefix: ">", items: listed(actions) },
    {
      heading: t("palette.settings"),
      items: [
        ...listed(["settings.open"]),
        ...settingsPages(platform).map((p) => ({
          id: `settings.${p.id}`,
          label: p.label,
          icon: <Settings size={16} />,
          keywords: p.keywords,
          onSelect: () => openSettings(p.id),
        })),
      ],
    },
  ];
  if (paletteMode === "find") {
    const { found } = find;
    const matches = found && "result" in found ? found.result.matches : [];
    return (
      <CommandPalette
        open={paletteOpen}
        onOpenChange={setPalette}
        groups={matches.length ? [{ heading: t("palette.findWork"), items: matches.map(matchItem) }] : []}
        title={t("palette.findWork")}
        placeholder={t("palette.findPlaceholder")}
        emptyText={(q) =>
          !q.trim()
            ? t("palette.findHint")
            : !found
              ? t("palette.findLooking")
              : "error" in found
                ? t("palette.findFailed", { message: found.error })
                : t("palette.findNone", { q: q.trim() })
        }
        shouldFilter={false}
        onQueryChange={find.setQuery}
        resetKey="find"
      />
    );
  }
  return (
    <CommandPalette
      open={paletteOpen}
      onOpenChange={setPalette}
      groups={groups}
      title={t("palette.title")}
      placeholder={t("palette.placeholder")}
      emptyText={(q) => t("palette.empty", { q })}
      recentHeading={t("palette.suggestions")}
      resetKey="commands"
    />
  );
}
