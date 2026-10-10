import { useEffect, useSyncExternalStore } from "react";
import { Download, FlaskConical, RotateCcw } from "lucide-react";
import { Badge, Button } from "@tandem/ui";
import { t } from "../i18n";
import { useEngines } from "../stores/engines";
import { useThreads } from "../stores/threads";
import { useUi } from "../stores/ui";
import type { DemoCore } from "./backend";

const LINK =
  "inline-flex h-7 items-center gap-1.5 rounded-sm px-2.5 text-sm font-medium focus-visible:shadow-focus focus-visible:outline-none";

/** The strip above the demo: what this is, where the real app is, the
 * message limit and Reset. Embedded in the home page, it leaves out the link
 * back to the site the visitor is already on. */
export function DemoBar({
  core,
  firstThread,
  embedded = false,
  onReset = () => window.location.reload(),
}: {
  core: DemoCore;
  firstThread: string;
  embedded?: boolean;
  onReset?: () => void;
}) {
  const limited = useSyncExternalStore(
    (fn) => core.subscribe(fn),
    () => core.limitReached(),
  );
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
        {limited && (
          <span data-testid="demo-limit">
            <Badge variant="waiting">{t("demo.limit")}</Badge>
          </span>
        )}
        <Button size="sm" variant="ghost" iconLeft={<RotateCcw size={14} />} onClick={onReset}>
          {t("demo.reset")}
        </Button>
        {!embedded && (
          <a href="/" className={`${LINK} text-fg-muted hover:bg-surface-3 hover:text-fg`}>
            {t("demo.back")}
          </a>
        )}
        <a href="/download/" target={embedded ? "_top" : undefined} className={`${LINK} bg-accent text-accent-fg hover:bg-accent-hover`}>
          <Download size={14} aria-hidden="true" />
          {t("demo.download")}
        </a>
      </span>
    </div>
  );
}
