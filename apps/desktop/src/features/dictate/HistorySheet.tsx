import { useEffect, useRef, useState } from "react";
import type { HistoryCursor, IpcError, TakePage, TakeView } from "@tandem/protocol";
import { MoreHorizontal } from "lucide-react";
import {
  Button,
  Checkbox,
  ConfirmDialog,
  Dialog,
  EmptyState,
  IconButton,
  Input,
  Menu,
  Sheet,
  Spinner,
  Textarea,
  useToast,
} from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { copyText } from "../../ipc/clipboard";
import { useDictate } from "../../stores/dictate";
import { formatDuration } from "./format";

/** The share card (1080×810, totals only: no transcript text). */
export function drawShareCard(
  canvas: HTMLCanvasElement,
  s: { words: number; speakingMs: number; takes: number; wpm: number; savedMs: number },
): void {
  canvas.width = 1080;
  canvas.height = 810;
  const g = canvas.getContext("2d");
  if (!g) return;
  const css = getComputedStyle(document.documentElement);
  const v = (name: string, fallback: string) => css.getPropertyValue(name).trim() || fallback;
  g.fillStyle = v("--tn-bg", "#060a12");
  g.fillRect(0, 0, 1080, 810);
  g.fillStyle = v("--tn-accent", "#2dd4bf");
  g.fillRect(80, 80, 12, 650);
  g.fillStyle = v("--tn-fg", "#e2e8f0");
  g.font = "600 56px Inter, sans-serif";
  g.fillText(t("dictate.share.title"), 130, 160);
  const rows: Array<[string, string]> = [
    [t("dictate.share.words"), s.words.toLocaleString()],
    [t("dictate.share.time"), formatDuration(s.speakingMs)],
    [t("dictate.share.takes"), s.takes.toLocaleString()],
    [t("dictate.share.wpm"), String(s.wpm)],
    [t("dictate.share.saved"), formatDuration(s.savedMs)],
  ];
  rows.forEach(([label, value], i) => {
    const y = 290 + i * 95;
    g.fillStyle = v("--tn-fg-muted", "#94a3b8");
    g.font = "400 34px Inter, sans-serif";
    g.fillText(label, 130, y);
    g.fillStyle = v("--tn-fg", "#e2e8f0");
    g.font = "600 52px Inter, sans-serif";
    g.fillText(value, 620, y);
  });
  g.fillStyle = v("--tn-fg-subtle", "#64748b");
  g.font = "400 26px Inter, sans-serif";
  g.fillText(t("dictate.share.footer"), 130, 740);
}

function ShareCard({ onClose }: { onClose: () => void }) {
  const stats = useDictate((s) => s.stats);
  const canvas = useRef<HTMLCanvasElement>(null);
  const { show } = useToast();
  useEffect(() => {
    if (canvas.current && stats)
      drawShareCard(canvas.current, {
        words: Number(stats.words),
        speakingMs: Number(stats.speakingMs),
        takes: Number(stats.takes),
        wpm: stats.wpm,
        savedMs: Number(stats.savedMs),
      });
  }, [stats]);
  const png = () => canvas.current?.toDataURL("image/png") ?? "";
  const save = async () => {
    const path = await invoke<string | null>(
      "dictate_save_file",
      { suggestedName: "vibezeo-dictate.png", kind: "png" },
      { timeoutMs: 24 * 3_600_000 },
    );
    if (!path) return;
    await invoke("dictate_share_card_save", { path, pngBase64: png() }).then(
      () => show({ tone: "success", title: t("dictate.share.saved.toast") }),
      (e: IpcError) => show({ tone: "error", title: e.message }),
    );
  };
  const copy = async () => {
    try {
      const blob = await (await fetch(png())).blob();
      await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
      show({ tone: "success", title: t("dictate.share.copied") });
    } catch {
      show({ tone: "error", title: t("dictate.share.copyFailed") });
    }
  };
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("dictate.share.dialog")}
      size="lg"
      footer={
        <>
          <Button variant="secondary" onClick={() => void copy()}>
            {t("dictate.share.copy")}
          </Button>
          <Button variant="primary" onClick={() => void save()} data-testid="share-save">
            {t("dictate.share.save")}
          </Button>
        </>
      }
    >
      <canvas
        ref={canvas}
        className="w-full rounded-md border border-border"
        aria-label={t("dictate.share.dialog")}
        data-testid="share-card"
      />
    </Dialog>
  );
}

