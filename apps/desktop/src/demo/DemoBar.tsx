import { useEffect } from "react";
import { Download, FlaskConical } from "lucide-react";
import { Badge } from "@tandem/ui";
import { t } from "../i18n";
import { useEngines } from "../stores/engines";
import { useThreads } from "../stores/threads";
import { useUi } from "../stores/ui";

const LINK =
  "inline-flex h-7 items-center gap-1.5 rounded-sm px-2.5 text-sm font-medium focus-visible:shadow-focus focus-visible:outline-none";

/** The strip above the demo: what this is, and where the real app is. */
export function DemoBar({ firstThread }: { firstThread: string }) {
  // The core announces detection at startup; the demo's engines are known.
  useEffect(() => {
    void useEngines.getState().refresh();
  }, []);
  // Open on the thread with a message ready to send.
  const loaded = useThreads((s) => s.loaded);
  useEffect(() => {
    if (!loaded) return;
    const threads = useThreads.getState();
    if (useUi.getState().mode === "thread" && threads.selectedId === null) threads.select(firstThread);
  }, [loaded, firstThread]);

  return (
    <div
      data-testid="demo-bar"
      className="flex h-10 shrink-0 items-center gap-3 border-b border-border bg-surface-2 px-3 text-sm text-fg"
    >
      <FlaskConical size={14} aria-hidden="true" className="shrink-0 text-accent" />
      <Badge variant="accent">{t("demo.label")}</Badge>
      <span className="hidden min-w-0 truncate text-fg-muted md:inline">{t("demo.note")}</span>
      <span className="min-w-0 truncate text-fg-muted md:hidden">{t("demo.noteShort")}</span>
      <span className="ml-auto flex shrink-0 items-center gap-1">
        <a href="/" className={`${LINK} text-fg-muted hover:bg-surface-3 hover:text-fg`}>
          {t("demo.back")}
        </a>
        <a href="/download/" className={`${LINK} bg-accent text-accent-fg hover:bg-accent-hover`}>
          <Download size={14} aria-hidden="true" />
          {t("demo.download")}
        </a>
      </span>
    </div>
  );
}
