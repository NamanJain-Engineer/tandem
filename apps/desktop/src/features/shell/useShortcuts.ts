import { useEffect } from "react";
import { useSetting } from "../../stores/settings";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { switchMode } from "./actions";
import { useBoot } from "./useBoot";

/** App shortcuts (UI_SPEC §5, M2 set). With a terminal focused on Windows
 * and Linux, "Terminal first" leaves shell keys like Ctrl+K to the shell. */
export function useShortcuts(): void {
  const platform = useBoot((s) => s.boot?.platform ?? "linux");
  const priority = useSetting("term_shortcut_priority", "terminal");
  useEffect(() => {
    const mac = platform === "macos";
    const onKey = (e: KeyboardEvent) => {
      const mod = mac ? e.metaKey : e.ctrlKey;
      if (!mod || e.altKey) return;
      const inTerminal = !!(e.target as HTMLElement | null)?.closest?.(".xterm");
      const terminalFirst = !mac && inTerminal && priority === "terminal";
      const ui = useUi.getState();
      const key = e.key.toLowerCase();
      const handled = (() => {
        if (key === "," && !e.shiftKey) return ui.openSettings(), true;
        if (key === "\\" && !e.shiftKey) return ui.toggleRail(), true;
        if (key === "b" && e.shiftKey) return ui.toggleSidebar(), true;
        if (!e.shiftKey && ["1", "2", "3"].includes(key)) return switchMode(key === "1" ? "agent" : key === "2" ? "code" : "thread"), true;
        if (terminalFirst) return false;
        if (key === "k" && !e.shiftKey) return ui.setPalette(!ui.paletteOpen), true;
        if (key === "t" && !e.shiftKey && ui.mode === "code" && !ui.settingsOpen) {
          const ws = useWorkspaces.getState();
          if (ws.activeId) void ws.createTab(ws.activeId);
          return true;
        }
        return false;
      })();
      if (handled) {
        e.preventDefault();
        e.stopPropagation();
      }
    };
    window.addEventListener("keydown", onKey, { capture: true });
    return () => window.removeEventListener("keydown", onKey, { capture: true });
  }, [platform, priority]);
}
