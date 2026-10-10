import { useCallback, useEffect, useMemo, useState } from "react";
import { BookOpen, X } from "lucide-react";
import type { IpcError, Memory, MemoryFile, Teammate } from "@tandem/protocol";
import {
  Banner,
  Button,
  EmptyState,
  IconButton,
  ProgressBar,
  SegmentedControl,
  Skeleton,
  Tabs,
  Textarea,
  useToast,
} from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useTeammates } from "../../stores/teammates";
import { addNote, deleteNote, notesOf, parseNotes } from "./notes";

/** Characters, as the core counts them (Unicode scalar values). */
export function chars(s: string): number {
  return Array.from(s).length;
}

/** Whether text holds something the log redactor would hide (the core's
 * own check refuses the same on save). */
async function looksSecret(text: string): Promise<boolean> {
  if (!text) return false;
  const redacted = await invoke<string>("redact_text", { text }).catch(() => text);
  return redacted !== text;
}

type View = "list" | "raw";

/** UI-73: NOTES.md and ABOUT_YOU.md, as a list of notes or raw Markdown. */
export function MemoryTab({ teammate, suggestions }: { teammate: Teammate; suggestions?: React.ReactNode }) {
  const version = useTeammates((s) => s.version);
  const [memory, setMemory] = useState<Memory | null>(null);
  const [error, setError] = useState<IpcError | null>(null);
  const [file, setFile] = useState<MemoryFile>("notes");

  const load = useCallback(async () => {
    try {
      setMemory(await invoke<Memory>("memory_get", { teammateId: teammate.id }));
      setError(null);
    } catch (e) {
      setError(e as IpcError);
    }
  }, [teammate.id]);

  useEffect(() => {
    void load();
  }, [load, version]);

  if (error && !memory) {
    return (
      <div className="p-4">
        <Banner tone="error" action={{ label: t("agent.memory.retry"), onClick: () => void load() }}>
          {error.message}
        </Banner>
      </div>
    );
  }
  if (!memory) {
    return (
      <div className="flex flex-col gap-2 p-4" aria-busy="true" aria-label={t("agent.memory.loading")}>
        {[0, 1, 2].map((n) => (
          <Skeleton key={n} height={28} width={n === 2 ? "60%" : "100%"} />
        ))}
      </div>
    );
  }
  return (
    <div className="flex min-h-0 flex-col gap-3 p-4" data-testid="memory-tab">
      <Tabs
        variant="pill"
        label={t("agent.memory.files")}
        value={file}
        onValueChange={(v) => setFile(v as MemoryFile)}
        items={[
          { id: "notes", label: t("agent.memory.notes") },
          { id: "about", label: t("agent.memory.about") },
        ]}
        renderPanel={(id) => (
          <FileView
            key={`${teammate.id}:${id}`}
            teammate={teammate}
            file={id as MemoryFile}
            text={id === "notes" ? memory.notes : memory.about}
            cap={id === "notes" ? memory.cap : memory.aboutCap}
            onSaved={load}
            suggestions={id === "notes" ? suggestions : null}
          />
        )}
      />
      <p className="text-sm text-fg-muted">{t("agent.memory.footer")}</p>
    </div>
  );
}

