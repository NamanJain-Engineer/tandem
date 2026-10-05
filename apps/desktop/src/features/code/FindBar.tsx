// UI_SPEC §2.27 search bar (F-CD-08): top-right inside the pane. Enter /
// Shift+Enter (and Mod+G / Mod+Shift+G) step through matches; Esc closes.
import { forwardRef, type KeyboardEvent } from "react";
import { CaseSensitive, ChevronDown, ChevronUp, Regex, X } from "lucide-react";
import { IconButton } from "@tandem/ui";
import { t } from "../../i18n";
import { countLabel, type FindFlags, type FindResult } from "./find";

export type FindBarProps = {
  query: string;
  flags: FindFlags;
  result: FindResult | null;
  valid: boolean;
  mac: boolean;
  onQuery: (query: string) => void;
  onFlags: (flags: FindFlags) => void;
  onStep: (dir: "next" | "prev") => void;
  onClose: () => void;
};

export const FindBar = forwardRef<HTMLInputElement, FindBarProps>(function FindBar(
  { query, flags, result, valid, mac, onQuery, onFlags, onStep, onClose },
  ref,
) {
  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    const mod = mac ? e.metaKey : e.ctrlKey;
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    } else if (e.key === "Enter") {
      e.preventDefault();
      onStep(e.shiftKey ? "prev" : "next");
    } else if (mod && e.code === "KeyG") {
      e.preventDefault();
      onStep(e.shiftKey ? "prev" : "next");
    } else if (mod && e.code === "KeyF") {
      e.preventDefault();
      e.currentTarget.select();
    }
  };
  const label = countLabel(query, result, valid);
  return (
    <div
      role="search"
      aria-label={t("find.label")}
      data-testid="find-bar"
      className="absolute right-3 top-2 z-20 flex items-center gap-0.5 rounded-md border border-border-strong bg-surface-3 p-1 shadow-2"
    >
      <input
        ref={ref}
        type="text"
        value={query}
        spellCheck={false}
        autoComplete="off"
        aria-label={t("find.placeholder")}
        aria-invalid={!valid || undefined}
        placeholder={t("find.placeholder")}
        data-testid="find-input"
        onChange={(e) => onQuery(e.target.value)}
        onKeyDown={onKeyDown}
        className="h-7 w-44 rounded-sm border border-border bg-surface-2 px-2 text-sm text-fg outline-none placeholder:text-fg-subtle focus:border-accent aria-invalid:border-danger"
      />
      <span role="status" data-testid="find-count" className="min-w-16 px-1.5 text-center text-xs tabular-nums text-fg-muted">
        {label}
      </span>
      <IconButton
        size="sm"
        icon={<CaseSensitive size={15} />}
        label={t("find.case")}
        pressed={flags.caseSensitive}
        onClick={() => onFlags({ ...flags, caseSensitive: !flags.caseSensitive })}
      />
      <IconButton
        size="sm"
        icon={<Regex size={15} />}
        label={t("find.regex")}
        pressed={flags.regex}
        onClick={() => onFlags({ ...flags, regex: !flags.regex })}
      />
      <IconButton size="sm" icon={<ChevronUp size={15} />} label={t("find.prev")} onClick={() => onStep("prev")} disabled={!query} />
      <IconButton size="sm" icon={<ChevronDown size={15} />} label={t("find.next")} onClick={() => onStep("next")} disabled={!query} />
      <IconButton size="sm" icon={<X size={15} />} label={t("find.close")} onClick={onClose} />
    </div>
  );
});
