// Find in terminal (F-CD-08, UI_SPEC §2.27): the search addon's options and
// the bar's "n of m" label. The addon searches scrollback plus the viewport
// and highlights every match (up to HIGHLIGHT_LIMIT).
import type { ISearchOptions } from "@xterm/addon-search";
import type { TerminalTheme } from "@tandem/ui";
import { t } from "../../i18n";

export const HIGHLIGHT_LIMIT = 1000;

export type FindFlags = { caseSensitive: boolean; regex: boolean };
/** `index` is -1 with no active match; `count` is capped at HIGHLIGHT_LIMIT. */
export type FindResult = { index: number; count: number };

/** Whether `query` can be searched with these flags (a bad regex can't). */
export function validQuery(query: string, flags: FindFlags): boolean {
  if (!flags.regex) return true;
  try {
    new RegExp(query);
    return true;
  } catch {
    return false;
  }
}

export function searchOptions(flags: FindFlags, theme: TerminalTheme, incremental = false): ISearchOptions {
  return {
    caseSensitive: flags.caseSensitive,
    regex: flags.regex,
    incremental,
    decorations: {
      matchBackground: theme.searchMatch,
      matchOverviewRuler: theme.searchMatch,
      activeMatchBackground: theme.searchActive,
      activeMatchBorder: theme.cursor,
      activeMatchColorOverviewRuler: theme.searchActive,
    },
  };
}

/** "3 of 12", "No results", or "–" before a search. */
export function countLabel(query: string, result: FindResult | null, valid: boolean): string {
  if (!query) return "";
  if (!valid) return t("find.invalid");
  if (!result || result.count === 0) return t("find.none");
  const of = result.count >= HIGHLIGHT_LIMIT ? `${HIGHLIGHT_LIMIT}+` : String(result.count);
  return result.index < 0 ? t("find.countOnly", { m: of }) : t("find.count", { n: result.index + 1, m: of });
}
