import type { EngineView, PaneRecord } from "@tandem/protocol";
import { t } from "../../i18n";

/** Pane titles default to "{Engine} · n" ("Shell · n" for terminals),
 * numbered per engine in creation order (UI-11). A pane's own name wins. */
export function paneTitles(panes: Record<string, PaneRecord>, engines: EngineView[]): Record<string, string> {
  const label = (id: string | null) => engines.find((e) => e.spec.id === id)?.spec.label ?? null;
  const sorted = Object.values(panes).sort((a, b) => a.createdAt - b.createdAt || a.id.localeCompare(b.id));
  const counts = new Map<string, number>();
  return Object.fromEntries(
    sorted.map((p) => {
      const key = p.kind === "agent" ? (p.engineId ?? "agent") : "shell";
      const n = (counts.get(key) ?? 0) + 1;
      counts.set(key, n);
      const engine = p.kind === "agent" ? (label(p.engineId) ?? p.engineId ?? "Agent") : null;
      const fallback = engine ? t("pane.engine", { engine, n }) : t("pane.shell", { n });
      return [p.id, p.humanName ?? p.title ?? fallback];
    }),
  );
}
