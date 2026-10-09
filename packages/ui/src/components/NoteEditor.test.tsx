import { createRef } from "react";
import { act, render, screen } from "@testing-library/react";
import { EditorSelection } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { describe, expect, it, vi } from "vitest";
import { NoteEditor, type NoteEditorHandle } from "./NoteEditor";

function viewOf(): EditorView {
  const host = screen.getByTestId("note-editor").querySelector(".cm-editor");
  const view = host ? EditorView.findFromDOM(host as HTMLElement) : null;
  if (!view) throw new Error("no editor view");
  return view;
}

const mac = navigator.platform.startsWith("Mac");

/** Presses `key` with Mod (Cmd on macOS, Ctrl elsewhere) in the editor. */
function mod(key: string) {
  screen.getByLabelText("Note").dispatchEvent(new KeyboardEvent("keydown", { key, ctrlKey: !mac, metaKey: mac, bubbles: true, cancelable: true }));
}

function enter() {
  screen.getByLabelText("Note").dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
}

function setup(text: string) {
  const onChange = vi.fn();
  const ref = createRef<NoteEditorHandle>();
  render(<NoteEditor ref={ref} docKey="n1" label="Note" taskLabel="Done" value={text} onChange={onChange} />);
  const view = viewOf();
  const select = (from: number, to = from) => act(() => view.dispatch({ selection: EditorSelection.single(from, to) }));
  const text_ = () => view.state.doc.toString();
  const selected = () => view.state.sliceDoc(view.state.selection.main.from, view.state.selection.main.to);
  return { view, onChange, ref, select, text: text_, selected };
}

describe("NoteEditor (F-SH-12)", () => {
  it("F-SH-12.2 Mod+B wraps the selection in ** and again unwraps it", () => {
    const n = setup("hello world");
    n.select(6, 11);
    mod("b");
    expect(n.text()).toBe("hello **world**");
    expect(n.selected()).toBe("world");
    expect(n.onChange).toHaveBeenLastCalledWith("hello **world**");
    mod("b");
    expect(n.text()).toBe("hello world");
    // The marks selected along with the word unwrap too.
    n.select(0, 5);
    mod("b");
    n.select(0, 9);
    expect(n.selected()).toBe("**hello**");
    mod("b");
    expect(n.text()).toBe("hello world");
    // Nothing selected: the marks go in with the cursor between them.
    n.select(5);
    mod("b");
    expect(n.text()).toBe("hello**** world");
    expect(n.view.state.selection.main.head).toBe(7);
  });

  it("F-SH-12 Mod+I and Mod+U: italic inside bold stays bold, underline wraps in <u>", () => {
    const n = setup("make it **bold**");
    n.select(10, 14);
    mod("i");
    expect(n.text()).toBe("make it ***bold***");
    mod("i");
    expect(n.text()).toBe("make it **bold**");
    // Bold off on bold-italic text leaves it italic.
    mod("i");
    mod("b");
    expect(n.text()).toBe("make it *bold*");
    n.select(0, 4);
    mod("u");
    expect(n.text()).toBe("<u>make</u> it *bold*");
    mod("u");
    expect(n.text()).toBe("make it *bold*");
  });

  it("F-SH-12 Enter continues bullets, numbers and tasks; Enter on an empty item ends the list", () => {
    const n = setup("- milk");
    n.select(6);
    enter();
    expect(n.text()).toBe("- milk\n- ");
    enter();
    expect(n.text()).toBe("- milk\n");
    act(() => n.view.dispatch({ changes: { from: 0, to: n.view.state.doc.length, insert: "1. one" }, selection: { anchor: 6 } }));
    enter();
    expect(n.text()).toBe("1. one\n2. ");
    act(() => n.view.dispatch({ changes: { from: 0, to: n.view.state.doc.length, insert: "  - [x] done" }, selection: { anchor: 12 } }));
    enter();
    expect(n.text()).toBe("  - [x] done\n  - [ ] ");
    // Outside a list, Enter is a plain new line.
    act(() => n.view.dispatch({ changes: { from: 0, to: n.view.state.doc.length, insert: "plain" }, selection: { anchor: 5 } }));
    enter();
    expect(n.text()).toBe("plain\n");
  });

  it("F-SH-12 task boxes are checkboxes that tick on a click", () => {
    const n = setup("- [ ] call Ana\n- [x] buy milk");
    const boxes = screen.getAllByRole("checkbox", { name: "Done" }) as HTMLInputElement[];
    expect(boxes.map((b) => b.checked)).toEqual([false, true]);
    act(() => {
      boxes[0]?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true }));
    });
    expect(n.text()).toBe("- [x] call Ana\n- [x] buy milk");
    expect((screen.getAllByRole("checkbox", { name: "Done" }) as HTMLInputElement[]).map((b) => b.checked)).toEqual([true, true]);
  });

  it("F-SH-12 the toolbar's list and task buttons turn lines into items and tick tasks", () => {
    const n = setup("eggs\nbread");
    n.select(0, 10);
    act(() => n.ref.current?.format("bullet"));
    expect(n.text()).toBe("- eggs\n- bread");
    act(() => n.ref.current?.format("bullet"));
    expect(n.text()).toBe("eggs\nbread");
    act(() => n.ref.current?.format("task"));
    expect(n.text()).toBe("- [ ] eggs\n- [ ] bread");
    n.select(0);
    act(() => n.ref.current?.format("task"));
    expect(n.text()).toBe("- [x] eggs\n- [ ] bread");
    n.select(6);
    act(() => n.ref.current?.format("bold"));
    expect(n.text()).toBe("- [x] ****eggs\n- [ ] bread");
  });
});
