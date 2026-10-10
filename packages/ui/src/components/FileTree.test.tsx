import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { FileTree, VIRTUALIZE_ABOVE, visibleRows, type FileTreeNode } from "./FileTree";

const ENTRIES: Record<string, FileTreeNode[] | undefined> = {
  "": [
    { path: "src", name: "src", kind: "dir", gitStatus: "modified" },
    { path: "node_modules", name: "node_modules", kind: "dir", ignored: true },
    { path: "README.md", name: "README.md", kind: "file" },
  ],
  src: [
    { path: "src/lib", name: "lib", kind: "dir" },
    { path: "src/main.rs", name: "main.rs", kind: "file", gitStatus: "modified" },
  ],
  "src/lib": [{ path: "src/lib/mod.rs", name: "mod.rs", kind: "file", gitStatus: "added" }],
};

function tree(props: Partial<Parameters<typeof FileTree>[0]> = {}) {
  const onToggle = vi.fn();
  const onOpen = vi.fn();
  render(<FileTree entries={ENTRIES} expanded={new Set(["src"])} onToggle={onToggle} onOpen={onOpen} {...props} />);
  return { onToggle, onOpen, el: screen.getByRole("tree", { name: "Files" }) };
}

const paths = () => screen.queryAllByRole("treeitem").map((r) => r.getAttribute("data-path"));

describe("FileTree (UI_SPEC §2.26, F-CD-19)", () => {
  it("shows expanded folders inline with levels and git letters", () => {
    tree();
    expect(paths()).toEqual(["src", "src/lib", "src/main.rs", "node_modules", "README.md"]);
    const main = screen.getByRole("treeitem", { name: "main.rs, modified" });
    expect(main.getAttribute("aria-level")).toBe("2");
    expect(main.textContent).toContain("M");
    expect(screen.getByRole("treeitem", { name: "node_modules, ignored" }).getAttribute("aria-expanded")).toBe("false");
    expect(screen.getByRole("treeitem", { name: "src, modified" }).getAttribute("aria-expanded")).toBe("true");
  });

  it("clicking a folder toggles it and clicking a file opens it", () => {
    const { onToggle, onOpen } = tree();
    fireEvent.click(screen.getByRole("treeitem", { name: "src, modified" }));
    expect(onToggle).toHaveBeenCalledWith("src", false);
    fireEvent.click(screen.getByRole("treeitem", { name: "README.md" }));
    expect(onOpen).toHaveBeenCalledWith(expect.objectContaining({ path: "README.md" }));
  });

  it("arrow keys move, Right expands, Left goes to the parent, Enter opens, letters jump", () => {
    const { el, onToggle, onOpen } = tree();
    const active = () => el.getAttribute("aria-activedescendant");
    const activePath = () => (active() ? document.getElementById(active() ?? "")?.getAttribute("data-path") : null);
    expect(activePath()).toBe("src");
    fireEvent.keyDown(el, { key: "ArrowDown" });
    expect(activePath()).toBe("src/lib");
    fireEvent.keyDown(el, { key: "ArrowRight" });
    expect(onToggle).toHaveBeenCalledWith("src/lib", true);
    fireEvent.keyDown(el, { key: "ArrowLeft" });
    expect(activePath()).toBe("src");
    fireEvent.keyDown(el, { key: "End" });
    expect(activePath()).toBe("README.md");
    fireEvent.keyDown(el, { key: "Enter" });
    expect(onOpen).toHaveBeenCalledWith(expect.objectContaining({ path: "README.md" }));
    fireEvent.keyDown(el, { key: "m" });
    expect(activePath()).toBe("src/main.rs");
  });

  it("the filter shows loaded matches with the folders on their way", () => {
    tree({ expanded: new Set() });
    fireEvent.change(screen.getByRole("searchbox", { name: "Filter files" }), { target: { value: "mod.rs" } });
    expect(paths()).toEqual(["src", "src/lib", "src/lib/mod.rs"]);
    fireEvent.change(screen.getByRole("searchbox", { name: "Filter files" }), { target: { value: "zzz" } });
    expect(screen.queryByRole("tree")).toBeNull();
    expect(screen.getByRole("status").textContent).toContain('No loaded files match "zzz"');
  });

  it("loading, empty and error states say so, and an error can be retried", () => {
    const onRetry = vi.fn();
    const { unmount } = render(<FileTree entries={{}} expanded={new Set()} onToggle={vi.fn()} onOpen={vi.fn()} />);
    expect(screen.getByRole("status").textContent).toContain("Loading…");
    unmount();
    render(<FileTree entries={{}} errors={{ "": "EACCES" }} expanded={new Set()} onToggle={vi.fn()} onOpen={vi.fn()} onRetry={onRetry} />);
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(onRetry).toHaveBeenCalledWith("");
  });

  it("a folder still loading shows a loading row under it", () => {
    const rows = visibleRows({ "": [{ path: "a", name: "a", kind: "dir" }] }, new Set(["a"]));
    expect(rows.map((r) => r.type)).toEqual(["node", "loading"]);
  });

  it("hands a row's pointer-down to the app (drag out to panes and composers)", () => {
    const onRowPointerDown = vi.fn();
    tree({ onRowPointerDown });
    fireEvent.pointerDown(screen.getByRole("treeitem", { name: "README.md" }));
    expect(onRowPointerDown).toHaveBeenCalledWith(expect.objectContaining({ path: "README.md" }), expect.anything());
  });

  it("long folders render only the rows in view", () => {
    const many: FileTreeNode[] = Array.from({ length: VIRTUALIZE_ABOVE + 100 }, (_, i) => ({
      path: `f${i}.txt`,
      name: `f${i}.txt`,
      kind: "file" as const,
    }));
    render(<FileTree entries={{ "": many }} expanded={new Set()} onToggle={vi.fn()} onOpen={vi.fn()} />);
    const shown = screen.getAllByRole("treeitem").length;
    expect(shown).toBeGreaterThan(10);
    expect(shown).toBeLessThan(100);
  });
});
