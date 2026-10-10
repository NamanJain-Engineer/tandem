// The sidebar's Scratchpad (F-SH-12, UI §3.5): local Markdown notes that
// save as you type. Wide panels show the list beside the open note; narrow
// ones show one or the other, with a way back to the list.
import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  ArrowLeft,
  Bold,
  Italic,
  List,
  ListChecks,
  NotebookPen,
  Plus,
  Trash2,
  Underline,
} from "lucide-react";
import type { ScratchNote } from "@tandem/protocol";
import {
  Banner,
  Button,
  ConfirmDialog,
  EmptyState,
  IconButton,
  NoteEditor,
  Spinner,
  cx,
  useToast,
  type NoteEditorHandle,
  type NoteFormat,
} from "@tandem/ui";
import { t } from "../../i18n";
import { useScratchpad } from "../../stores/scratchpad";
import { age } from "../thread/groups";

/** Wide enough for the list beside the note. */
export const WIDE_AT = 560;

/** Follows the panel's width. */
function useWidth(): [(el: HTMLDivElement | null) => void, number] {
  const [el, setEl] = useState<HTMLDivElement | null>(null);
  const [width, setWidth] = useState(0);
  useEffect(() => {
    if (!el) return;
    const measure = () => setWidth(el.clientWidth);
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [el]);
  return [setEl, width];
}

const titleOf = (note: ScratchNote) => note.title || t("scratchpad.untitled");

function NoteList({ onDelete }: { onDelete: (note: ScratchNote) => void }) {
  const notes = useScratchpad((s) => s.notes);
  const activeId = useScratchpad((s) => s.activeId);
  const now = Date.now();
  return (
    <ul
      aria-label={t("scratchpad.list")}
      className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-auto p-1.5"
      data-testid="scratchpad-list"
    >
      {notes.map((note) => (
        <li key={note.id} className="group flex items-center">
          <button
            type="button"
            aria-current={note.id === activeId ? "true" : undefined}
            data-testid="scratchpad-note"
            data-id={note.id}
            onClick={() => void useScratchpad.getState().open(note.id)}
            className={cx(
              "flex min-w-0 flex-1 items-baseline gap-2 rounded-sm px-2 py-1.5 text-left text-sm focus-visible:shadow-focus focus-visible:outline-none",
              note.id === activeId
                ? "bg-surface-3 text-fg"
                : "text-fg-muted hover:bg-surface-2 hover:text-fg",
            )}
          >
            <span className={cx("min-w-0 flex-1 truncate", !note.title && "italic text-fg-subtle")}>
              {titleOf(note)}
            </span>
            <span className="shrink-0 text-xs tabular-nums text-fg-subtle">{age(note.updatedAt, now)}</span>
          </button>
          <IconButton
            size="sm"
            icon={<Trash2 size={13} />}
            label={t("scratchpad.delete", { title: titleOf(note) })}
            className="opacity-0 focus-visible:opacity-100 group-hover:opacity-100"
            onClick={() => onDelete(note)}
          />
        </li>
      ))}
    </ul>
  );
}

const FORMATS: Array<{ kind: NoteFormat; icon: ReactNode; label: Parameters<typeof t>[0]; kbd?: string[] }> =
  [
    { kind: "bold", icon: <Bold size={14} />, label: "scratchpad.bold", kbd: ["Mod", "B"] },
    { kind: "italic", icon: <Italic size={14} />, label: "scratchpad.italic", kbd: ["Mod", "I"] },
    { kind: "underline", icon: <Underline size={14} />, label: "scratchpad.underline", kbd: ["Mod", "U"] },
    { kind: "bullet", icon: <List size={14} />, label: "scratchpad.bullet" },
    { kind: "task", icon: <ListChecks size={14} />, label: "scratchpad.task" },
  ];

function Editor({ onBack }: { onBack?: () => void }) {
  const activeId = useScratchpad((s) => s.activeId);
  const text = useScratchpad((s) => s.text);
  const opening = useScratchpad((s) => s.opening);
  const saveError = useScratchpad((s) => s.saveError);
  const focusNonce = useScratchpad((s) => s.focusNonce);
  const editor = useRef<NoteEditorHandle>(null);
  // A note just created starts with the cursor in it.
  useEffect(() => {
    if (focusNonce > 0) editor.current?.focus();
  }, [focusNonce]);
  if (!activeId) return null;
  return (
    <section
      aria-label={t("scratchpad.editorRegion")}
      className="flex min-h-0 min-w-0 flex-1 flex-col"
      data-testid="scratchpad-editor"
    >
      <div
        className="flex h-9 shrink-0 items-center gap-0.5 border-b border-divider px-1.5"
        role="toolbar"
        aria-label={t("scratchpad.toolbar")}
      >
        {onBack && (
          <IconButton
            size="sm"
            icon={<ArrowLeft size={14} />}
            label={t("scratchpad.back")}
            onClick={onBack}
            data-testid="scratchpad-back"
          />
        )}
        {FORMATS.map((f) => (
          <IconButton
            key={f.kind}
            size="sm"
            icon={f.icon}
            label={t(f.label)}
            kbd={f.kbd}
            onClick={() => editor.current?.format(f.kind)}
            data-testid={`scratchpad-${f.kind}`}
          />
        ))}
      </div>
      {saveError && (
        <Banner
          tone="error"
          action={{ label: t("scratchpad.retry"), onClick: () => void useScratchpad.getState().flush() }}
        >
          {t("scratchpad.saveFailed", { message: saveError })}
        </Banner>
      )}
      {opening ? (
        <div className="flex flex-1 items-center justify-center">
          <Spinner label={t("scratchpad.opening")} />
        </div>
      ) : (
        <NoteEditor
          ref={editor}
          docKey={activeId}
          value={text}
          label={t("scratchpad.editor")}
          placeholder={t("scratchpad.placeholder")}
          taskLabel={t("scratchpad.done")}
          onChange={(next) => useScratchpad.getState().edit(next)}
          className="flex-1"
        />
      )}
    </section>
  );
}

export function ScratchpadPanel() {
  const status = useScratchpad((s) => s.status);
  const error = useScratchpad((s) => s.error);
  const notes = useScratchpad((s) => s.notes);
  const activeId = useScratchpad((s) => s.activeId);
  const [ref, width] = useWidth();
  const wide = width >= WIDE_AT;
  const [deleting, setDeleting] = useState<ScratchNote | null>(null);
  const { show } = useToast();

  useEffect(() => {
    void useScratchpad.getState().load();
    // Anything typed goes to disk before the panel goes, or the window does.
    const flush = () => void useScratchpad.getState().flush();
    window.addEventListener("pagehide", flush);
    window.addEventListener("beforeunload", flush);
    return () => {
      window.removeEventListener("pagehide", flush);
      window.removeEventListener("beforeunload", flush);
      flush();
    };
  }, []);

  // Wide: the newest note opens by itself.
  useEffect(() => {
    if (wide && status === "ready" && !activeId && notes[0]) void useScratchpad.getState().open(notes[0].id);
  }, [wide, status, activeId, notes]);

  const create = () => void useScratchpad.getState().create();
  const newNote = (
    <Button
      size="sm"
      variant="secondary"
      iconLeft={<Plus size={14} />}
      onClick={create}
      data-testid="scratchpad-new-empty"
    >
      {t("scratchpad.new")}
    </Button>
  );

  let body;
  if (status === "error") {
    body = (
      <EmptyState
        title={t("scratchpad.loadFailed")}
        body={error}
        primary={
          <Button size="sm" variant="secondary" onClick={() => void useScratchpad.getState().load()}>
            {t("scratchpad.retry")}
          </Button>
        }
      />
    );
  } else if (status !== "ready") {
    body = (
      <div className="flex flex-1 items-center justify-center">
        <Spinner label={t("scratchpad.loading")} />
      </div>
    );
  } else if (notes.length === 0) {
    body = (
      <EmptyState
        icon={<NotebookPen size={18} />}
        title={t("scratchpad.empty.title")}
        body={t("scratchpad.empty.body")}
        primary={newNote}
      />
    );
  } else if (wide) {
    body = (
      <div className="flex min-h-0 flex-1">
        <div className="flex w-[220px] shrink-0 flex-col border-r border-divider">
          <NoteList onDelete={setDeleting} />
        </div>
        <Editor />
      </div>
    );
  } else {
    body = activeId ? (
      <Editor onBack={() => void useScratchpad.getState().close()} />
    ) : (
      <NoteList onDelete={setDeleting} />
    );
  }

  return (
    <div
      ref={ref}
      className="flex h-full min-h-0 flex-col"
      data-testid="scratchpad"
      data-layout={wide ? "wide" : "narrow"}
    >
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-divider pl-3 pr-1.5">
        <h2 className="flex-1 text-sm font-medium text-fg">{t("scratchpad.title")}</h2>
        <IconButton
          size="sm"
          icon={<Plus size={14} />}
          label={t("scratchpad.new")}
          onClick={create}
          disabled={status !== "ready"}
          data-testid="scratchpad-new"
        />
      </div>
      {body}
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(o) => !o && setDeleting(null)}
        danger
        title={t("scratchpad.deleteTitle", { title: deleting ? titleOf(deleting) : "" })}
        body={t("scratchpad.deleteBody")}
        confirmLabel={t("scratchpad.deleteConfirm")}
        cancelLabel={t("common.cancel")}
        onConfirm={() => {
          const note = deleting;
          setDeleting(null);
          if (note)
            void useScratchpad
              .getState()
              .remove(note.id)
              .then((err) => err && show({ tone: "error", title: err.message }));
        }}
      />
    </div>
  );
}
