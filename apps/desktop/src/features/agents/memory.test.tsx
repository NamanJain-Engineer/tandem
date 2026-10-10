import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { Memory, Teammate } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { MemoryTab } from "./MemoryTab";

const SCOUT = {
  id: "tm_scout",
  name: "Scout",
  home: "/data/teammates/tm_scout",
  settings: {
    memoryCap: 4096,
    learnMode: "ask",
    mayMessage: true,
    maySchedule: false,
    defaultAccess: "edits",
  },
} as unknown as Teammate;

const RAW =
  "# Notes\n\n- Release branch is cut on Thursdays.\n  Never on Fridays.\n- Prefers pnpm.\ntrailing text";

function setup(initial = RAW) {
  let notes = initial;
  const calls: Call[] = fakeBackend({
    memory_get: (): Memory => ({
      notes,
      about: "",
      cap: 40,
      used: notes.length,
      aboutCap: 2048,
      aboutUsed: 0,
    }),
    memory_write: (a) => {
      notes = String(a?.markdown);
      return null;
    },
    // The core's redactor: anything with "token=" is a secret here.
    redact_text: (a) => String(a?.text).replace(/token=\S+/g, "token=[REDACTED]"),
  });
  render(
    <UiProvider>
      <MemoryTab teammate={SCOUT} />
    </UiProvider>,
  );
  return { calls, notes: () => notes };
}

describe("UI-73 Memory tab", () => {
  beforeEach(() => resetStores());

  it("F-AG-07.3 List → Raw → List keeps the file byte for byte (nothing is written)", async () => {
    const { calls } = setup();
    await screen.findByTestId("memory-notes");
    expect(screen.getAllByTestId("memory-note").map((n) => n.textContent)).toEqual([
      expect.stringContaining("Release branch is cut on Thursdays. Never on Fridays."),
      expect.stringContaining("Prefers pnpm."),
    ]);
    fireEvent.click(screen.getByRole("radio", { name: "Raw" }));
    expect((screen.getByTestId("memory-raw") as HTMLTextAreaElement).value).toBe(RAW);
    fireEvent.click(screen.getByRole("radio", { name: "List" }));
    await screen.findByTestId("memory-notes");
    expect(calls.some((c) => c.cmd === "memory_write")).toBe(false);
  });

  it("F-AG-07 Add note and delete write the file and leave every other byte", async () => {
    const { notes } = setup("- a\nkeep\n");
    await screen.findByTestId("memory-notes");
    fireEvent.change(screen.getByRole("textbox", { name: "New note" }), { target: { value: "b" } });
    await act(async () => fireEvent.click(screen.getByTestId("memory-add")));
    await waitFor(() => expect(notes()).toBe("- a\nkeep\n- b\n"));
    await waitFor(() => expect(screen.getAllByTestId("memory-note")).toHaveLength(2));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Delete note: a" })));
    await waitFor(() => expect(notes()).toBe("keep\n- b\n"));
  });

  it("F-AG-07.2 a credential-shaped note can't be added", async () => {
    setup("");
    await screen.findByText("No notes yet");
    fireEvent.change(screen.getByRole("textbox", { name: "New note" }), {
      target: { value: "token=ghp_abc" },
    });
    await screen.findByText(/This looks like a secret/);
    expect((screen.getByTestId("memory-add") as HTMLButtonElement).disabled).toBe(true);
  });

  it("F-AG-07.1 the budget meter shows use against the cap and Save is off over it", async () => {
    setup("- short\n");
    expect((await screen.findByTestId("memory-budget")).textContent).toBe("8 / 40 characters");
    fireEvent.click(screen.getByRole("radio", { name: "Raw" }));
    fireEvent.change(screen.getByTestId("memory-raw"), { target: { value: "x".repeat(50) } });
    await screen.findByText(/Over the budget by 10 characters/);
    expect((screen.getByTestId("memory-save") as HTMLButtonElement).disabled).toBe(true);
  });
});
