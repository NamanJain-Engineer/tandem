import { useCallback, useEffect, useState } from "react";
import type { UsageReport } from "@tandem/protocol";
import { Button, Dialog, EmptyState, Heatmap, Skeleton, Switch, tokenCount, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { drawShareCard, shareCard } from "./shareCard";

/** UI-37 / F-AC-07: tokens from the engines' own local logs, on this machine. */
export function UsageDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={t("usage.page.title")} size="lg">
      {open && <UsageBody />}
    </Dialog>
  );
}

function UsageBody() {
  const [report, setReport] = useState<UsageReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [withWorkspaces, setWithWorkspaces] = useState(false);
  const { show } = useToast();

  const load = useCallback(() => {
    setError(null);
    setReport(null);
    invoke<UsageReport>("usage_report")
      .then(setReport)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  }, []);
  useEffect(load, [load]);

  if (error) {
    return (
      <div className="flex flex-col items-start gap-3 p-1" data-testid="usage-error">
        <p className="text-base text-danger">{t("usage.page.error")}</p>
        <Button onClick={load}>{t("usage.page.retry")}</Button>
      </div>
    );
  }
  if (!report) {
    return (
      <div className="flex flex-col gap-3" data-testid="usage-loading" aria-busy="true">
        <Skeleton className="h-24 w-full" />
        <Skeleton className="h-40 w-full" />
      </div>
    );
  }
  const unsupported =
    report.unsupported.length > 0 ? (
      <p className="text-sm text-fg-subtle" data-testid="usage-unsupported">
        {t("usage.page.unsupported", { engines: report.unsupported.join(", ") })}
      </p>
    ) : null;
  if (report.total === 0) {
    return (
      <div className="flex flex-col gap-3" data-testid="usage-empty">
        <EmptyState title={t("usage.page.empty")} body={t("usage.page.source")} />
        {unsupported}
      </div>
    );
  }

  const save = async () => {
    try {
      const png = drawShareCard(shareCard(report, withWorkspaces), report);
      const path = await invoke<string | null>("usage_share_save", { pngBase64: png });
      if (path) show({ title: t("usage.card.saved", { path }) });
    } catch {
      show({ title: t("usage.card.failed"), tone: "error" });
    }
  };

  return (
    <div className="flex flex-col gap-5" data-testid="usage-page">
      <div className="flex items-baseline justify-between gap-4">
        <p className="text-2xl font-semibold tabular-nums" data-testid="usage-total">
          {t("usage.page.total", { tokens: tokenCount(report.total) })}
        </p>
        <p className="text-sm text-fg-subtle">{t("usage.range", { since: report.since, until: report.until })}</p>
      </div>
      <Heatmap
        label={t("usage.page.heatmap")}
        days={report.days.map((d) => ({ date: d.date, value: d.tokens }))}
        describe={(d) => t("usage.page.day", { date: d.date, tokens: tokenCount(d.value) })}
      />
      <div className="grid grid-cols-2 gap-6">
        <Table
          title={t("usage.page.engines")}
          testId="usage-engines"
          rows={report.engines.map((e) => [e.label, t("usage.page.sessions", { n: e.sessions }), tokenCount(e.tokens)])}
        />
        <Table
          title={t("usage.page.accounts")}
          testId="usage-accounts"
          rows={report.accounts.map((a) => [a.label, a.engineId, tokenCount(a.tokens)])}
        />
      </div>
      <Table
        title={t("usage.page.workspaces")}
        testId="usage-workspaces"
        rows={report.workspaces.map((w) => [w.name, w.path, tokenCount(w.tokens)])}
      />
      <div className="flex flex-col gap-1">
        <p className="text-sm text-fg-subtle">{t("usage.page.source")}</p>
        {report.skippedLines > 0 && (
          <p className="text-sm text-fg-subtle" data-testid="usage-skipped">
            {t("usage.page.skipped", { n: report.skippedLines })}
          </p>
        )}
        {unsupported}
      </div>
      <div className="flex items-center justify-between gap-4 border-t border-divider pt-4">
        <Switch label={t("usage.card.includeWorkspaces")} checked={withWorkspaces} onCheckedChange={setWithWorkspaces} />
        <Button onClick={() => void save()} data-testid="usage-share">
          {t("usage.card.save")}
        </Button>
      </div>
    </div>
  );
}

function Table({ title, rows, testId }: { title: string; rows: string[][]; testId: string }) {
  return (
    <section className="flex flex-col gap-1.5">
      <h3 className="text-2xs font-semibold uppercase tracking-caps text-fg-subtle">{title}</h3>
      <table className="w-full text-left text-sm" data-testid={testId}>
        <tbody>
          {rows.map((r) => (
            <tr key={r.join("|")} className="border-t border-divider first:border-t-0">
              <td className="py-1.5 pr-3 text-fg">{r[0]}</td>
              <td className="truncate py-1.5 pr-3 text-fg-muted">{r[1]}</td>
              <td className="py-1.5 text-right tabular-nums text-fg">{r[2]}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}
