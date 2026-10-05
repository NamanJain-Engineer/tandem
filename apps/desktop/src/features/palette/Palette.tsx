import { Folder, Settings } from "lucide-react";
import { CommandPalette, type PaletteGroup, type PaletteItem } from "@tandem/ui";
import { t } from "../../i18n";
import { useLayout } from "../../stores/layout";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { COMMANDS, bindingFor, commandTitle, isEnabled, runCommand } from "../commands/commands";
import { keysOf } from "../commands/keys";
import { switchMode } from "../shell/actions";
import { settingsPages } from "../settings/pages";
import { useBoot } from "../shell/useBoot";

/** Mod+K (UI_SPEC §2.22). Registry commands run through `runCommand`, the
 * same path as their shortcuts (F-SH-09 AC4). */
export function Palette() {
  const { paletteOpen, setPalette, openSettings } = useUi();
  const workspaces = useWorkspaces();
  // Enabled commands depend on the focused pane and mode.
  useLayout((s) => s.tabs);
  useUi((s) => s.mode);
  const platform = useBoot((s) => s.boot?.platform ?? "linux");
  const mac = platform === "macos";
  const item = (id: string): PaletteItem | null => {
    const spec = COMMANDS.find((c) => c.id === id);
    if (!spec || !isEnabled(spec)) return null;
    const keys = bindingFor(spec, { mac, terminalFirst: false });
    return { id, label: commandTitle(id), kbd: keys ? keysOf(keys) : undefined, onSelect: () => void runCommand(id) };
  };
  const listed = (ids: string[]) => ids.map(item).filter((i): i is PaletteItem => i !== null);
  const navigation = COMMANDS.filter((c) => c.category === "navigation").map((c) => c.id);
  // The palette doesn't list itself.
  const actions = COMMANDS.filter((c) => c.category !== "navigation" && c.id !== "palette.open" && c.id !== "settings.open").map((c) => c.id);
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
        ...settingsPages(platform).map((p) => ({ id: `settings.${p.id}`, label: p.label, icon: <Settings size={16} />, keywords: p.keywords, onSelect: () => openSettings(p.id) })),
      ],
    },
  ];
  return (
    <CommandPalette
      open={paletteOpen}
      onOpenChange={setPalette}
      groups={groups}
      title={t("palette.title")}
      placeholder={t("palette.placeholder")}
      emptyText={(q) => t("palette.empty", { q })}
      recentHeading={t("palette.suggestions")}
    />
  );
}
