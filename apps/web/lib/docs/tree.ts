import type { PageTree } from "fumadocs-core/server";

/** The docs tree with every page at its slash URL (D-1817), for the sidebar
 * and the page footer. The export serves each page at /docs/x/
 * (`trailingSlash`), and Next's router fetches a link's payload from the
 * link's own path: /docs/x.txt for "/docs/x", which doesn't exist, so every
 * prefetch failed and every click fell back to a full load through a
 * redirect. Fumadocs' loader builds URLs without the slash and strips one from
 * a custom `url`, so the tree is mapped here. */
export function withSlashes(tree: PageTree.Root): PageTree.Root {
  return {
    ...tree,
    children: tree.children.map(node),
    ...(tree.fallback && { fallback: withSlashes(tree.fallback) }),
  };
}

function node(n: PageTree.Node): PageTree.Node {
  if (n.type === "page") return item(n);
  if (n.type === "folder")
    return { ...n, ...(n.index && { index: item(n.index) }), children: n.children.map(node) };
  return n;
}

function item(page: PageTree.Item): PageTree.Item {
  return page.external || page.url.endsWith("/") ? page : { ...page, url: `${page.url}/` };
}
