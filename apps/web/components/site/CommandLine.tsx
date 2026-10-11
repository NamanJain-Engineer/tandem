import { Terminal } from "lucide-react";
import { Icon } from "./Icon";

/** One shell command in a terminal-like box. No copy button (it would need a
 * script): one click selects the whole command. A command longer than the box
 * wraps instead of scrolling: a scrolling box would have to take keyboard
 * focus (WCAG 2.1.1, axe's scrollable-region-focusable). */
export function CommandLine({ command, label = "Terminal" }: { command: string; label?: string }) {
  return (
    <div data-stage="" className="min-w-0 max-w-full overflow-hidden rounded-md border border-stage-line bg-stage text-left">
      <div className="flex items-center gap-2 border-b border-stage-line px-3 py-1.5 font-mono text-eyebrow text-stage-muted">
        <Icon icon={Terminal} size={14} />
        <span>{label}</span>
        <span className="ml-auto">Click to select</span>
      </div>
      <pre className="whitespace-pre-wrap px-4 py-3 font-mono text-[0.8125rem] leading-6 text-stage-fg wrap-anywhere">
        <span aria-hidden="true" className="select-none text-stage-accent">
          ${" "}
        </span>
        <code className="select-all">{command}</code>
      </pre>
    </div>
  );
}