function CorrectDialog({ take, onClose }: { take: TakeView; onClose: () => void }) {
  const [text, setText] = useState(take.text);
  const [remember, setRemember] = useState(true);
  const { show } = useToast();
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("dictate.history.correct")}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t("dictionary.cancel")}
          </Button>
          <Button
            variant="primary"
            data-testid="correct-save"
            onClick={() =>
              void invoke("dictate_take_correct", { takeId: take.id, corrected: text, remember }).then(
                onClose,
                (e: IpcError) => show({ tone: "error", title: e.message }),
              )
            }
          >
            {t("dictionary.save")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3">
        <Textarea
          label={t("dictate.history.myCorrection")}
          value={text}
          rows={4}
          onChange={(e) => setText(e.target.value)}
        />
        <Checkbox label={t("dictate.history.remember")} checked={remember} onCheckedChange={setRemember} />
      </div>
    </Dialog>
  );
}

/** UI-65: every take, searchable, with the totals. Stored only on this device. */
export function HistorySheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  const stats = useDictate((s) => s.stats);
  const rev = useDictate((s) => s.historyRev);
  const status = useDictate((s) => s.status);
  const { show } = useToast();
  const [query, setQuery] = useState("");
  const [takes, setTakes] = useState<TakeView[] | null>(null);
  const [next, setNext] = useState<HistoryCursor | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirmClear, setConfirmClear] = useState(false);
  const [correcting, setCorrecting] = useState<TakeView | null>(null);
  const [sharing, setSharing] = useState(false);

  const load = (before: HistoryCursor | null) =>
    invoke<TakePage | null>("dictate_history", { query: query || null, before, limit: 50 }).then(
      (page) => {
        setError(null);
        const got = page?.takes ?? [];
        setTakes((cur) => (before ? [...(cur ?? []), ...got] : got));
        setNext(page?.next ?? null);
      },
      (e: IpcError) => setError(e.message),
    );
  useEffect(() => {
    if (!open) return;
    void useDictate.getState().loadStats();
    const id = setTimeout(() => void load(null), 150);
    return () => clearTimeout(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, query, rev]);

  const exportCsv = async () => {
    const path = await invoke<string | null>(
      "dictate_save_file",
      { suggestedName: "vibezeo-dictate-history.csv", kind: "csv" },
      { timeoutMs: 24 * 3_600_000 },
    );
    if (!path) return;
    await invoke("dictate_history_export", { path }).then(
      () => show({ tone: "success", title: t("dictate.history.exported") }),
      (e: IpcError) => show({ tone: "error", title: e.message }),
    );
  };

  return (
    <Sheet
      open={open}
      onOpenChange={(o) => !o && onClose()}
      title={t("dictate.history.title")}
      subtitle={t("dictate.history.local")}
      size="lg"
    >
      <div className="flex flex-col gap-4" data-testid="history-sheet">
        <div
          className="flex flex-wrap items-center gap-x-5 gap-y-1 text-sm text-fg-muted"
          data-testid="history-stats"
        >
          <span data-testid="history-words">
            {t("dictate.stats.words", { n: Number(stats?.words ?? 0) })}
          </span>
          <span>
            {t("dictate.history.speaking", { time: formatDuration(Number(stats?.speakingMs ?? 0)) })}
          </span>
          <span>{t("dictate.stats.takes", { n: Number(stats?.takes ?? 0) })}</span>
          <span>{t("dictate.stats.wpm", { n: stats?.wpm ?? 0 })}</span>
          <span title={t("dictate.history.savedTooltip")}>
            {t("dictate.history.saved", { time: formatDuration(Number(stats?.savedMs ?? 0)) })}
          </span>
          <span className="flex-1" />
          <Button size="sm" variant="ghost" onClick={() => setSharing(true)}>
            {t("dictate.history.share")}
          </Button>
        </div>
        <div className="flex items-center gap-2">
          <Input
            label={t("dictate.history.search")}
            hideLabel
            placeholder={t("dictate.history.search")}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            className="flex-1"
            data-testid="history-search"
          />
          <Button size="sm" variant="secondary" onClick={() => void exportCsv()} data-testid="history-export">
            {t("dictate.history.export")}
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setConfirmClear(true)} data-testid="history-clear">
            {t("dictate.history.clear")}
          </Button>
        </div>
        {error ? (
          <p role="alert" className="text-sm text-[var(--tn-danger)]">
            {error}
          </p>
        ) : !takes ? (
          <Spinner label={t("dictate.loading")} />
        ) : takes.length === 0 ? (
          <EmptyState
            title={query ? t("dictate.history.noMatch") : t("dictate.history.empty")}
            body={query ? undefined : t("dictate.history.emptyBody", { key: status?.hold.label ?? "" })}
          />
        ) : (
          <ul
            className="flex flex-col divide-y divide-divider rounded-md border border-border"
            data-testid="history-list"
          >
            {takes.map((take) => (
              <li key={take.id} className="flex flex-col gap-1 px-3 py-2" data-take={take.id}>
                <span className="text-xs text-fg-muted">
                  {t("dictate.history.meta", {
                    when: new Date(Number(take.createdAt)).toLocaleString(),
                    source:
                      take.engine === "cloud"
                        ? t("dictate.history.source.cloud")
                        : t("dictate.history.source.local", { model: take.model }),
                    words: take.wordCount,
                    time: formatDuration(Number(take.durationMs)),
                  })}
                </span>
                <div className="flex items-start gap-2">
                  <p className="flex-1 text-base text-fg" data-testid="take-text">
                    {take.text}
                  </p>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() =>
                      void copyText(take.text).then(() =>
                        show({ tone: "success", title: t("dictate.history.copied") }),
                      )
                    }
                  >
                    {t("dictate.history.copy")}
                  </Button>
                  <Button size="sm" variant="ghost" onClick={() => setCorrecting(take)}>
                    {t("dictate.history.correctAction")}
                  </Button>
                  <Menu
                    trigger={
                      <IconButton
                        size="sm"
                        icon={<MoreHorizontal size={14} />}
                        label={t("dictate.history.more")}
                      />
                    }
                    entries={[
                      {
                        id: "dict",
                        label: t("dictate.history.addToDictionary"),
                        onSelect: () =>
                          void invoke("dictionary_save", {
                            rule: {
                              id: "",
                              kind: "word",
                              replacement: take.text.trim().slice(0, 200),
                              phrases: [],
                              matchCase: false,
                              protect: true,
                              spacing: "exact",
                              enabled: true,
                              lang: "*",
                              appProfiles: [],
                              updatedAt: 0,
                              deletedAt: null,
                            },
                          }).then(
                            () => show({ tone: "success", title: t("dictate.history.added") }),
                            (e: IpcError) => show({ tone: "error", title: e.message }),
                          ),
                      },
                      {
                        id: "delete",
                        label: t("dictate.history.delete"),
                        danger: true,
                        onSelect: () =>
                          void invoke("dictate_take_delete", { takeId: take.id }).then(() => void load(null)),
                      },
                    ]}
                  />
                </div>
              </li>
            ))}
          </ul>
        )}
        {next && (
          <Button size="sm" variant="ghost" onClick={() => void load(next)}>
            {t("dictate.history.loadMore")}
          </Button>
        )}
      </div>
      <ConfirmDialog
        open={confirmClear}
        onOpenChange={setConfirmClear}
        title={t("dictate.history.clearTitle")}
        body={t("dictate.history.clearBody")}
        confirmLabel={t("dictate.history.clearConfirm")}
        danger
        onConfirm={() => void invoke("dictate_history_clear").then(() => void load(null))}
      />
      {correcting && <CorrectDialog take={correcting} onClose={() => setCorrecting(null)} />}
      {sharing && <ShareCard onClose={() => setSharing(false)} />}
    </Sheet>
  );
}
