import type { PhoneReview } from "@tandem/remote/messages";
import { Badge, DiffView } from "@tandem/ui";
import { Card, Notice } from "../../components/Panel";

const RULE: Record<string, string> = {
  many_files: "20+ files",
  migrations: "Touches migrations",
  ci: "Touches CI",
  lockfile: "Changes a lockfile",
  env: "Touches .env files",
  auth: "Touches auth paths",
  public_api: "Touches the public API",
  deleted_tests: "Deletes tests",
};

const KNOWN_CHANGES = [
  "added",
  "modified",
  "deleted",
  "renamed",
  "type_changed",
  "untracked",
  "conflicted",
] as const;
type Change = (typeof KNOWN_CHANGES)[number];

/** F-CN-09: what the phone's Review tab shows for a `review.current` reply:
 * the item and its diff, read only, or that nothing waits. */
export function ReviewBody({ review }: { review: PhoneReview }) {
  const item = review.item;
  if (!item) return <Notice>Nothing to review</Notice>;
  const shown = review.files.filter((f) => !f.withheld);
  const withheld = review.files.filter((f) => f.withheld);
  return (
    <div className="flex flex-col gap-3" data-testid="remote-review">
      <Card>
        <div className="flex flex-col gap-1">
          <div className="flex items-center gap-2">
            <span className="flex-1 font-medium text-fg">{item.title}</span>
            {review.total > 0 && review.position > 0 && (
              <span className="text-xs text-fg-muted">
                {review.position} of {review.total}
              </span>
            )}
          </div>
          <span className="text-xs text-fg-muted">
            {[item.engine, item.workspace, item.branch].filter(Boolean).join(" · ")}
          </span>
          {item.changed && <Badge variant="waiting">Changed since you approved</Badge>}
          {item.closing && <p className="whitespace-pre-wrap text-sm text-fg">{item.closing}</p>}
          <span className="text-xs text-fg-muted">
            {item.files} {item.files === 1 ? "file" : "files"} · +{item.additions} −{item.deletions}
          </span>
          {item.risks.length > 0 && (
            <div className="flex flex-wrap gap-1">
              {item.risks.map((r) => (
                <Badge key={r.rule} variant="outline">
                  {RULE[r.rule] ?? r.rule}
                </Badge>
              ))}
            </div>
          )}
          {item.check && (
            <span className="text-xs text-fg-muted">
              {item.check.state === "running"
                ? "Check running…"
                : item.check.state === "passed"
                  ? "Check passed"
                  : `Check failed (exit ${item.check.exit ?? "—"})`}
            </span>
          )}
          <span className="text-xs text-fg-subtle">
            Approve or merge it on your computer, or ask Conductor.
          </span>
        </div>
      </Card>
      <DiffView
        files={shown.map((f) => ({
          path: f.path,
          change: (KNOWN_CHANGES as readonly string[]).includes(f.change) ? (f.change as Change) : "modified",
          additions: f.additions,
          deletions: f.deletions,
          binary: false,
          patch: f.patch,
          truncated: f.truncated,
        }))}
        labels={{ truncated: "The rest of this file's changes are on your computer." }}
      />
      {withheld.map((f) => (
        <Notice key={f.path}>{`${f.path}: not shown on the phone, since .env files hold secrets.`}</Notice>
      ))}
      {review.omitted > 0 && (
        <Notice>{`${review.omitted} more ${review.omitted === 1 ? "file is" : "files are"} only on your computer.`}</Notice>
      )}
    </div>
  );
}
