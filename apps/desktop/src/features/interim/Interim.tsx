import { Bot, MessagesSquare } from "lucide-react";
import { Button, EmptyState } from "@tandem/ui";
import { t } from "../../i18n";
import { switchMode } from "../shell/actions";

/** Agent and Thread modes until M7/M9 (MILESTONES M2 slice of F-SH-02). */
export function InterimMain({ mode }: { mode: "agent" | "thread" }) {
  return (
    <div className="flex h-full items-center justify-center">
      <EmptyState
        icon={mode === "agent" ? <Bot size={20} /> : <MessagesSquare size={20} />}
        title={mode === "agent" ? t("interim.agent.title") : t("interim.thread.title")}
        body={mode === "agent" ? t("interim.agent.body") : t("interim.thread.body")}
        primary={
          <Button variant="primary" onClick={() => switchMode("code")}>
            {t("interim.openCode")}
          </Button>
        }
      />
    </div>
  );
}

export function InterimRail({ mode, collapsed }: { mode: "agent" | "thread"; collapsed: boolean }) {
  const label = mode === "agent" ? t("rail.teammates") : t("rail.threads");
  return (
    <nav aria-label={label} className={collapsed ? "w-[var(--rail-collapsed)] shrink-0 border-r border-border bg-surface-1" : "w-[var(--rail-w)] shrink-0 border-r border-border bg-surface-1"}>
      {!collapsed && <h2 className="px-3 pt-3 text-2xs font-semibold uppercase tracking-caps text-fg-subtle">{label.toUpperCase()}</h2>}
    </nav>
  );
}
