import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { Compartment, EditorState, RangeSetBuilder, type Extension } from "@codemirror/state";
import {
  Decoration,
  drawSelection,
  EditorView,
  keymap,
  placeholder as placeholderText,
  ViewPlugin,
  WidgetType,
  type DecorationSet,
  type ViewUpdate,
} from "@codemirror/view";
import { tags as t } from "@lezer/highlight";
import { cx } from "../cx";
import { languageFor } from "./CodeEditor";
import { continueList, NOTE_FORMATS, taskBoxes, tickAt, toggleBold, toggleItalic, toggleUnderline, type NoteFormat } from "./noteCommands";

const highlight = HighlightStyle.define([
  { tag: [t.heading], fontWeight: "600", color: "var(--color-fg)" },
  { tag: [t.strong], fontWeight: "600" },
  { tag: [t.emphasis], fontStyle: "italic" },
  { tag: [t.strikethrough], textDecoration: "line-through" },
  { tag: [t.link, t.url], color: "var(--color-accent)", textDecoration: "underline" },
  { tag: [t.monospace], fontFamily: "var(--font-mono)", color: "var(--color-fg-muted)" },
  { tag: [t.processingInstruction, t.meta], color: "var(--color-fg-subtle)" },
  { tag: [t.quote], color: "var(--color-fg-muted)", fontStyle: "italic" },
]);

const theme = EditorView.theme({
  "&": { height: "100%", fontSize: "var(--text-base)", backgroundColor: "var(--color-bg)", color: "var(--color-fg)" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: "var(--font-sans)", lineHeight: "1.6" },
  ".cm-content": { caretColor: "var(--color-accent)", padding: "calc(var(--spacing) * 3) calc(var(--spacing) * 4)" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--color-accent)" },
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
    backgroundColor: "var(--tn-selection)",
  },
  ".cm-placeholder": { color: "var(--color-fg-subtle)" },
  ".cm-note-task": { margin: "0 var(--spacing) 0 0", verticalAlign: "middle", accentColor: "var(--color-accent)", cursor: "pointer" },
});

/** A task box drawn as a checkbox; a click ticks it. */
class TaskBox extends WidgetType {
  constructor(
    readonly checked: boolean,
    readonly label: string,
  ) {
    super();
  }
  eq(other: TaskBox) {
    return other.checked === this.checked && other.label === this.label;
  }
  toDOM() {
    const box = document.createElement("input");
    box.type = "checkbox";
    box.checked = this.checked;
    box.className = "cm-note-task";
    box.tabIndex = -1;
    box.setAttribute("aria-label", this.label);
    return box;
  }
  ignoreEvent() {
    return false;
  }
}

function taskDecorations(view: EditorView, label: string): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  for (const { from, to } of view.visibleRanges) {
    const start = view.state.doc.lineAt(from).from;
    for (const box of taskBoxes(view.state.sliceDoc(start, to), start)) {
      builder.add(box.from, box.from + 3, Decoration.replace({ widget: new TaskBox(box.checked, label) }));
    }
  }
  return builder.finish();
}

function tasks(label: string): Extension {
  return [
    ViewPlugin.fromClass(
      class {
        decorations: DecorationSet;
        constructor(view: EditorView) {
          this.decorations = taskDecorations(view, label);
        }
        update(u: ViewUpdate) {
          if (u.docChanged || u.viewportChanged) this.decorations = taskDecorations(u.view, label);
        }
      },
      { decorations: (p) => p.decorations },
    ),
    EditorView.domEventHandlers({
      mousedown: (e, view) => {
        const target = e.target as HTMLElement | null;
        if (!target?.classList.contains("cm-note-task") || view.state.readOnly) return false;
        const tr = tickAt(view.state, view.posAtDOM(target));
        if (!tr) return false;
        e.preventDefault();
        view.dispatch(tr);
        return true;
      },
    }),
  ];
}

export type NoteEditorHandle = {
  focus: () => void;
  /** A toolbar button: bold, italic, underline, bullet list or task list. */
  format: (kind: NoteFormat) => void;
};

export type NoteEditorProps = {
  /** The note's text. A new `docKey` starts a fresh document (and undo
   * history); with the same key, a different value replaces the text. */
  value: string;
  docKey: string;
  /** The editing area's accessible name. */
  label: string;
  placeholder?: string;
  /** What a task box's checkbox is called for screen readers. */
  taskLabel: string;
  onChange?: (text: string) => void;
  className?: string;
};

/** A Markdown note editor (the scratchpad, F-SH-12): Mod+B, Mod+I and
 * Mod+U wrap the selection in `**`, `*` and `<u>`; Enter continues lists;
 * task boxes are checkboxes. */
export const NoteEditor = forwardRef<NoteEditorHandle, NoteEditorProps>(function NoteEditor(
  { value, docKey, label, placeholder, taskLabel, onChange, className },
  ref,
) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const handlers = useRef({ onChange });
  handlers.current = { onChange };
  const lang = useRef(new Compartment());
  const loadedKey = useRef<string | null>(null);

  const extensions = (): Extension[] => [
    history(),
    drawSelection(),
    EditorView.lineWrapping,
    keymap.of([
      { key: "Mod-b", preventDefault: true, run: toggleBold },
      { key: "Mod-i", preventDefault: true, run: toggleItalic },
      { key: "Mod-u", preventDefault: true, run: toggleUnderline },
      { key: "Enter", run: continueList },
      ...defaultKeymap,
      ...historyKeymap,
    ]),
    syntaxHighlighting(highlight),
    theme,
    tasks(taskLabel),
    placeholder ? placeholderText(placeholder) : [],
    lang.current.of([]),
    EditorView.contentAttributes.of({ "aria-label": label, "aria-multiline": "true" }),
    EditorView.updateListener.of((u) => {
      if (u.docChanged) handlers.current.onChange?.(u.state.doc.toString());
    }),
  ];

  useImperativeHandle(ref, () => ({
    focus: () => view.current?.focus(),
    format: (kind) => {
      const v = view.current;
      if (!v) return;
      NOTE_FORMATS[kind]({ state: v.state, dispatch: (tr) => v.dispatch(tr) });
      v.focus();
    },
  }));

  // Markdown highlighting, loaded once on first use.
  const loadLanguage = (v: EditorView) => {
    languageFor("note.md")
      ?.load()
      .then((support) => {
        if (view.current === v) v.dispatch({ effects: lang.current.reconfigure(support) });
      })
      .catch(() => undefined);
  };

  useEffect(() => {
    if (!host.current) return;
    const v = new EditorView({ parent: host.current, state: EditorState.create({ doc: value, extensions: extensions() }) });
    view.current = v;
    loadedKey.current = docKey;
    loadLanguage(v);
    return () => {
      v.destroy();
      view.current = null;
    };
    // The view lives as long as the component; props flow in below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const v = view.current;
    if (!v) return;
    if (loadedKey.current !== docKey) {
      loadedKey.current = docKey;
      v.setState(EditorState.create({ doc: value, extensions: extensions() }));
      loadLanguage(v);
    } else if (v.state.doc.toString() !== value) {
      v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: value } });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [docKey, value]);

  return <div ref={host} data-testid="note-editor" className={cx("h-full min-h-0 overflow-hidden", className)} />;
});
