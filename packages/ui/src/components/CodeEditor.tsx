import { useEffect, useRef } from "react";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { LanguageDescription, bracketMatching, HighlightStyle, indentOnInput, syntaxHighlighting } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { highlightSelectionMatches, search, searchKeymap } from "@codemirror/search";
import { Compartment, EditorState, type Extension } from "@codemirror/state";
import {
  drawSelection,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
} from "@codemirror/view";
import { tags as t } from "@lezer/highlight";
import { cx } from "../cx";

/** Syntax colours from the semantic tokens (no colours of its own). */
const highlight = HighlightStyle.define([
  { tag: [t.keyword, t.modifier, t.controlKeyword, t.operatorKeyword, t.definitionKeyword], color: "var(--color-accent)" },
  { tag: [t.string, t.special(t.string), t.regexp, t.character], color: "var(--color-success)" },
  { tag: [t.number, t.bool, t.null, t.atom, t.unit], color: "var(--color-warning)" },
  { tag: [t.comment, t.lineComment, t.blockComment, t.docComment], color: "var(--color-fg-subtle)", fontStyle: "italic" },
  { tag: [t.typeName, t.className, t.namespace, t.standard(t.typeName)], color: "var(--color-info)" },
  { tag: [t.function(t.variableName), t.function(t.propertyName), t.macroName], color: "var(--color-working-text)" },
  { tag: [t.tagName, t.attributeName, t.self, t.special(t.variableName)], color: "var(--color-danger)" },
  { tag: [t.heading], fontWeight: "600", color: "var(--color-fg)" },
  { tag: [t.link, t.url], textDecoration: "underline" },
  { tag: [t.emphasis], fontStyle: "italic" },
  { tag: [t.strong], fontWeight: "600" },
  { tag: [t.invalid], color: "var(--color-error)" },
]);

const theme = EditorView.theme({
  "&": { height: "100%", fontSize: "var(--text-sm)", backgroundColor: "var(--color-bg)", color: "var(--color-fg)" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "1.55" },
  ".cm-content": { caretColor: "var(--color-accent)" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--color-accent)" },
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
    backgroundColor: "var(--tn-selection)",
  },
  ".cm-gutters": { backgroundColor: "var(--color-surface-1)", color: "var(--color-fg-subtle)", borderRight: "1px solid var(--color-divider)" },
  ".cm-activeLine": { backgroundColor: "var(--color-surface-2)" },
  ".cm-activeLineGutter": { backgroundColor: "var(--color-surface-2)", color: "var(--color-fg-muted)" },
  ".cm-matchingBracket": { backgroundColor: "var(--color-accent-soft)", outline: "1px solid var(--color-border-strong)" },
  ".cm-searchMatch": { backgroundColor: "var(--color-waiting-soft)" },
  ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: "var(--color-accent-soft)" },
  ".cm-selectionMatch": { backgroundColor: "var(--color-surface-3)" },
  ".cm-panels": { backgroundColor: "var(--color-surface-2)", color: "var(--color-fg)" },
  ".cm-panels.cm-panels-bottom": { borderTop: "1px solid var(--color-divider)" },
  ".cm-panel input, .cm-panel button": { fontSize: "var(--text-sm)" },
  ".cm-tooltip": { backgroundColor: "var(--color-surface-3)", border: "1px solid var(--color-border)" },
});

/** The language for a file name, loaded on first use. */
export function languageFor(path: string): LanguageDescription | null {
  const name = path.slice(path.lastIndexOf("/") + 1);
  return LanguageDescription.matchFilename(languages, name);
}

export type CodeEditorProps = {
  /** The text. A new `docKey` starts a fresh document (and undo history);
   * with the same key, a different value (a reload) replaces the text. */
  value: string;
  docKey: string;
  /** Picks the syntax by file name. */
  path: string;
  /** The editing area's accessible name. */
  label: string;
  readOnly?: boolean;
  /** Line breaks other than `\n` to keep (CodeMirror splits on any). */
  onChange?: (text: string) => void;
  /** Mod+S. */
  onSave?: () => void;
  /** Puts the cursor on this line (1-based); a new `nonce` repeats it. */
  goto?: { line: number; col?: number; nonce: number } | null;
  className?: string;
};

/** CodeMirror 6 for the Files editor (F-CD-19): highlighting by file name,
 * undo, search (Mod+F), Mod+S to save, read-only mode and go-to-line. */
export function CodeEditor({ value, docKey, path, label, readOnly = false, onChange, onSave, goto, className }: CodeEditorProps) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const handlers = useRef({ onChange, onSave });
  handlers.current = { onChange, onSave };
  const lang = useRef(new Compartment());
  const ro = useRef(new Compartment());
  const aria = useRef(new Compartment());
  const loadedKey = useRef<string | null>(null);

  const extensions = (): Extension[] => [
    lineNumbers(),
    highlightActiveLineGutter(),
    history(),
    drawSelection(),
    indentOnInput(),
    bracketMatching(),
    highlightActiveLine(),
    highlightSelectionMatches(),
    search({ top: false }),
    keymap.of([
      {
        key: "Mod-s",
        preventDefault: true,
        run: () => {
          handlers.current.onSave?.();
          return true;
        },
      },
      ...defaultKeymap,
      ...historyKeymap,
      ...searchKeymap,
      indentWithTab,
    ]),
    syntaxHighlighting(highlight),
    theme,
    lang.current.of([]),
    ro.current.of([EditorState.readOnly.of(readOnly), EditorView.editable.of(!readOnly)]),
    aria.current.of(EditorView.contentAttributes.of({ "aria-label": label })),
    EditorView.updateListener.of((u) => {
      if (u.docChanged) handlers.current.onChange?.(u.state.doc.toString());
    }),
  ];

  // Create once; a new document replaces the state.
  useEffect(() => {
    if (!host.current) return;
    const v = new EditorView({ parent: host.current, state: EditorState.create({ doc: value, extensions: extensions() }) });
    view.current = v;
    loadedKey.current = docKey;
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
    } else if (v.state.doc.toString() !== value) {
      v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: value } });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [docKey, value]);

  useEffect(() => {
    const v = view.current;
    if (!v) return;
    v.dispatch({ effects: ro.current.reconfigure([EditorState.readOnly.of(readOnly), EditorView.editable.of(!readOnly)]) });
  }, [readOnly, docKey]);

  useEffect(() => {
    view.current?.dispatch({ effects: aria.current.reconfigure(EditorView.contentAttributes.of({ "aria-label": label })) });
  }, [label, docKey]);

  // Syntax: loaded per file type, ignored if another file opened meanwhile.
  useEffect(() => {
    const desc = languageFor(path);
    const v = view.current;
    if (!v) return;
    if (!desc) {
      v.dispatch({ effects: lang.current.reconfigure([]) });
      return;
    }
    let live = true;
    desc
      .load()
      .then((support) => {
        if (live && view.current) view.current.dispatch({ effects: lang.current.reconfigure(support) });
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [path, docKey]);

  useEffect(() => {
    const v = view.current;
    if (!v || !goto) return;
    const n = Math.min(Math.max(1, goto.line), v.state.doc.lines);
    const line = v.state.doc.line(n);
    const pos = line.from + Math.min(Math.max(0, (goto.col ?? 1) - 1), line.length);
    v.dispatch({ selection: { anchor: pos }, effects: EditorView.scrollIntoView(pos, { y: "center" }) });
    v.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [goto?.nonce, docKey]);

  return <div ref={host} data-testid="code-editor" data-readonly={readOnly || undefined} className={cx("h-full min-h-0 overflow-hidden", className)} />;
}