function FileView({
  teammate,
  file,
  text,
  cap,
  onSaved,
  suggestions,
}: {
  teammate: Teammate;
  file: MemoryFile;
  text: string;
  cap: number;
  onSaved: () => Promise<void>;
  suggestions: React.ReactNode;
}) {
  const { show } = useToast();
  const [view, setView] = useState<View>("list");
  const [draft, setDraft] = useState("");
  // Raw editing: what was loaded, and what's typed.
  const [base, setBase] = useState(text);
  const [raw, setRaw] = useState(text);
  const [secret, setSecret] = useState(false);
  const dirty = raw !== base;
  const changedOutside = dirty && text !== base;
  const used = chars(text);
  const notes = useMemo(() => notesOf(parseNotes(text)), [text]);

  useEffect(() => {
    if (!dirty) {
      setBase(text);
      setRaw(text);
    }
    // Only follow the file while nothing's being edited.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [text]);

  useEffect(() => {
    let live = true;
    const id = setTimeout(() => {
      void looksSecret(view === "raw" ? raw : draft).then((s) => live && setSecret(s));
    }, 200);
    return () => {
      live = false;
      clearTimeout(id);
    };
  }, [raw, draft, view]);

  const write = async (markdown: string): Promise<boolean> => {
    try {
      await invoke("memory_write", { teammateId: teammate.id, file, markdown });
      await onSaved();
      return true;
    } catch (e) {
      show({ tone: "error", title: (e as IpcError).message });
      return false;
    }
  };

  const nextAdded = draft.trim() ? addNote(text, draft) : text;
  const addOver = chars(nextAdded) - cap;
  const rawOver = chars(raw) - cap;

  const add = async () => {
    if (!draft.trim() || addOver > 0 || secret) return;
    if (await write(nextAdded)) setDraft("");
  };

  return (
    <div className="flex flex-col gap-3 pt-3">
      <div className="flex flex-wrap items-center gap-3">
        <SegmentedControl
          size="sm"
          label={t("agent.memory.view")}
          value={view}
          onValueChange={(v) => setView(v as View)}
          items={[
            { id: "list", label: t("agent.memory.list") },
            { id: "raw", label: t("agent.memory.raw") },
          ]}
        />
        {file === "about" && (
          <span className="text-sm text-fg-muted">
            {t("agent.memory.aboutHint", { name: teammate.name })}
          </span>
        )}
        <div className="ml-auto flex w-56 flex-col gap-1">
          <ProgressBar
            value={cap ? used / cap : 0}
            label={t("agent.memory.budget")}
            valueText={t("agent.memory.used", { used, cap })}
          />
          <span className="text-right text-sm tabular-nums text-fg-muted" data-testid="memory-budget">
            {t("agent.memory.used", { used, cap })}
          </span>
        </div>
      </div>
      {used > cap && <Banner tone="warning">{t("agent.memory.overBudget", { n: used - cap })}</Banner>}
      {suggestions}
      {view === "list" ? (
        <>
          {notes.length === 0 ? (
            <EmptyState
              icon={<BookOpen size={20} />}
              title={t("agent.memory.emptyTitle")}
              body={t("agent.memory.empty", { name: teammate.name })}
            />
          ) : (
            <ul
              className="flex flex-col rounded-md border border-border bg-surface-2"
              data-testid="memory-notes"
            >
              {notes.map((n) => (
                <li
                  key={n.index}
                  tabIndex={0}
                  data-testid="memory-note"
                  className="group flex min-h-8 items-center gap-2 border-b border-divider px-3 py-1.5 text-base text-fg last:border-b-0 focus-visible:shadow-focus focus-visible:outline-none"
                  onKeyDown={(e) => {
                    if (e.key === "Delete") void write(deleteNote(text, n.index));
                  }}
                >
                  <span
                    aria-hidden="true"
                    className="size-1 shrink-0 rounded-full bg-current text-fg-subtle"
                  />
                  <span className="min-w-0 flex-1 break-words">{n.text}</span>
                  <IconButton
                    size="sm"
                    className="opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 focus-visible:opacity-100"
                    icon={<X size={14} />}
                    label={t("agent.memory.delete", { note: n.text.slice(0, 40) })}
                    onClick={() => void write(deleteNote(text, n.index))}
                  />
                </li>
              ))}
            </ul>
          )}
          <div className="flex flex-col gap-2">
            <Textarea
              label={t("agent.memory.addLabel")}
              hideLabel
              placeholder={t("agent.memory.addPlaceholder", { name: teammate.name })}
              value={draft}
              minRows={2}
              maxRows={6}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                  e.preventDefault();
                  void add();
                }
              }}
              error={
                secret
                  ? t("agent.memory.secret")
                  : addOver > 0
                    ? t("agent.memory.addOver", { n: addOver })
                    : undefined
              }
            />
            <Button
              size="sm"
              variant="secondary"
              className="self-end"
              disabled={!draft.trim() || addOver > 0 || secret}
              onClick={() => void add()}
              data-testid="memory-add"
            >
              {t("agent.memory.add")}
            </Button>
          </div>
        </>
      ) : (
        <div className="flex flex-col gap-2">
          {changedOutside && (
            <Banner
              tone="warning"
              action={{
                label: t("agent.memory.reload"),
                onClick: () => {
                  setBase(text);
                  setRaw(text);
                },
              }}
            >
              {t("agent.memory.changedOutside", { file: file === "notes" ? "NOTES.md" : "ABOUT_YOU.md" })}
            </Banner>
          )}
          <Textarea
            label={t("agent.memory.rawLabel")}
            hideLabel
            monospace
            minRows={10}
            maxRows={30}
            value={raw}
            data-testid="memory-raw"
            onChange={(e) => setRaw(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "s" && (e.metaKey || e.ctrlKey)) {
                e.preventDefault();
                if (dirty && rawOver <= 0 && !secret) void write(raw).then((ok) => ok && setBase(raw));
              }
            }}
            error={
              secret
                ? t("agent.memory.secret")
                : rawOver > 0
                  ? t("agent.memory.overBudget", { n: rawOver })
                  : undefined
            }
          />
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="ghost" disabled={!dirty} onClick={() => setRaw(base)}>
              {t("agent.memory.revert")}
            </Button>
            <Button
              size="sm"
              variant="primary"
              disabled={!dirty || rawOver > 0 || secret}
              onClick={() => void write(raw).then((ok) => ok && setBase(raw))}
              data-testid="memory-save"
            >
              {t("agent.memory.save")}
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
