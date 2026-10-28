import { useEffect } from "react";
import { Radar } from "lucide-react";
import type { WorkspaceSummary } from "@tandem/protocol";
import { Badge, cx, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { listen } from "../../ipc/events";
import { useDashboard } from "../../stores/dashboard";
import { loadRadar, radarNote, useWorktrees } from "../../stores/worktrees";
import { worktreeAgents } from "./WorktreeReview";
import { useWorkspaceStatus } from "./workspaceStatus";

type Side = { laneId: string; branch: string | null };
type Overlap = { a: Side; b: Side; symbols: string[] };

/** The rail row's radar mark (F-WT-10): what the worktree shares with a
 * sibling; it opens Check overlap. Beside the row's status ("● 1 working",
 * "Needs you", "Failed") it's the radar alone, its words kept as its name and
 * title, so the row's name keeps its room (D-2603, D-2611); otherwise its
 * words follow the radar, cut short when the row needs the room (D-2613). */
export function RadarMark({ wt }: { wt: WorkspaceSummary }) {
  const symbols = useWorktrees((s) => s.radar[wt.id]);
  const status = useWorkspaceStatus(wt);
  if (!symbols || symbols.length === 0) return null;
  const label =
    symbols.length === 1
      ? t("radar.mark.one", { symbol: symbols[0] })
      : t("radar.mark.many", { n: symbols.length });
  const compact = status.waiting || status.working > 0 || status.failed;
  return (
    <button
      type="button"
      data-testid="worktree-radar"
      data-compact={compact ? "" : undefined}
      title={t("radar.mark.title", { symbols: symbols.join(", ") })}
      onClick={(e) => {
        e.stopPropagation();
        useWorktrees.getState().open({ kind: "overlap", worktreeId: wt.id });
      }}
      onKeyDown={(e) => e.stopPropagation()}
      className={cx(
        "flex min-w-0 rounded-xs focus-visible:shadow-focus focus-visible:outline-none",
        compact ? "shrink-0" : "shrink",
      )}
    >
      <Badge variant="waiting" className="min-w-0 max-w-full">
        <Radar size={10} aria-hidden="true" className="shrink-0" />
        <span className={compact ? "sr-only" : "truncate"}>{label}</span>
      </Badge>
    </button>
  );
}

/** Types the core's note into the worktree's first agent; false when none runs there. */
async function warn(laneId: string, otherId: string): Promise<"sent" | "no_agent" | string> {
  const agent = worktreeAgents(useDashboard.getState().items, laneId)[0];
  if (!agent || agent.source.type !== "pane") return "no_agent";
  const note = await radarNote(laneId, otherId);
  if (!note.ok) return note.error.message;
  try {
    await invoke("pty_type_prompt", { paneId: agent.source.paneId, text: note.value });
    return "sent";
  } catch (e) {
    return (e as { message?: string }).message ?? String(e);
  }
}

/** F-WT-10: keeps the rail's marks fresh and shows the notice when a
 * checkpoint makes two worktrees share symbols. Nothing is typed into an
 * agent without the click on Warn both agents. */
export function useIntentRadar(laneIds: string[], active: boolean) {
  const { show } = useToast();
  const key = laneIds.join(",");
  useEffect(() => {
    if (!active || !key) return;
    const ids = key.split(",");
    void loadRadar(ids);
    let live = true;
    let off: (() => void) | undefined;
    void listen<Overlap>("radar:overlap", (p) => {
      if (!live) return;
      void loadRadar(ids);
      const branches = { a: p.a.branch ?? "", b: p.b.branch ?? "" };
      show({
        tone: "warning",
        title: t("radar.notice.title", { a: branches.a, b: branches.b, symbols: p.symbols.join(", ") }),
        body: t("radar.notice.body"),
        duration: 20_000,
        action: {
          label: t("radar.notice.warn"),
          onClick: () => {
            void Promise.all([warn(p.a.laneId, p.b.laneId), warn(p.b.laneId, p.a.laneId)]).then((results) => {
              const missing = results
                .map((r, i) => (r === "no_agent" ? (i === 0 ? branches.a : branches.b) : null))
                .filter((x): x is string => x !== null);
              const failed = results.find((r) => r !== "sent" && r !== "no_agent");
              if (failed) show({ tone: "error", title: t("radar.notice.failed", { reason: failed }) });
              else if (missing.length > 0)
                show({ tone: "warning", title: t("radar.notice.noAgent", { branch: missing.join(", ") }) });
              else show({ tone: "success", title: t("radar.notice.warned") });
            });
          },
        },
      });
    }).then((u) => {
      if (live) off = u;
      else u();
    });
    return () => {
      live = false;
      off?.();
    };
  }, [key, active, show]);
}
