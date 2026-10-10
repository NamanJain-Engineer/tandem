import { useEffect, useState } from "react";
import { Gauge, RefreshCw } from "lucide-react";
import type { UsageView } from "@tandem/protocol";
import { Button, Popover, UsageMeter, cx, meterTone } from "@tandem/ui";
import { t } from "../../i18n";
import { useAccounts } from "../../stores/accounts";
import { useEngines } from "../../stores/engines";
import { useLayout } from "../../stores/layout";
import { defaultEngine } from "../../stores/launch";
import { resetClock } from "../../stores/limits";
import { fullest, useUsage } from "../../stores/usage";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";

const TONE = { muted: "text-fg-muted", warning: "text-warning-text", danger: "text-danger" };

/** The chip's text and tone for one account's usage (F-AC-04). */
export function usageChip(view: UsageView | undefined, engine: string) {
  const top = view ? fullest(view.windows) : null;
  if (!top) {
    return {
      text: t("usage.unavailable"),
      tone: "muted" as const,
      tooltip: view?.reason ?? t("usage.checking"),
    };
  }
  const pct = Math.round(top.usedPct);
  return {
    text:
      top.resetsAt !== null
        ? t("usage.chip", { engine, pct, time: resetClock(top.resetsAt) })
        : t("usage.chipNoReset", { engine, pct }),
    tone: meterTone(top.usedPct / 100, "plan"),
    tooltip: t("usage.tooltip", { window: windowName(top.kind) }),
  };
}

/** The focused agent pane's account; with nothing focused, the default
 * engine's default account (UI_SPEC §3.2). */
function useChipAccount(): string | null {
  const tabId = useWorkspaces((ws) => (ws.activeId ? (ws.activeTab[ws.activeId] ?? null) : null));
  const focused = useLayout((l) => {
    const tab = tabId ? l.tabs[tabId] : undefined;
    const pane = tab?.active ? tab.panes[tab.active] : undefined;
    return pane?.kind === "agent" ? (pane.accountId ?? null) : null;
  });
  const engine = useEngines((s) => defaultEngine(s.list));
  const fallback = useAccounts((s) => s.list.find((a) => a.engineId === engine && a.isDefault)?.id ?? null);
  return focused ?? fallback;
}

/** "5h" and "Wk" meters beside the chip on wide windows (D-113). */
const SHORT: Record<string, string> = { "5h": "5h", week: "Wk" };

/** "5-hour window", "Weekly"; other windows by their kind ("3d"). */
function windowName(kind: string): string {
  if (kind === "5h") return t("usage.fiveHour");
  if (kind === "week") return t("usage.weekly");
  return kind;
}

/** UI_SPEC title bar: usage for the focused session's account; the popover
 * lists every account's windows. */
export function UsageChip() {
  const accountId = useChipAccount();
  const accounts = useAccounts((s) => s.list);
  const usage = useUsage((s) => s.byAccount);
  const label = useEngines((s) => s.label);
  const account = accounts.find((a) => a.id === accountId);
  const [refreshing, setRefreshing] = useState(false);
  useEffect(() => {
    if (accountId) useUsage.getState().ensure(accountId);
  }, [accountId]);
  if (!accountId || !account) return null;
  const engine = label(account.engineId) ?? account.engineId;
  const view = usage[accountId];
  const chip = usageChip(view, engine);
  return (
    <Popover
      label={t("usage.popover")}
      trigger={
        <button
          type="button"
          data-testid="usage-chip"
          data-tone={chip.tone}
          title={chip.tooltip}
          className={cx(
            "inline-flex h-7 shrink-0 items-center gap-1 whitespace-nowrap rounded-sm px-2 text-sm tabular-nums hover:bg-surface-4 focus-visible:shadow-focus focus-visible:outline-none",
            TONE[chip.tone],
          )}
        >
          <Gauge size={13} aria-hidden="true" />
          {chip.text}
          {view && view.windows.length > 0 && (
            <span className="ml-1 hidden items-center gap-2 2xl:inline-flex" aria-hidden="true">
              {view.windows.map((w) => (
                <UsageMeter key={w.kind} label={SHORT[w.kind] ?? w.kind} value={w.usedPct / 100} size="xs" />
              ))}
            </span>
          )}
        </button>
      }
    >
      <div className="flex w-72 flex-col gap-3 p-3" data-testid="usage-popover">
        {accounts.map((a) => {
          const row = usage[a.id];
          return (
            <div key={a.id} className="flex flex-col gap-1">
              <span className="text-sm font-medium text-fg">
                {t("usage.accountRow", { account: a.label, engine: label(a.engineId) ?? a.engineId })}
              </span>
              {row && row.windows.length > 0 ? (
                row.windows.map((w) => (
                  <div key={w.kind} className="flex flex-col gap-0.5" data-testid="usage-window">
                    <span className="flex justify-between gap-2 text-xs text-fg-muted">
                      <span>{windowName(w.kind)}</span>
                      <span className="tabular-nums">
                        {w.resetsAt !== null
                          ? t("usage.windowResets", {
                              pct: Math.round(w.usedPct),
                              time: resetClock(w.resetsAt),
                            })
                          : t("usage.window", { pct: Math.round(w.usedPct) })}
                      </span>
                    </span>
                    <UsageMeter label={windowName(w.kind)} value={w.usedPct / 100} showLabel={false} />
                  </div>
                ))
              ) : (
                <span className="text-xs text-fg-subtle">{row?.reason ?? t("usage.checking")}</span>
              )}
            </div>
          );
        })}
        <div className="flex items-center justify-between gap-2">
          <Button size="sm" variant="ghost" onClick={() => useUi.getState().openSettings("accounts")}>
            {t("accounts.chipManage")}
          </Button>
          <Button
            size="sm"
            variant="ghost"
            loading={refreshing}
            iconLeft={<RefreshCw size={13} aria-hidden="true" />}
            data-testid="usage-refresh"
            onClick={() => {
              setRefreshing(true);
              void useUsage
                .getState()
                .refresh()
                .finally(() => setRefreshing(false));
            }}
          >
            {t("usage.refresh")}
          </Button>
        </div>
      </div>
    </Popover>
  );
}
