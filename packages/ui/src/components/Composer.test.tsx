import { act, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { UiProvider } from "../theme/ThemeProvider";
import { Composer, mentionAt, type ComposerProps, type MentionItem } from "./Composer";

function Harness(props: Partial<ComposerProps> & { initial?: string }) {
  const [value, setValue] = useState(props.initial ?? "");
  return (
    <UiProvider theme="dark">
      <Composer placeholder="Ask anything…" onSend={() => undefined} {...props} value={value} onChange={(v) => { setValue(v); props.onChange?.(v); }} />
    </UiProvider>
  );
}

const box = () => screen.getByRole("textbox", { name: "Message" });

describe("Composer (F-TH-07, UI-22)", () => {
  it("F-TH-07 Enter sends, Shift+Enter doesn't, and an empty message never sends", () => {
    const onSend = vi.fn();
    render(<Harness onSend={onSend} />);
    fireEvent.keyDown(box(), { key: "Enter" });
    expect(onSend).not.toHaveBeenCalled();
    fireEvent.change(box(), { target: { value: "hello" } });
    fireEvent.keyDown(box(), { key: "Enter", shiftKey: true });
    expect(onSend).not.toHaveBeenCalled();
    fireEvent.keyDown(box(), { key: "Enter" });
    expect(onSend).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("composer-send")).toHaveProperty("disabled", false);
  });

  it("F-TH-07 Enter during IME composition never sends", () => {
    const onSend = vi.fn();
    render(<Harness onSend={onSend} initial="かな" />);
    fireEvent.keyDown(box(), { key: "Enter", isComposing: true });
    fireEvent.keyDown(box(), { key: "Enter", keyCode: 229 });
    expect(onSend).not.toHaveBeenCalled();
  });

  it("F-TH-07 Send with Mod+Enter swaps the keys", () => {
    const onSend = vi.fn();
    render(<Harness onSend={onSend} initial="hi" sendWithModEnter />);
    fireEvent.keyDown(box(), { key: "Enter" });
    expect(onSend).not.toHaveBeenCalled();
    fireEvent.keyDown(box(), { key: "Enter", ctrlKey: true });
    expect(onSend).toHaveBeenCalledTimes(1);
  });

  it("F-TH-07 Esc stops a running turn; ↑ in an empty composer recalls the last message", () => {
    const onStop = vi.fn();
    render(<Harness running onStop={onStop} onRecallLast={() => "fix the redirect"} />);
    expect(screen.getByTestId("composer-stop")).toBeTruthy();
    fireEvent.keyDown(box(), { key: "Escape" });
    expect(onStop).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(box(), { key: "ArrowUp" });
    expect((box() as HTMLTextAreaElement).value).toBe("fix the redirect");
  });

  it("F-TH-07 a queued message shows as a chip and can be removed", () => {
    const onRemoveQueued = vi.fn();
    render(<Harness running onStop={() => undefined} queued="next" onRemoveQueued={onRemoveQueued} />);
    expect(screen.getByText("Queued · sends when this turn finishes")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Remove queued message" }));
    expect(onRemoveQueued).toHaveBeenCalled();
  });

  it("F-TH-07 @ opens the picker; Enter picks into a chip and removes the query", async () => {
    const items: MentionItem[] = [
      { id: "src/", label: "src/", kind: "folder" },
      { id: "src/auth.ts", label: "src/auth.ts", kind: "file" },
    ];
    const mentions = vi.fn((q: string) => items.filter((i) => i.label.startsWith(q)));
    const onMention = vi.fn();
    const onSend = vi.fn();
    render(<Harness mentions={mentions} onMention={onMention} onSend={onSend} />);
    const el = box() as HTMLTextAreaElement;
    await act(async () => {
      fireEvent.change(el, { target: { value: "look at @src/a", selectionStart: 14 } });
    });
    expect(mentions).toHaveBeenLastCalledWith("src/a");
    expect(screen.getByRole("listbox")).toBeTruthy();
    expect(screen.getAllByRole("option")).toHaveLength(1);
    fireEvent.keyDown(el, { key: "Enter" });
    expect(onSend).not.toHaveBeenCalled();
    expect(onMention).toHaveBeenCalledWith(items[1]);
    expect(el.value).toBe("look at ");
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("F-TH-07 a pasted image is handed to the app; the disabled reason shows", () => {
    const onPasteImage = vi.fn();
    render(<Harness onPasteImage={onPasteImage} disabledReason="Claude Code isn't signed in." />);
    const clip = (items: unknown[], types: string[], text: string) => ({ items, types, getData: () => text });
    fireEvent.paste(box(), { clipboardData: clip([{ kind: "file", type: "image/png" }], ["Files"], "") });
    fireEvent.paste(box(), { clipboardData: clip([], ["image/png"], "") });
    fireEvent.paste(box(), { clipboardData: clip([], [], "") });
    expect(onPasteImage).toHaveBeenCalledTimes(3);
    // Text pastes as text.
    fireEvent.paste(box(), { clipboardData: clip([{ kind: "string", type: "text/plain" }], ["text/plain"], "hi") });
    expect(onPasteImage).toHaveBeenCalledTimes(3);
    expect(screen.getByText("Claude Code isn't signed in.")).toBeTruthy();
    expect(box()).toHaveProperty("disabled", true);
  });

  it("mentionAt finds the @query before the caret only at a word start", () => {
    expect(mentionAt("see @src/a", 10)).toEqual({ start: 4, query: "src/a" });
    expect(mentionAt("@", 1)).toEqual({ start: 0, query: "" });
    expect(mentionAt("mail a@b.co", 11)).toBeNull();
    expect(mentionAt("@src done", 9)).toBeNull();
  });
});
