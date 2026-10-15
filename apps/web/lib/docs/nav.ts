import type { PageTree } from "fumadocs-core/server";

export type NavPage = { title: string; url: string };
export type NavGroup = { title: string; pages: NavPage[] };

const text = (n: unknown): string => (typeof n === "string" ? n : String(n ?? ""));

/** The sidebar's groups (D-1839): content/docs/meta.json's separators become
 * labels, and the pages after each one its links, in order. Folders flatten
 * into the group they sit in. */
export function navGroups(tree: PageTree.Root): NavGroup[] {
  const groups: NavGroup[] = [];
  const add = (page: NavPage) => {
    if (groups.length === 0) groups.push({ title: "", pages: [] });
    groups[groups.length - 1]?.pages.push(page);
  };
  const walk = (nodes: PageTree.Node[]) => {
    for (const n of nodes) {
      if (n.type === "separator") groups.push({ title: text(n.name), pages: [] });
      else if (n.type === "page") add({ title: text(n.name), url: n.url });
      else {
        if (n.index) add({ title: text(n.index.name), url: n.index.url });
        walk(n.children);
      }
    }
  };
  walk(tree.children);
  return groups.filter((g) => g.pages.length > 0);
}
