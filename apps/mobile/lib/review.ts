// F-CN-09: what the Review tab says about a `review.current` reply. Kept
// out of the screen so it runs in tests without React Native.
import type { PhoneReview, PhoneReviewItem } from "@tandem/protocol";
import { t, type StringKey } from "./strings";

/** A diff line's kind: headers and hunks are muted, + added, − removed. */
export function lineKind(line: string): "meta" | "add" | "del" | "context" {
  if (
    line.startsWith("@@") ||
    line.startsWith("+++") ||
    line.startsWith("---") ||
    line.startsWith("diff --git")
  )
    return "meta";
  if (line.startsWith("+")) return "add";
  if (line.startsWith("-")) return "del";
  return "context";
}

export function statText(item: PhoneReviewItem): string {
  return item.files === 1
    ? t("review.statOne", { adds: item.additions, dels: item.deletions })
    : t("review.stat", { n: item.files, adds: item.additions, dels: item.deletions });
}

export function checkText(item: PhoneReviewItem): string | null {
  if (!item.check) return null;
  if (item.check.state === "running") return t("review.checkRunning");
  if (item.check.state === "passed") return t("review.checkPassed");
  return t("review.checkFailed", { code: item.check.exit ?? "—" });
}

export function ruleText(rule: string): string {
  return t(`review.rule.${rule}` as StringKey);
}

/** What the tab shows: nothing waiting, or the item with its notes. */
export function reviewLines(
  r: PhoneReview,
): { empty: true } | { empty: false; withheld: string[]; omitted: string | null } {
  if (!r.item) return { empty: true };
  return {
    empty: false,
    withheld: r.files.filter((f) => f.withheld).map((f) => t("review.withheld", { path: f.path })),
    omitted:
      r.omitted === 0
        ? null
        : r.omitted === 1
          ? t("review.omittedOne")
          : t("review.omitted", { n: r.omitted }),
  };
}
