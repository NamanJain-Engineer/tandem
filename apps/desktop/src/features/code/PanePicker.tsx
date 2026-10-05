import { useEffect, useState, type ReactNode } from "react";
import { Bot, SquareTerminal } from "lucide-react";
import type { IpcError } from "@tandem/protocol";
import { Button, Callout, Dialog, Spinner, cx, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { useEngines } from "../../stores/engines";
import { useLayout, type Launch } from "../../stores/layout";
import { useSession } from "../../stores/session";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";

/** Live panes across every workspace, from the core's counts. */
function liveTotal(): number {
  return useWorkspaces.getState().list.reduce((n, w) => n + w.liveCount, 0);
}

export function atPaneLimit(): boolean {
  const limit = useSession.getState().entitlement?.limits.panes ?? null;
  return limit !== null && liveTotal() >= limit;
}

/** "Free includes 4 live panes" (UI_SPEC copy table). No process starts. */
export function PaneLimitCard({ onManage }: { onManage: () => void }) {
  const limit = useSession((s) => s.entitlement?.limits.panes ?? 4);
  const openUpgrade = useUi((s) => s.openUpgrade);
  return (
    <Callout
      tone="accent"
      title={t("limit.panes.title", { n: limit })}
      actions={
        <>
          <Button size="sm" variant="primary" onClick={() => openUpgrade({ kind: "panes", limit })}>
            {t("limit.panes.plans")}
          </Button>
          <Button size="sm" variant="ghost" onClick={onManage}>
            {t("limit.panes.manage")}
          </Button>
        </>
      }
    >
      <p data-testid="pane-limit">{t("limit.panes.body")}</p>
    </Callout>
  );
}

export type Tile = { id: string; label: string; body: string; icon: ReactNode };

export function launchFor(tileId: string): Launch {
  return tileId === "shell" ? { kind: "terminal" } : { kind: "agent", engineId: tileId };
}

/** Launch tiles (UI-10): installed engines, then Shell (always present).
 * Browser joins in M9. */
export function useTiles(workspaceName: string): Tile[] {
  const engines = useEngines((s) => s.list);
  const loaded = useEngines((s) => s.loaded);
  useEffect(() => {
    if (!loaded) void useEngines.getState().refresh();
  }, [loaded]);
  return [
    ...engines
      .filter((e) => e.install)
      .map((e) => ({
        id: e.spec.id,
        label: e.spec.label,
        body: t("picker.agent.body", { name: workspaceName }),
        icon: <Bot size={18} />,
      })),
    {
      id: "shell",
      label: t("picker.shell"),
      body: t("picker.shell.body", { name: workspaceName }),
      icon: <SquareTerminal size={18} />,
    },
  ];
}

export function LaunchTiles({
  tiles,
  onPick,
  busy,
  autoFocus,
}: {
  tiles: Tile[];
  onPick: (id: string) => void;
  busy: string | null;
  autoFocus?: boolean;
}) {
  return (
    <ul className="grid grid-cols-[repeat(auto-fill,minmax(132px,1fr))] gap-2">
      {tiles.map((tile, i) => (
        <li key={tile.id}>
          <button
            type="button"
            data-testid={`tile-${tile.id}`}
            autoFocus={autoFocus && i === 0}
            disabled={busy !== null}
            onClick={() => onPick(tile.id)}
            className={cx(
              "flex h-[88px] w-full flex-col items-start justify-between rounded-md border border-border bg-surface-2 p-3 text-left",
              "hover:border-border-strong hover:bg-surface-3 focus-visible:shadow-focus focus-visible:outline-none disabled:opacity-60",
            )}
          >
            <span className="inline-flex text-fg-muted">{busy === tile.id ? <Spinner /> : tile.icon}</span>
            <span className="flex flex-col">
              <span className="text-base font-medium text-fg">{tile.label}</span>
              <span className="truncate text-sm text-fg-subtle">{tile.body}</span>
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
}

/** The picker a split opens (F-CD-02). At the live-pane limit it shows the
 * upgrade card instead, and nothing is spawned (F-CD-03). */
export function PanePicker({ workspaceName }: { workspaceName: string }) {
  const picker = useLayout((s) => s.picker);
  const closePicker = useLayout((s) => s.closePicker);
  const split = useLayout((s) => s.split);
  const tiles = useTiles(workspaceName);
  const { show } = useToast();
  const [limited, setLimited] = useState(false);
  const [checking, setChecking] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    if (!picker) return;
    void useEngines.getState().refresh();
    setBusy(null);
    setLimited(atPaneLimit());
    setChecking(true);
    void useWorkspaces
      .getState()
      .refresh()
      .catch(() => undefined)
      .finally(() => {
        setLimited(atPaneLimit());
        setChecking(false);
      });
  }, [picker]);

  const pick = async (tileId: string) => {
    if (!picker) return;
    setBusy(tileId);
    const res = await split(picker.tabId, picker.target, picker.side, launchFor(tileId));
    setBusy(null);
    if (res.ok) {
      closePicker();
      void useWorkspaces
        .getState()
        .refresh()
        .catch(() => undefined);
    } else if ((res.error as IpcError).code === "limit_reached") {
      setLimited(true);
    } else {
      show({ tone: "error", title: res.error.message });
      closePicker();
    }
  };

  return (
    <Dialog
      open={picker !== null}
      onOpenChange={(o) => !o && closePicker()}
      title={t("picker.title")}
      size="sm"
    >
      <div data-testid="pane-picker" className="flex flex-col gap-3">
        {limited ? (
          <PaneLimitCard onManage={closePicker} />
        ) : checking && tiles.length === 0 ? (
          <Spinner />
        ) : (
          <>
            <LaunchTiles tiles={tiles} busy={busy} onPick={(id) => void pick(id)} autoFocus />
            <p className="text-sm text-fg-subtle">{t("picker.more")}</p>
          </>
        )}
      </div>
    </Dialog>
  );
}
