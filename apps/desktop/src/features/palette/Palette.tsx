import { Folder, FolderPlus, Gem, PanelLeft, Settings, Stethoscope } from "lucide-react";
import { CommandPalette, useToast, type PaletteGroup } from "@tandem/ui";
import { t } from "../../i18n";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { copyDiagnostics, pickAndAddWorkspace, switchMode } from "../shell/actions";
import { settingsPages } from "../settings/pages";
import { useBoot } from "../shell/useBoot";

/** Mod+K (UI_SPEC §2.22) with the M2 command set. */
export function Palette() {
  const { paletteOpen, setPalette, openSettings, toggleRail, openUpgrade } = useUi();
  const workspaces = useWorkspaces();
  const platform = useBoot((s) => s.boot?.platform ?? "linux");
  const { show } = useToast();
  const groups: PaletteGroup[] = [
    {
      heading: t("palette.goto"),
      items: [
        { id: "mode.agent", label: t("titlebar.mode.tooltip", { mode: t("titlebar.mode.agent") }), kbd: ["Mod", "1"], onSelect: () => switchMode("agent") },
        { id: "mode.code", label: t("titlebar.mode.tooltip", { mode: t("titlebar.mode.code") }), kbd: ["Mod", "2"], onSelect: () => switchMode("code") },
        { id: "mode.thread", label: t("titlebar.mode.tooltip", { mode: t("titlebar.mode.thread") }), kbd: ["Mod", "3"], onSelect: () => switchMode("thread") },
      ],
    },
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
    {
      heading: t("palette.actions"),
      prefix: ">",
      items: [
        { id: "ws.add", label: t("rail.addFolder"), icon: <FolderPlus size={16} />, onSelect: () => void pickAndAddWorkspace(show) },
        { id: "rail.toggle", label: t("titlebar.toggleRail"), icon: <PanelLeft size={16} />, kbd: ["Mod", "\\"], onSelect: toggleRail },
        { id: "diag.copy", label: t("menu.copyDiagnostics"), icon: <Stethoscope size={16} />, onSelect: () => void copyDiagnostics(show) },
        { id: "upgrade", label: t("palette.upgrade"), icon: <Gem size={16} />, onSelect: () => openUpgrade() },
      ],
    },
    {
      heading: t("palette.settings"),
      items: settingsPages(platform).map((p) => ({ id: `settings.${p.id}`, label: p.label, icon: <Settings size={16} />, keywords: p.keywords, onSelect: () => openSettings(p.id) })),
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
