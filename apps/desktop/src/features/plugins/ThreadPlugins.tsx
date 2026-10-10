import { useEffect, useState } from "react";
import type { IpcError, PluginCard } from "@tandem/protocol";
import { Button, Dialog, EmptyState, Skeleton, Switch, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useUi } from "../../stores/ui";

/** Attach ▾ → "Plugins for this thread" (F-TH-07, F-PL-05): connected
 * plugins this thread may use, none by default. Plugins granted to every
 * thread show on and locked. A change reaches the thread's next turn. */
export function ThreadPlugins({
  threadId,
  open,
  onOpenChange,
}: {
  threadId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { show } = useToast();
  const [cards, setCards] = useState<PluginCard[] | null>(null);
  const [mine, setMine] = useState<string[]>([]);
  const [everyThread, setEveryThread] = useState<string[]>([]);

  useEffect(() => {
    if (!open) return;
    setCards(null);
    void Promise.all([
      invoke<PluginCard[]>("plugins_catalog"),
      invoke<string[]>("grants_list", { subject: { type: "thread", threadId } }),
      invoke<string[]>("grants_list", { subject: { type: "all_threads" } }),
    ]).then(
      ([c, own, all]) => {
        setCards(c.filter((x) => x.state === "connected" && x.connectionId));
        setMine(own);
        setEveryThread(all);
      },
      (e: IpcError) => {
        setCards([]);
        show({ tone: "error", title: e.message });
      },
    );
  }, [open, threadId, show]);

  const toggle = (connectionId: string, granted: boolean) => {
    setMine((m) => (granted ? [...m, connectionId] : m.filter((x) => x !== connectionId)));
    void invoke("grants_set", { subject: { type: "thread", threadId }, connectionId, granted }).catch(
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
                onOpenChange(false);
                useUi.getState().openSettings("plugins");
              }}
            >
              {t("threadPlugins.openSettings")}
            </Button>
          }
        />
      );
    return (
      <div className="flex flex-col gap-1" data-testid="thread-plugins">
        {cards.map((c) => {
          const id = c.connectionId ?? "";
          const all = everyThread.includes(id);
          return (
            <div key={id} className="flex flex-col" data-testid={`thread-plugin-${c.pluginId}`}>
              <Switch
                label={c.accountLabel ? `${c.label} · ${c.accountLabel}` : c.label}
                checked={all || mine.includes(id)}
                disabled={all}
                onCheckedChange={(v) => toggle(id, v)}
              />
              {all && <span className="pl-9 text-sm text-fg-subtle">{t("threadPlugins.allThreads")}</span>}
            </div>
          );
        })}
        <p className="pt-1 text-sm text-fg-muted">{t("threadPlugins.nextTurn")}</p>
      </div>
    );
  })();

  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={t("threadPlugins.title")} size="sm">
      {body}
    </Dialog>
  );
}
