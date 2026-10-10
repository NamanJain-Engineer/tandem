import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { IpcError, ScratchNote } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { SAVE_MS, useScratchpad } from "../../stores/scratchpad";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { ScratchpadPanel } from "./ScratchpadPanel";

/** A fake `$DATA/scratchpad` behind the commands. */
function disk(initial: Record<string, string> = {}) {
  const notes = new Map(Object.entries(initial));
  let clock = 1_000;
  let next = 0;
  let failWrites = false;
  const meta = (id: string): ScratchNote => ({
    id,
    title:
      (notes.get(id) ?? "")
        .split("\n")
        .find((l) => l.trim())
        ?.replace(/^#+\s*/, "") ?? "",
    updatedAt: ++clock,
    bytes: notes.get(id)?.length ?? 0,
  });
  const err = (code: string, message: string): IpcError => ({
    code: code as IpcError["code"],
    message,
    stage: "scratchpad",
    hint: null,
    retryable: false,
  });
  const calls = fakeBackend({
    scratchpad_list: () => [...notes.keys()].map(meta).reverse(),
    scratchpad_read: (a) => {
      const text = notes.get(String(a?.id));
      if (text === undefined) throw err("not_found", "That note doesn't exist");
      return text;
    },
    scratchpad_write: (a) => {
      if (failWrites) throw err("internal", "disk full");
      const id = a?.id ? String(a.id) : `note_${++next}`;
      if (a?.id && !notes.has(id)) throw err("not_found", "That note doesn't exist");
      notes.set(id, String(a?.markdown));
      return meta(id);
    },
    scratchpad_delete: (a) => {
      if (!notes.delete(String(a?.id))) throw err("not_found", "That note doesn't exist");
      return null;
    },
  });
  return { notes, calls, failing: (on: boolean) => (failWrites = on) };
}

const writes = (calls: Call[]) => calls.filter((c) => c.cmd === "scratchpad_write");

function mount() {
  return render(
    <UiProvider>
      <ScratchpadPanel />
    </UiProvider>,
  );
}

beforeEach(() => resetStores());
afterEach(() => vi.useRealTimers());

describe("Scratchpad (F-SH-12)", () => {
  it("F-SH-12.1 while someone types without pausing, a save still goes out within 300 ms of the first unsaved keystroke", async () => {
    vi.useFakeTimers();
    const d = disk({ note_a: "" });
    useScratchpad.setState({ activeId: "note_a", text: "", status: "ready" });
    const edit = (text: string) => useScratchpad.getState().edit(text);
    edit("h");
    await act(async () => vi.advanceTimersByTime(100));
    edit("he");
    await act(async () => vi.advanceTimersByTime(100));
    edit("hel");
    await act(async () => vi.advanceTimersByTime(90));
    // A debounce would still be waiting; the clock started at the first key.
    expect(writes(d.calls)).toHaveLength(0);
    edit("hell");
    await act(async () => vi.advanceTimersByTime(10));
    expect(writes(d.calls).map((c) => c.args?.markdown)).toEqual(["hell"]);
    expect(d.notes.get("note_a")).toBe("hell");
    // The next keystroke starts the next clock.
    edit("hello");
    await act(async () => vi.advanceTimersByTime(SAVE_MS - 1));
    expect(writes(d.calls)).toHaveLength(1);
    await act(async () => vi.advanceTimersByTime(1));
    expect(d.notes.get("note_a")).toBe("hello");
    // Nothing new: nothing written.
    await act(async () => vi.advanceTimersByTime(5 * SAVE_MS));
    expect(writes(d.calls)).toHaveLength(2);
  });

  it("F-SH-12 an empty scratchpad says so; New note opens one, and its first line names it in the list", async () => {
    disk();
    mount();
    expect(await screen.findByText("No notes yet")).toBeTruthy();
    fireEvent.click(screen.getByTestId("scratchpad-new-empty"));
    // jsdom measures the panel as narrow: the new note opens on its own.
    await screen.findByTestId("note-editor");
    fireEvent.click(screen.getByTestId("scratchpad-back"));
    await waitFor(() =>
      expect(screen.getAllByTestId("scratchpad-note").map((n) => n.textContent)).toEqual([
        expect.stringMatching(/^Untitled note/),
      ]),
    );
    fireEvent.click(screen.getAllByTestId("scratchpad-note")[0] as HTMLElement);
    await screen.findByTestId("note-editor");
    act(() => useScratchpad.getState().edit("# Groceries\n- [ ] milk"));
    fireEvent.click(screen.getByTestId("scratchpad-back"));
    // Going back writes what was typed.
    await waitFor(() =>
      expect(screen.getAllByTestId("scratchpad-note")[0]?.textContent).toMatch(/^Groceries/),
    );
  });

  it("F-SH-12 Delete asks first, and a save still waiting for the note isn't sent", async () => {
    const d = disk({ note_a: "keep", note_b: "# Bin me" });
    mount();
    await waitFor(() => expect(screen.getAllByTestId("scratchpad-note")).toHaveLength(2));
    await act(async () => useScratchpad.getState().open("note_b"));
    await act(async () => useScratchpad.getState().close());
    // Typed into note B, then deleted before its save went out.
    act(() => {
      useScratchpad.setState({ activeId: "note_b", text: "# Bin me" });
      useScratchpad.getState().edit("# Bin me now");
      useScratchpad.setState({ activeId: null, text: "" });
    });
    const before = writes(d.calls).length;
    const row = screen
      .getAllByTestId("scratchpad-note")
      .find((r) => r.getAttribute("data-id") === "note_b") as HTMLElement;
    fireEvent.click(within(row.parentElement as HTMLElement).getByRole("button", { name: /^Delete / }));
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog.textContent).toContain("Delete “Bin me”?");
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete note" }));
    await waitFor(() =>
      expect(screen.getAllByTestId("scratchpad-note").map((r) => r.getAttribute("data-id"))).toEqual([
        "note_a",
      ]),
    );
    await act(async () => new Promise((r) => setTimeout(r, 2 * SAVE_MS)));
    expect(writes(d.calls).length).toBe(before);
    expect(d.notes.has("note_b")).toBe(false);
    expect(screen.queryByText(/Couldn't save this note/)).toBeNull();
  });

  it("F-SH-12 a failed save keeps the text, says so, and Retry writes it", async () => {
    const d = disk({ note_a: "" });
    mount();
    await act(async () => useScratchpad.getState().open("note_a"));
    d.failing(true);
    act(() => useScratchpad.getState().edit("important"));
    await act(async () => useScratchpad.getState().flush());
    expect(await screen.findByText("Couldn't save this note: disk full")).toBeTruthy();
    expect(d.notes.get("note_a")).toBe("");
    d.failing(false);
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(d.notes.get("note_a")).toBe("important"));
    await waitFor(() => expect(screen.queryByText(/Couldn't save this note/)).toBeNull());
  });

  it("F-SH-12 the list loads newest first; a failure to load offers Try again", async () => {
    const calls = fakeBackend({
      scratchpad_list: () => {
        throw { code: "internal", message: "permission denied", stage: null, hint: null, retryable: false };
      },
    });
    mount();
    expect(await screen.findByText("Couldn't load your notes")).toBeTruthy();
    expect(calls.filter((c) => c.cmd === "scratchpad_list")).toHaveLength(1);
    disk({ note_a: "older", note_b: "newer" });
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() =>
      expect(screen.getAllByTestId("scratchpad-note").map((r) => r.getAttribute("data-id"))).toEqual([
        "note_b",
        "note_a",
      ]),
    );
  });
});
