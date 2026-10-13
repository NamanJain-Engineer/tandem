import type { PageTree } from "fumadocs-core/server";
import { describe, expect, it } from "vitest";
import { withSlashes } from "./tree";

// The export serves each docs page at its slash URL, and Next's router
// fetches a link's payload from the link's own path (D-1817).

const tree: PageTree.Root = {
  name: "Docs",
  children: [
    { type: "page", name: "Getting started", url: "/docs" },
    { type: "separator", name: "Modes" },
    { type: "page", name: "Code mode", url: "/docs/code-mode" },
    {
      type: "folder",
      name: "Remote",
      index: { type: "page", name: "Remote", url: "/docs/remote" },
      children: [
        { type: "page", name: "SSH", url: "/docs/remote/ssh" },
        { type: "folder", name: "More", children: [{ type: "page", name: "WSL", url: "/docs/remote/wsl/" }] },
      ],
    },
    { type: "page", name: "Source", url: "https://example.com/x", external: true },
  ],
  fallback: { name: "Other", children: [{ type: "page", name: "Old", url: "/docs/old" }] },
};

const urls = (nodes: PageTree.Node[]): string[] =>
  nodes.flatMap((n) =>
    n.type === "page"
      ? [n.url]
      : n.type === "folder"
        ? [...(n.index ? [n.index.url] : []), ...urls(n.children)]
        : [],
  );

describe("docs page tree (F-WB-02)", () => {
  it("F-WB-02 every page in the sidebar is linked at its slash URL, external links as they are (D-1817)", () => {
    const out = withSlashes(tree);
    expect(urls(out.children)).toEqual([
      "/docs/",
      "/docs/code-mode/",
      "/docs/remote/",
      "/docs/remote/ssh/",
      "/docs/remote/wsl/",
      "https://example.com/x",
    ]);
    expect(urls(out.fallback?.children ?? [])).toEqual(["/docs/old/"]);
    expect(out.children[1]).toEqual(tree.children[1]);
  });

  it("F-WB-02 the loader's tree is left as it was (D-1817)", () => {
    const before = JSON.stringify(tree);
    withSlashes(tree);
    expect(JSON.stringify(tree)).toBe(before);
  });
});
