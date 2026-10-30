import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { MemorySuggestions } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { waiting } from "../../test/waiting";
import { SuggestionCards, useSuggestions } from "./Suggestions";

const ASK = {
  teammateId: "tm_scout",
  threadId: "th_1",
  setId: "set_1",
  items: [
    { kind: "add", text: "Prefers pnpm." },
    { kind: "remove", text: "Uses npm." },
  ],
  rationale: "You said so twice.",
  applied: false,
  changeId: null,
} as unknown as MemorySuggestions;

function mount() {
  render(
    <UiProvider>
      <SuggestionCards threadId="th_1" />
    </UiProvider>,
  );
}

describe("F-AG-08 memory suggestions", () => {
  beforeEach(() => resetStores());

  it("F-AG-08.1 Ask: Keep/Discard each item, then resolve once with the kept indices", async () => {
    const calls = fakeBackend({ memory_suggestions_resolve: () => null });
    mount();
    act(() => useSuggestions.getState().add(ASK));
    expect(screen.getAllByTestId("memory-suggestion")).toHaveLength(2);
    const keep = screen.getAllByTestId("memory-suggestion-keep")[0]!;
    await waitFor(() => expect(waiting(keep)).toBe(false));
    fireEvent.click(keep);
    expect(calls.some((c) => c.cmd === "memory_suggestions_resolve")).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    await waitFor(() => expect(screen.queryByTestId("memory-suggestions")).toBeNull());
    expect(calls.filter((c) => c.cmd === "memory_suggestions_resolve")).toEqual([
      { cmd: "memory_suggestions_resolve", args: { suggestionSetId: "set_1", keep: [0] } },
    ]);
  });

  it("F-AG-08 Auto mode: the note's Undo reverts the change", async () => {
    const calls = fakeBackend({ memory_undo: () => null });
    mount();
    act(() => useSuggestions.getState().add({ ...ASK, applied: true, changeId: "mc_9" }));
    fireEvent.click(await screen.findByRole("button", { name: "Undo" }));
    await waitFor(() => expect(screen.queryByTestId("memory-updated")).toBeNull());
    expect(calls).toContainEqual({ cmd: "memory_undo", args: { changeId: "mc_9" } });
  });

  it("F-TL-12.16 Keep writes to memory, so it waits until its row has held still, and Discard doesn't", async () => {
    const calls = fakeBackend({ memory_suggestions_resolve: () => null });
    mount();
    act(() => useSuggestions.getState().add(ASK));
    const [keep] = screen.getAllByTestId("memory-suggestion-keep");
    // Just arrived: Keep is reachable but off, and says why.
    expect(waiting(keep!)).toBe(true);
    expect(keep!.getAttribute("data-grant")).not.toBeNull();
    expect(keep!.getAttribute("title")).toBe(
      "Keep turns on once this has been still on screen for a moment.",
    );
    fireEvent.click(keep!);
    expect(screen.getAllByTestId("memory-suggestion-keep")).toHaveLength(2);
    // Discard needs no wait: the second row is decided at once.
    fireEvent.click(screen.getAllByRole("button", { name: "Discard" })[1]!);
    expect(screen.getAllByTestId("memory-suggestion-keep")).toHaveLength(1);
    await waitFor(() => expect(waiting(screen.getByTestId("memory-suggestion-keep"))).toBe(false));
    fireEvent.click(screen.getByTestId("memory-suggestion-keep"));
    await waitFor(() =>
      expect(calls.filter((c) => c.cmd === "memory_suggestions_resolve")).toEqual([
        { cmd: "memory_suggestions_resolve", args: { suggestionSetId: "set_1", keep: [0] } },
      ]),
    );
  });

  it("F-AG-08 suggestions show only in their own chat", () => {
    fakeBackend();
    mount();
    act(() => useSuggestions.getState().add({ ...ASK, threadId: "th_other" }));
    expect(screen.queryByTestId("memory-suggestions")).toBeNull();
  });
});
