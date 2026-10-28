import { render, screen } from "@testing-library/react";
import { EditorView } from "@codemirror/view";
import { describe, expect, it, vi } from "vitest";
import { CodeEditor, languageFor } from "./CodeEditor";

function viewOf(): EditorView {
  const host = screen.getByTestId("code-editor").querySelector(".cm-editor");
  const view = host ? EditorView.findFromDOM(host as HTMLElement) : null;
  if (!view) throw new Error("no editor view");
  return view;
}

describe("CodeEditor (F-CD-19)", () => {
  it("reports edits and saves on Mod+S", () => {
    const onChange = vi.fn();
    const onSave = vi.fn();
    render(<CodeEditor docKey="a" path="src/main.rs" label="Editor" value={"fn main() {}\n"} onChange={onChange} onSave={onSave} />);
    const view = viewOf();
    view.dispatch({ changes: { from: 0, insert: "// hi\n" } });
    expect(onChange).toHaveBeenLastCalledWith("// hi\nfn main() {}\n");
    const content = screen.getByLabelText("Editor");
    content.dispatchEvent(new KeyboardEvent("keydown", { key: "s", ctrlKey: true, metaKey: navigator.platform.startsWith("Mac"), bubbles: true, cancelable: true }));
    expect(onSave).toHaveBeenCalledTimes(1);
  });

  it("read-only refuses edits; a new document replaces the text", () => {
    const onChange = vi.fn();
    const { rerender } = render(<CodeEditor docKey="h" path="/etc/hosts" label="Editor" readOnly value={"127.0.0.1 localhost\n"} onChange={onChange} />);
    const view = viewOf();
    expect(view.state.readOnly).toBe(true);
    expect(screen.getByLabelText("Editor").getAttribute("contenteditable")).toBe("false");
    rerender(<CodeEditor docKey="b" path="b.txt" label="Editor" value={"bee\n"} onChange={onChange} />);
    expect(viewOf().state.doc.toString()).toBe("bee\n");
    expect(viewOf().state.readOnly).toBe(false);
    // The same file reloaded from disk: the text changes in place.
    rerender(<CodeEditor docKey="b" path="b.txt" label="Editor" value={"bee 2\n"} onChange={onChange} />);
    expect(viewOf().state.doc.toString()).toBe("bee 2\n");
  });

  it("goes to a line and column", () => {
    render(<CodeEditor docKey="g" path="a.ts" label="Editor" value={"one\ntwo\nthree\nfour\n"} goto={{ line: 3, col: 2, nonce: 1 }} />);
    const view = viewOf();
    const head = view.state.selection.main.head;
    expect(view.state.doc.lineAt(head).number).toBe(3);
    expect(head - view.state.doc.line(3).from).toBe(1);
  });

  it("picks the syntax by file name", () => {
    expect(languageFor("src/main.rs")?.name).toBe("Rust");
    expect(languageFor("web/app.tsx")?.name).toBe("TSX");
    expect(languageFor("Dockerfile")?.name).toBe("Dockerfile");
    expect(languageFor("notes.unknownext")).toBeNull();
  });
});

describe("CodeEditor code intelligence (F-CD-23)", () => {
  const press = (key: string, shiftKey = false) =>
    screen
      .getByLabelText("Editor")
      .dispatchEvent(new KeyboardEvent("keydown", { key, shiftKey, bubbles: true, cancelable: true }));

  it("F-CD-23.1 F12 asks for the definition at the cursor; Shift+F12 for references", () => {
    const definition = vi.fn();
    const references = vi.fn();
    render(
      <CodeEditor
        docKey="i"
        path="main.py"
        label="Editor"
        value={"import util\nutil.helper()\n"}
        goto={{ line: 2, col: 7, nonce: 1 }}
        intel={{ definition, references }}
      />,
    );
    press("F12");
    expect(definition).toHaveBeenCalledWith({ line: 1, character: 6 });
    press("F12", true);
    expect(references).toHaveBeenCalledWith({ line: 1, character: 6 });
  });

  it("F-CD-23.3 diagnostics underline their ranges and follow new ones", () => {
    const value = "x = 1\nERROR here\n";
    const diag = { line: 1, character: 0, endLine: 1, endCharacter: 5, severity: "error" as const, message: "fake: error here" };
    const { rerender } = render(<CodeEditor docKey="d" path="a.py" label="Editor" value={value} intel={{ diagnostics: [diag] }} />);
    const marks = () => Array.from(screen.getByTestId("code-editor").querySelectorAll(".cm-diag-error"));
    expect(marks().map((m) => m.textContent)).toEqual(["ERROR"]);
    rerender(<CodeEditor docKey="d" path="a.py" label="Editor" value={value} intel={{ diagnostics: [{ ...diag, severity: "warning" }] }} />);
    expect(marks()).toEqual([]);
    expect(screen.getByTestId("code-editor").querySelectorAll(".cm-diag-warning")).toHaveLength(1);
    rerender(<CodeEditor docKey="d" path="a.py" label="Editor" value={value} intel={{ diagnostics: [] }} />);
    expect(screen.getByTestId("code-editor").querySelectorAll(".cm-diag-warning")).toHaveLength(0);
  });

  it("F-CD-23.4 without intel, F12 does nothing special", () => {
    render(<CodeEditor docKey="n" path="a.rs" label="Editor" value={"fn a() {}\n"} />);
    expect(() => press("F12")).not.toThrow();
  });
});
