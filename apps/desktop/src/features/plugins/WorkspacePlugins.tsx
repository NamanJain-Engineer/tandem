import { useEffect, useState } from "react";
import type { IpcError, PluginCard } from "@tandem/protocol";
import { Button, Dialog, EmptyState, Skeleton, Switch, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useUi } from "../../stores/ui";

/** The workspace row's menu → "Plugins for this workspace…" (F-PL-05):
 * connected plugins every thread in the workspace and every agent terminal
 * started in it may use, none by default. A worktree's dialog shows its
 * folder's grants on and locked, since its sessions use them too. A change
 * reaches a thread's next message and a terminal's next start (F-PL-01). */
export function WorkspacePlugins({
  workspaceId,
  folder,
  onClose,
}: {
  workspaceId: string;
  /** The folder a worktree came from, whose grants it has as well. */
  folder: { id: string; name: string } | null;
  onClose: () => void;
}) {
  const { show } = useToast();
  const [cards, setCards] = useState<PluginCard[] | null>(null);
  const [mine, setMine] = useState<string[]>([]);
  const [fromFolder, setFromFolder] = useState<string[]>([]);
  const folderId = folder?.id ?? null;

  useEffect(() => {
    setCards(null);
    void Promise.all([
      invoke<PluginCard[]>("plugins_catalog"),
      invoke<string[]>("grants_list", { subject: { type: "workspace", workspaceId } }),
      folderId
        ? invoke<string[]>("grants_list", { subject: { type: "workspace", workspaceId: folderId } })
        : Promise.resolve([]),
    ]).then(
      ([c, own, inherited]) => {
        setCards(c.filter((x) => x.state === "connected" && x.connectionId));
        setMine(own);
        setFromFolder(inherited);
      },
      (e: IpcError) => {
        setCards([]);
        show({ tone: "error", title: e.message });
      },
    );
  }, [workspaceId, folderId, show]);

  const toggle = (connectionId: string, granted: boolean) => {
    setMine((m) => (granted ? [...m, connectionId] : m.filter((x) => x !== connectionId)));
    void invoke("grants_set", { subject: { type: "workspace", workspaceId }, connectionId, granted }).catch(
      (e: IpcError) => {
        setMine((m) => (granted ? m.filter((x) => x !== connectionId) : [...m, connectionId]));
        show({ tone: "error", title: e.message });
      },
    );
  };

  const body = (() => {
    if (!cards) return <Skeleton height={64} />;
    if (cards.length === 0)
      return (
        <EmptyState
          title={t("threadPlugins.none")}
          body={t("threadPlugins.noneBody")}
          primary={
            <Button
              size="sm"
              variant="secondary"
              onClick={() => {
                onClose();
                useUi.getState().openSettings("plugins");
              }}
            >
              {t("threadPlugins.openSettings")}
            </Button>
          }
        />
      );
    return (
      <div className="flex flex-col gap-1" data-testid="workspace-plugins">
        {cards.map((c) => {
          const id = c.connectionId ?? "";
          const inherited = folder !== null && fromFolder.includes(id);
          return (
            <div key={id} className="flex flex-col" data-testid={`workspace-plugin-${c.pluginId}`}>
              <Switch
                label={c.accountLabel ? `${c.label} · ${c.accountLabel}` : c.label}
                checked={inherited || mine.includes(id)}
                disabled={inherited}
                onCheckedChange={(v) => toggle(id, v)}
              />
              {inherited && folder && (
                <span className="pl-9 text-sm text-fg-subtle">
                  {t("workspacePlugins.fromFolder", { folder: folder.name })}
                </span>
              )}
            </div>
          );
        })}
        <p className="pt-1 text-sm text-fg-muted">{t("workspacePlugins.nextStart")}</p>
      </div>
    );
  })();

  return (
    <Dialog open onOpenChange={(v) => !v && onClose()} title={t("workspacePlugins.title")} size="sm">
      {body}
    </Dialog>
  );
}
