// The recall palette (F-EX-02, UI-141): commands the panes' shells ran,
// newest first, searchable. Enter inserts (nothing runs); Mod+Enter runs it
// in a shell at its prompt; Mod+Backspace deletes it from the history.
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { create } from "zustand";
import type { HistoryEntry, IpcError } from "@tandem/protocol";
import { Button, EmptyState, QuickPicker, SegmentedControl, Spinner, cx, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useSettings, useSetting } from "../../stores/settings";
import { age } from "../thread/groups";
import { currentTarget, insertText, targetWorkspace, type InsertTarget } from "../insert/target";

type RecallStore = { open: boolean; target: InsertTarget; show: () => void; close: () => void };

export const useRecall = create<RecallStore>((set) => ({
  open: false,
  target: { kind: "clipboard" },
  show: () => set({ open: true, target: currentTarget() }),
  close: () => set({ open: false }),
}));

/** "4:12", "14s", "120ms". */
export function duration(ms: number | null): string {
  if (ms === null) return "";
  if (ms < 1000) return `${ms}ms`;
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** Can Run type this into the target? (The core checks the prompt too.) */
export function canRun(entry: HistoryEntry, target: InsertTarget): boolean {
  return !entry.secret && target.kind === "pane" && !target.agent;
}

function Row({ e, now }: { e: HistoryEntry; now: number }) {
  const ok = e.exitCode === 0;
  return (
    <div className="flex min-w-0 flex-1 flex-col py-0.5" data-testid="recall-row" data-id={e.id} data-command={e.command} data-secret={e.secret || undefined}>
      <span className="truncate font-mono text-sm text-fg" title={e.command}>
        {e.command}
      </span>
      <span className="flex min-w-0 items-center gap-2 text-xs text-fg-muted">
        <span className="truncate">{e.cwd}</span>
        {e.exitCode !== null && (
          <span className={cx("shrink-0", ok ? "text-done-text" : "text-error-text")}>
            {ok ? t("recall.ok") : t("recall.failed", { code: e.exitCode })}
          </span>
        )}
        {e.durationMs !== null && <span className="shrink-0 tabular-nums">{duration(e.durationMs)}</span>}
        <span className="shrink-0">{age(e.lastRunAt, now)}</span>
        {e.paneName && <span className="shrink-0 truncate">{e.paneName}</span>}
        {e.runs > 1 && <span className="shrink-0 tabular-nums">{t("recall.runs", { n: e.runs })}</span>}
      </span>
    </div>
  );
}

export function RecallPicker() {
  const { open, target, close } = useRecall();
  const enabled = useSetting("command_history_enabled", true);
  const [query, setQuery] = useState("");
  const [scope, setScope] = useState<"here" | "all">("here");
  const [items, setItems] = useState<HistoryEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [value, setValue] = useState<string>("");
  const [now, setNow] = useState(() => Date.now());
  const { show } = useToast();
  const workspaceId = targetWorkspace(target);

  const search = useCallback(async () => {
    try {
      const rows = await invoke<HistoryEntry[]>("history_search", {
        query,
        workspaceId: scope === "here" ? workspaceId : null,
        limit: 100,
      });
      setItems(rows);
      setError(null);
      setNow(Date.now());
    } catch (e) {
      setError((e as IpcError).message ?? String(e));
    }
  }, [query, scope, workspaceId]);

  useEffect(() => {
    if (!open) return;
    const timer = setTimeout(() => void search(), 60);
    return () => clearTimeout(timer);
  }, [open, search]);

  useEffect(() => {
    if (open) {
      setQuery("");
      setItems(null);
      setScope(workspaceId ? "here" : "all");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const byId = (id: string) => items?.find((e) => String(e.id) === id);

  const select = (id: string, mod: boolean) => {
    const entry = byId(id);
    if (!entry) return;
    if (!mod) {
      close();
      void insertText(target, entry.command).then((how) => {
        if (how === "copied") show({ tone: "success", title: t("recall.copied") });
        if (how === "failed") show({ tone: "error", title: t("recall.copyFailed") });
      });
      return;
    }
    if (!canRun(entry, target) || target.kind !== "pane") {
      show({ tone: "info", title: entry.secret ? t("recall.secret") : t("recall.cantRun") });
      return;
    }
    close();
    void invoke("history_run", { id: entry.id, paneId: target.paneId }).catch((e: IpcError) =>
      show({ tone: "error", title: e.message }),
    );
  };

  const remove = (id: string) => {
    const entry = byId(id);
    if (!entry) return;
    void invoke("history_delete", { id: entry.id }).then(() => void search());
  };

  let state: ReactNode = undefined;
  if (!enabled) {
    state = (
      <EmptyState
        title={t("recall.off")}
        primary={
          <Button variant="primary" size="sm" onClick={() => void useSettings.getState().set("command_history_enabled", true)}>
            {t("recall.turnOn")}
          </Button>
        }
      />
    );
  } else if (error) {
    state = (
      <EmptyState
        title={t("recall.error")}
        body={error}
        primary={
          <Button size="sm" onClick={() => void search()}>
            {t("recall.retry")}
          </Button>
        }
      />
    );
  } else if (items === null) {
    state = (
      <div className="flex items-center gap-2 p-3 text-sm text-fg-muted" data-testid="recall-loading">
        <Spinner size={12} /> {t("recall.loading")}
      </div>
    );
  } else if (items.length === 0) {
    state = query.trim() ? (
      <p className="px-3 py-6 text-center text-base text-fg-muted">{t("recall.noMatch", { q: query.trim() })}</p>
    ) : (
      <EmptyState title={t("recall.empty")} body={t("recall.empty.body")} />
    );
  }

  const highlighted = byId(value) ?? items?.[0];
  const runnable = highlighted ? canRun(highlighted, target) : false;
  return (
    <QuickPicker
      open={open}
      onOpenChange={(o) => !o && close()}
      title={t("recall.title")}
      placeholder={t("recall.placeholder")}
      query={query}
      onQueryChange={setQuery}
      value={value}
      onValueChange={setValue}
      items={(items ?? []).map((e) => ({ id: String(e.id), label: e.command, content: <Row e={e} now={now} /> }))}
      onSelect={select}
      onDelete={remove}
      state={state}
      header={
        <SegmentedControl
          size="sm"
          label={t("recall.scope")}
          value={scope}
          onValueChange={(v) => v && setScope(v as "here" | "all")}
          items={[
            ...(workspaceId ? [{ id: "here", label: t("recall.here") }] : []),
            { id: "all", label: t("recall.everywhere") },
          ]}
        />
      }
      hints={[
        { keys: ["Enter"], label: target.kind === "clipboard" ? t("recall.hint.copy") : t("recall.hint.insert") },
        ...(runnable ? [{ keys: ["Mod", "Enter"], label: t("recall.hint.run") }] : []),
        { keys: ["Mod", "Backspace"], label: t("recall.hint.delete") },
      ]}
    />
  );
}
