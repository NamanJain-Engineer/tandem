import { useEffect, useState, type ReactNode } from "react";
import { SquareTerminal } from "lucide-react";
import type { IpcError } from "@tandem/protocol";
import { Button, Callout, Dialog, Spinner, cx, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { useLayout } from "../../stores/layout";
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

/** Launch tiles: Shell now; engines join in M4 and Browser in M9 (UI-10). */
export function useTiles(workspaceName: string): Tile[] {
  return [
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

  const pick = async () => {
    if (!picker) return;
    setBusy("shell");
    const res = await split(picker.tabId, picker.target, picker.side);
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
            <LaunchTiles tiles={tiles} busy={busy} onPick={() => void pick()} autoFocus />
            <p className="text-sm text-fg-subtle">{t("picker.more")}</p>
          </>
        )}
      </div>
    </Dialog>
  );
}
