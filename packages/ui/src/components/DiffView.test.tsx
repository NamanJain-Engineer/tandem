import { fireEvent, render, screen } from "@testing-library/react";
import { axe } from "vitest-axe";
import { describe, expect, it, vi } from "vitest";
import { DiffView, LARGE_DIFF_LINES, parsePatch, type DiffFile } from "./DiffView";

const PATCH = [
  "diff --git a/app.txt b/app.txt",
  "index 5626abf..f719efd 100644",
  "--- a/app.txt",
  "+++ b/app.txt",
  "@@ -1,3 +1,3 @@ fn main",
  " one",
  "-two",
  "+deux",
  " three",
  "\\ No newline at end of file",
].join("\n");

const file = (over: Partial<DiffFile> = {}): DiffFile => ({
  path: "app.txt",
  change: "modified",
  additions: 1,
  deletions: 1,
  binary: false,
  patch: PATCH,
  truncated: false,
  ...over,
});

describe("DiffView (UI_SPEC §2.25)", () => {
  it("parses hunks with old and new line numbers and drops the git header", () => {
    expect(parsePatch(PATCH)).toEqual([
      { kind: "hunk", text: "@@ -1,3 +1,3 @@ fn main" },
      { kind: "context", text: "one", oldNo: 1, newNo: 1 },
      { kind: "del", text: "two", oldNo: 2, newNo: null },
      { kind: "add", text: "deux", oldNo: null, newNo: 2 },
      { kind: "context", text: "three", oldNo: 3, newNo: 3 },
      { kind: "note", text: "No newline at end of file" },
    ]);
  });

  it("shows each file's path, letter and counts, and collapses on the caret", () => {
    render(<DiffView files={[file(), file({ path: "new.ts", oldPath: "old.ts", change: "renamed", additions: 0, deletions: 0, patch: "" })]} />);
    const blocks = screen.getAllByTestId("diff-file");
    expect(blocks).toHaveLength(2);
    expect(blocks[0]?.textContent).toContain("app.txt");
    expect(blocks[0]?.textContent).toContain("+1");
    expect(blocks[0]?.textContent).toContain("−1");
    expect(blocks[1]?.textContent).toContain("old.ts → new.ts");
    expect(blocks[0]?.querySelectorAll('[data-kind="add"]')).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Collapse app.txt" }));
    expect(blocks[0]?.querySelectorAll('[data-kind="add"]')).toHaveLength(0);
    expect(screen.getByRole("button", { name: "Expand app.txt" })).toBeTruthy();
  });

  it("says when a file is binary, cut short or when there are no changes", () => {
    const { rerender } = render(<DiffView files={[file({ binary: true, patch: "" })]} />);
    expect(screen.getByText("Binary file changed")).toBeTruthy();
    rerender(<DiffView files={[file({ truncated: true })]} />);
    expect(screen.getByText("This diff is too long to show in full.")).toBeTruthy();
    rerender(<DiffView files={[]} labels={{ empty: "Nothing changed yet" }} />);
    expect(screen.getByText("Nothing changed yet")).toBeTruthy();
  });

  it("starts a file over 2,000 lines collapsed with a Show diff button", () => {
    const body = Array.from({ length: LARGE_DIFF_LINES + 5 }, (_, i) => `+line ${i}`).join("\n");
    render(<DiffView files={[file({ additions: LARGE_DIFF_LINES + 5, deletions: 0, patch: `@@ -0,0 +1,${LARGE_DIFF_LINES + 5} @@\n${body}` })]} />);
    const show = screen.getByRole("button", { name: "Show diff (2,006 lines)" });
    expect(screen.queryAllByRole("row")).toHaveLength(0);
    fireEvent.click(show);
    expect(screen.getAllByRole("row").length).toBe(LARGE_DIFF_LINES + 6);
  });

  it("has no axe violations", async () => {
    const { container } = render(<DiffView files={[file()]} />);
    expect(await axe(container)).toHaveNoViolations();
  });
});

describe("DiffView names (F-CD-23)", () => {
  const file = {
    path: "src/main.py",
    change: "modified" as const,
    additions: 1,
    deletions: 1,
    binary: false,
    truncated: false,
    patch: "diff --git a/src/main.py b/src/main.py\n@@ -1,2 +1,2 @@\n import util\n-old_call()\n+util.helper(1)\n",
  };

  it("F-CD-23.2 a name on a new-side line offers Go to definition and Find references", async () => {
    const onDefinition = vi.fn();
    const onReferences = vi.fn();
    render(
      <DiffView
        files={[file]}
        names={{ definition: "Go to definition", references: "Find references", onDefinition, onReferences }}
      />,
    );
    // Removed lines have no names to click.
    expect(screen.queryByText("old_call()")).not.toBeNull();
    expect(document.querySelector('[data-testid="diff-name"][data-name="old_call"]')).toBeNull();
    const helper = document.querySelector<HTMLElement>('[data-testid="diff-name"][data-name="helper"]');
    expect(helper).not.toBeNull();
    fireEvent.click(helper as HTMLElement);
    fireEvent.click(await screen.findByRole("menuitem", { name: "Find references" }));
    expect(onReferences).toHaveBeenCalledWith({ path: "src/main.py", line: 2, character: 5, name: "helper" });
    fireEvent.click(document.querySelector('[data-testid="diff-name"][data-name="util"]') as HTMLElement);
    fireEvent.click(await screen.findByRole("menuitem", { name: "Go to definition" }));
    expect(onDefinition).toHaveBeenCalledWith({ path: "src/main.py", line: 1, character: 7, name: "util" });
    // Names are buttons: the keyboard reaches them.
    expect(helper?.tagName).toBe("BUTTON");
    expect(screen.getAllByRole("button", { name: "helper" }).length).toBeGreaterThan(0);
  });
});
