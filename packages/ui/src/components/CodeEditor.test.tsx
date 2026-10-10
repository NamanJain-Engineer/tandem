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
