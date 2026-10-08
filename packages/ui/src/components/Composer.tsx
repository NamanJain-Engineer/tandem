import {
  forwardRef,
  useEffect,
  useId,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  useState,
  type ClipboardEvent,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { ArrowUp, AtSign, File, Folder, ImageIcon, MessagesSquare, Mic, Paperclip, Square } from "lucide-react";
import { cx } from "../cx";
import { Chip } from "./Badge";
import { IconButton } from "./IconButton";
import { SegmentedControl } from "./SegmentedControl";
import { Select, type SelectOption } from "./Select";
import { Tooltip } from "./Tooltip";

export type ComposerAttachment = {
  id: string;
  label: string;
  kind: "file" | "folder" | "image";
  /** An error chip, e.g. "Too large (max 20 MB)". */
  error?: string;
};

export type MentionItem = { id: string; label: string; detail?: string; kind: "folder" | "file" | "thread" };

export type ComposerLabels = {
  message: string;
  send: string;
  stop: string;
  emptySend: string;
  attach: string;
  dictate: string;
  mode: string;
  plan: string;
  act: string;
  modeTooltip: string;
  engine: string;
  model: string;
  effort: string;
  access: string;
  queued: string;
  removeQueued: string;
  mentions: string;
  noMentions: string;
  dropToAttach: string;
  removeAttachment: (label: string) => string;
};

export const COMPOSER_LABELS: ComposerLabels = {
  message: "Message",
  send: "Send",
  stop: "Stop",
  emptySend: "Type a message first",
  attach: "Attach",
  dictate: "Dictate",
  mode: "Plan or Act",
  plan: "Plan",
  act: "Act",
  modeTooltip: "Plan: propose a plan, no file changes. Act: make changes.",
  engine: "Engine",
  model: "Model",
  effort: "Effort",
  access: "Access",
  queued: "Queued · sends when this turn finishes",
  removeQueued: "Remove queued message",
  mentions: "Files, folders and threads",
  noMentions: "No matches",
  dropToAttach: "Drop to attach",
  removeAttachment: (label) => `Remove ${label}`,
};

export type ComposerSelect = {
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  disabled?: boolean;
};

export type ComposerProps = {
  value: string;
  onChange: (value: string) => void;
  /** Enter (or Mod+Enter with `sendWithModEnter`). While running the app
   * queues the message. */
  onSend: () => void;
  onStop?: () => void;
  running?: boolean;
  placeholder: string;
  sendWithModEnter?: boolean;
  /** ↑ in an empty composer: the last message, to edit, or null. */
  onRecallLast?: () => string | null;
  attachments?: ComposerAttachment[];
  onRemoveAttachment?: (id: string) => void;
  queued?: string | null;
  onRemoveQueued?: () => void;
  mode?: "plan" | "act";
  onModeChange?: (mode: "plan" | "act") => void;
  engine?: ComposerSelect;
  model?: ComposerSelect;
  /** Hidden when the engine has no effort setting. */
  effort?: ComposerSelect;
  access?: ComposerSelect & { warning?: boolean };
  /** "12.4k", with "Tokens used in this thread · …" as its tooltip. */
  tokens?: { text: string; tooltip: string };
  onAttach?: () => void;
  /** A paste that may hold an image (an image type, or no text): the app
   * reads the system clipboard natively and ignores "no image". */
  onPasteImage?: () => void;
  /** Files are being dragged over the thread. */
  dropActive?: boolean;
  /** Shows the mic (Dictate, M8). */
  dictation?: boolean;
  onDictate?: () => void;
  /** `@` opens a picker over these; picking one becomes a chip. */
  mentions?: (query: string) => Promise<MentionItem[]> | MentionItem[];
  onMention?: (item: MentionItem) => void;
  /** Dims the composer with the reason inline. */
  disabledReason?: ReactNode;
  labels?: Partial<ComposerLabels>;
  autoFocus?: boolean;
  className?: string;
};

export type ComposerHandle = { focus: () => void };

const MIN_H = 88;
const MAX_H = 320;

const MENTION_ICON = { folder: Folder, file: File, thread: MessagesSquare };
const ATTACHMENT_ICON = { folder: Folder, file: File, image: ImageIcon };

/** The `@query` the caret is in, if any. */
export function mentionAt(text: string, caret: number): { start: number; query: string } | null {
  const m = /(?:^|\s)@([^\s@]*)$/.exec(text.slice(0, caret));
  if (!m) return null;
  const query = m[1] ?? "";
  return { start: caret - query.length - 1, query };
}

/** UI_SPEC §2.23 (UI-22): textarea with chips above and a toolbar below. */
export const Composer = forwardRef<ComposerHandle, ComposerProps>(function Composer(
  {
    value,
    onChange,
    onSend,
    onStop,
    running = false,
    placeholder,
    sendWithModEnter = false,
    onRecallLast,
    attachments = [],
    onRemoveAttachment,
    queued,
    onRemoveQueued,
    mode,
    onModeChange,
    engine,
    model,
    effort,
    access,
    tokens,
    onAttach,
    onPasteImage,
    dropActive = false,
    dictation = false,
    onDictate,
    mentions,
    onMention,
    disabledReason,
    labels: partial,
    autoFocus,
    className,
  },
  ref,
) {
  const labels = { ...COMPOSER_LABELS, ...partial };
  const area = useRef<HTMLTextAreaElement>(null);
  const listId = useId();
  const [picker, setPicker] = useState<{ start: number; query: string; items: MentionItem[]; active: number } | null>(null);
  const lookup = useRef(0);
  const disabled = disabledReason !== undefined && disabledReason !== null;
  const empty = !value.trim() && attachments.length === 0;

  useImperativeHandle(ref, () => ({ focus: () => area.current?.focus() }), []);

  useLayoutEffect(() => {
    const el = area.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(MAX_H, Math.max(MIN_H, el.scrollHeight))}px`;
  }, [value]);

  useEffect(() => {
    if (autoFocus) area.current?.focus();
  }, [autoFocus]);

  const refreshPicker = (text: string, caret: number) => {
    if (!mentions) return;
    const at = mentionAt(text, caret);
    if (!at) {
      setPicker(null);
      return;
    }
    const n = ++lookup.current;
    void Promise.resolve(mentions(at.query)).then((items) => {
      if (n === lookup.current) setPicker({ ...at, items, active: 0 });
    });
  };

  const pick = (item: MentionItem) => {
    if (!picker) return;
    const caret = area.current?.selectionStart ?? value.length;
    const next = value.slice(0, picker.start) + value.slice(caret);
    lookup.current++;
    setPicker(null);
    onChange(next);
    onMention?.(item);
    requestAnimationFrame(() => area.current?.setSelectionRange(picker.start, picker.start));
  };

  const send = () => {
    if (empty || disabled) return;
    onSend();
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    // IME: Enter that confirms a composition never sends (UI-22).
    if (e.nativeEvent.isComposing || e.keyCode === 229) return;
    if (picker && picker.items.length > 0) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const d = e.key === "ArrowDown" ? 1 : -1;
        setPicker({ ...picker, active: (picker.active + d + picker.items.length) % picker.items.length });
        return;
      }
      if (e.key === "Enter" || e.key === "Tab") {
        e.preventDefault();
        const item = picker.items[picker.active];
        if (item) pick(item);
        return;
      }
    }
    if (picker && e.key === "Escape") {
      e.preventDefault();
      lookup.current++;
      setPicker(null);
      return;
    }
    if (e.key === "Enter") {
      const mod = e.metaKey || e.ctrlKey;
      const sends = sendWithModEnter ? mod : !e.shiftKey;
      if (sends) {
        e.preventDefault();
        send();
      }
      return;
    }
    if (e.key === "Escape" && running && onStop) {
      e.preventDefault();
      onStop();
      return;
    }
    if (e.key === "ArrowUp" && value === "" && onRecallLast) {
      const last = onRecallLast();
      if (last !== null) {
        e.preventDefault();
        onChange(last);
      }
    }
  };

  const onPaste = (e: ClipboardEvent<HTMLTextAreaElement>) => {
    if (!onPasteImage) return;
    const data = e.clipboardData;
    const items = Array.from(data?.items ?? []);
    const types = Array.from(data?.types ?? []);
    // Webviews differ in how a copied image shows up: as a file item, as a
    // type, or not at all (a paste with no text). The app reads the system
    // clipboard itself, so any of these asks it.
    const image =
      items.some((i) => i.type.startsWith("image/")) || types.some((t) => t.startsWith("image/")) || !data?.getData("text/plain");
    if (image) {
      e.preventDefault();
      onPasteImage();
    }
  };

  const activeId = picker?.items[picker.active] ? `${listId}-${picker.active}` : undefined;

  return (
    <div
      data-testid="composer"
      data-running={running || undefined}
      className={cx(
        "@container relative flex flex-col rounded-lg border border-border bg-surface-2 shadow-1 focus-within:border-border-strong",
        disabled && "opacity-60",
        className,
      )}
    >
      {dropActive && (
        <div className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center rounded-lg border-2 border-dashed border-accent bg-accent-soft text-base font-medium text-accent">
          {labels.dropToAttach}
        </div>
      )}
      {(queued || attachments.length > 0) && (
        <div className="flex flex-wrap gap-1.5 px-3 pt-2">
          {queued && (
            <Chip
              label={labels.queued}
              onRemove={onRemoveQueued}
              removeLabel={() => labels.removeQueued}
              className="border-waiting/50"
            />
          )}
          {attachments.map((a) => {
            const Icon = ATTACHMENT_ICON[a.kind];
            return (
              <Chip
                key={a.id}
                label={a.error ? `${a.label} · ${a.error}` : a.label}
                icon={<Icon size={12} className={a.error ? "text-error" : undefined} />}
                onRemove={onRemoveAttachment ? () => onRemoveAttachment(a.id) : undefined}
                removeLabel={labels.removeAttachment}
                className={a.error ? "border-error/60" : undefined}
              />
            );
          })}
        </div>
      )}
      {picker && (
        <div
          id={listId}
          role="listbox"
          aria-label={labels.mentions}
          className="absolute inset-x-2 bottom-full z-[var(--z-popover)] mb-1 max-h-64 overflow-y-auto rounded-md border border-border bg-surface-3 p-1 shadow-2"
        >
          {picker.items.length === 0 ? (
            <p className="px-2 py-1 text-sm text-fg-subtle">{labels.noMentions}</p>
          ) : (
            picker.items.map((item, n) => {
              const Icon = MENTION_ICON[item.kind];
              return (
                <div
                  key={item.id}
                  id={`${listId}-${n}`}
                  role="option"
                  aria-selected={n === picker.active}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    pick(item);
                  }}
                  className={cx(
                    "flex h-7 cursor-default items-center gap-2 rounded-sm px-2 text-base text-fg",
                    n === picker.active && "bg-surface-4",
                  )}
                >
                  <Icon size={14} aria-hidden="true" className="shrink-0 text-fg-muted" />
                  <span className="min-w-0 truncate font-mono text-sm">{item.label}</span>
                  {item.detail && <span className="ml-auto shrink-0 truncate text-sm text-fg-subtle">{item.detail}</span>}
                </div>
              );
            })
          )}
        </div>
      )}
      <textarea
        ref={area}
        aria-label={labels.message}
        aria-controls={picker ? listId : undefined}
        aria-activedescendant={activeId}
        aria-autocomplete={mentions ? "list" : undefined}
        value={value}
        placeholder={placeholder}
        disabled={disabled}
        onChange={(e) => {
          onChange(e.target.value);
          refreshPicker(e.target.value, e.target.selectionStart);
        }}
        onKeyDown={onKeyDown}
        onPaste={onPaste}
        onBlur={() => setPicker(null)}
        style={{ minHeight: MIN_H, maxHeight: MAX_H }}
        className="w-full resize-none overflow-y-auto bg-transparent px-3 py-2.5 text-md text-fg placeholder:text-fg-subtle focus:outline-none disabled:cursor-not-allowed"
      />
      {disabled && <div className="px-3 pb-1 text-sm text-fg-muted">{disabledReason}</div>}
      <div className="flex min-h-10 flex-wrap items-center gap-1.5 border-t border-divider px-2 py-1.5">
        {mode && (
          <SegmentedControl
            label={labels.mode}
            size="sm"
            value={mode}
            onValueChange={(v) => onModeChange?.(v === "plan" ? "plan" : "act")}
            items={[
              { id: "plan", label: labels.plan, tooltip: labels.modeTooltip },
              { id: "act", label: labels.act, tooltip: labels.modeTooltip },
            ]}
          />
        )}
        {engine && (
          <Select size="sm" ariaLabel={labels.engine} className="min-w-0 max-w-40" value={engine.value} options={engine.options} onValueChange={engine.onChange} disabled={engine.disabled || disabled} />
        )}
        {model && (
          <Select size="sm" ariaLabel={labels.model} className="min-w-0 max-w-40" value={model.value} options={model.options} onValueChange={model.onChange} disabled={model.disabled || disabled} />
        )}
        {effort && (
          <Select size="sm" ariaLabel={labels.effort} className="hidden min-w-0 max-w-32 @xl:flex" value={effort.value} options={effort.options} onValueChange={effort.onChange} disabled={effort.disabled || disabled} />
        )}
        {access && (
          <Select
            size="sm"
            ariaLabel={labels.access}
            className={cx("min-w-0 max-w-40", access.warning && "[&_button]:border-warning [&_button]:text-waiting-text")}
            value={access.value}
            options={access.options}
            onValueChange={access.onChange}
            disabled={access.disabled || disabled}
          />
        )}
        <span className="ml-auto" />
        {tokens && (
          <Tooltip label={tokens.tooltip}>
            <span tabIndex={0} className="hidden rounded-xs px-1 text-sm tabular-nums text-fg-subtle focus-visible:shadow-focus focus-visible:outline-none @lg:inline">
              {tokens.text}
            </span>
          </Tooltip>
        )}
        {mentions && (
          <IconButton
            size="sm"
            icon={<AtSign size={16} />}
            label={labels.mentions}
            disabled={disabled}
            onClick={() => {
              const el = area.current;
              if (!el) return;
              const caret = el.selectionStart ?? value.length;
              const pad = caret > 0 && !/\s/.test(value[caret - 1] ?? "") ? " @" : "@";
              const next = value.slice(0, caret) + pad + value.slice(caret);
              onChange(next);
              el.focus();
              const at = caret + pad.length;
              requestAnimationFrame(() => el.setSelectionRange(at, at));
              refreshPicker(next, at);
            }}
          />
        )}
        {onAttach && <IconButton size="sm" icon={<Paperclip size={16} />} label={labels.attach} kbd={["Mod", "U"]} onClick={onAttach} disabled={disabled} />}
        {dictation && <IconButton size="sm" icon={<Mic size={16} />} label={labels.dictate} onClick={onDictate} disabled={disabled} />}
        {running && onStop ? (
          <IconButton
            size="sm"
            icon={<Square size={14} className="fill-current" />}
            label={labels.stop}
            kbd={["Esc"]}
            onClick={onStop}
            className="text-danger hover:text-danger"
            data-testid="composer-stop"
          />
        ) : null}
        {(!running || !onStop || !empty) && (
          <IconButton
            size="sm"
            variant="primary"
            icon={<ArrowUp size={16} />}
            label={empty ? labels.emptySend : labels.send}
            kbd={empty ? undefined : sendWithModEnter ? ["Mod", "Enter"] : ["Enter"]}
            onClick={send}
            disabled={empty || disabled}
            data-testid="composer-send"
          />
        )}
      </div>
    </div>
  );
});
