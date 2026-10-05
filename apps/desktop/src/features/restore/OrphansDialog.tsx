import { useEffect, useState } from "react";
import { SquareTerminal } from "lucide-react";
import type { OrphanInfo } from "@tandem/protocol";
import { Button, Dialog, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";

/** UI-132: processes a crashed run left behind (F-CD-03 AC8). The core
 * already applied the "always" / "never" setting; this asks otherwise. */
export function OrphansDialog() {
  const [orphans, setOrphans] = useState<OrphanInfo[]>([]);
  const [busy, setBusy] = useState(false);
  const { show } = useToast();

  useEffect(() => {
    void invoke<OrphanInfo[]>("orphans_list")
      .then((list) => setOrphans(list ?? []))
      .catch(() => undefined);
  }, []);

  const resolve = async (action: "stop" | "keep") => {
    setBusy(true);
    const n = orphans.reduce((sum, o) => sum + o.pids.length, 0);
    await invoke("orphans_resolve", { action }, { timeoutMs: 15_000 }).catch(() => undefined);
    setBusy(false);
    setOrphans([]);
    if (action === "stop") show({ title: t("orphans.stopped", { n }) });
  };

  const time = (ms: number) =>
    new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).format(ms);

  return (
    <Dialog
      open={orphans.length > 0}
      onOpenChange={(o) => !o && !busy && void resolve("keep")}
      size="sm"
      title={t("orphans.title", { n: orphans.length })}
      description={t("orphans.body")}
      footer={
        <>
          <Button variant="ghost" disabled={busy} onClick={() => void resolve("keep")}>
            {t("orphans.keep")}
          </Button>
          <Button
            variant="primary"
            loading={busy}
            data-testid="orphans-stop"
            onClick={() => void resolve("stop")}
          >
            {t("orphans.stop")}
          </Button>
        </>
      }
    >
      <ul data-testid="orphans" className="flex flex-col gap-1.5">
        {orphans.map((o) => (
          <li key={o.paneId} className="flex items-center gap-2 text-base">
            <SquareTerminal size={14} aria-hidden="true" className="shrink-0 text-fg-muted" />
            <span className="font-mono text-sm text-fg">{o.exe}</span>
            <span className="min-w-0 flex-1 truncate text-sm text-fg-subtle" title={o.cwd}>
              {o.cwd}
            </span>
            <span className="shrink-0 text-sm text-fg-subtle">
              {t("orphans.started", { time: time(o.startedAt) })}
            </span>
          </li>
        ))}
      </ul>
    </Dialog>
  );
}
