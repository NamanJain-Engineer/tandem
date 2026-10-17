// Fumadocs content for /docs (F-WB-02, DECISIONS "Web") and the blog (F-WB-06).
import { defineCollections, defineConfig, defineDocs } from "fumadocs-mdx/config";
import { blogFrontmatter } from "./lib/blog/schema";

export const docs = defineDocs({ dir: "content/docs" });

/** content/blog/<slug>.mdx (or .md). A bad frontmatter fails the build. */
export const blog = defineCollections({ type: "doc", dir: "content/blog", schema: blogFrontmatter });

export default defineConfig();
