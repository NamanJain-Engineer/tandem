// The browser demo's Files tab: the same made-up "shop" project its terminal
// lists (`ls`, `git status`), in memory. Edits stay in the page and checks
// work like the core's (a stale hash is a conflict); nothing is saved.
import type { EditorTabs, FileContent, FileDiff, FsEntry } from "@tandem/protocol";

const REDIRECT = `import type { Session } from "../session";

/** Where a shopper goes next: sign-in first, then their cart or payment. */
export function nextStep(session: Session): string {
  if (!session.user) {
    return \`/login?next=\${encodeURIComponent(session.path)}\`;
  }
  return session.cart.empty ? "/cart" : "/checkout/pay";
}
`;

const SOURCES: Record<string, string> = {
  "README.md": "# shop\n\nA made-up storefront for the Vibezeo browser demo. Nothing here runs.\n",
  "package.json": `{
  "name": "shop",
  "private": true,
  "type": "module",
  "scripts": { "dev": "vite", "test": "vitest run" }
}
`,
  "tsconfig.json": `{ "compilerOptions": { "strict": true, "module": "ESNext", "target": "ES2022" } }\n`,
  "vite.config.ts": `import { defineConfig } from "vite";\n\nexport default defineConfig({});\n`,
  "src/checkout/redirect.ts": REDIRECT,
  "src/checkout/redirect.test.ts": `import { describe, expect, it } from "vitest";
import { nextStep } from "./redirect";
import { guest } from "../../tests/setup";

describe("nextStep", () => {
  it("sends guests to sign in first", () => {
    expect(nextStep(guest("/cart"))).toBe("/login?next=%2Fcart");
  });
});
`,
  "src/checkout/cart.ts": `export type Cart = { items: string[]; empty: boolean };\n\nexport const emptyCart: Cart = { items: [], empty: true };\n`,
  "src/api/orders.ts": `export async function orders(userId: string): Promise<string[]> {\n  return [userId];\n}\n`,
  "src/api/rate-limit.ts": `export const LIMIT_PER_MINUTE = 300;\n`,
  "src/session.ts": `import type { Cart } from "./checkout/cart";\n\nexport type Session = { user: string | null; path: string; cart: Cart };\n`,
  "tests/setup.ts": `import type { Session } from "../src/session";\n\nexport const guest = (path: string): Session => ({ user: null, path, cart: { items: ["tea"], empty: false } });\n`,
};

/** What `git status` in the demo terminal says. */
const MODIFIED = new Set(["src/checkout/redirect.ts", "src/checkout/redirect.test.ts"]);

const hash = (text: string) => {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 0x01000193);
  return `demo-${(h >>> 0).toString(16)}`;
};

export class DemoFiles {
  private files = new Map(Object.entries(SOURCES));
  private tabs: EditorTabs = { tabs: [], active: null, expanded: [], showIgnored: false, divider: null, view: null };

  list(dir: string): FsEntry[] {
    const prefix = dir ? `${dir}/` : "";
    const dirs = new Set<string>();
    const files: string[] = [];
    for (const path of this.files.keys()) {
      if (!path.startsWith(prefix)) continue;
      const rest = path.slice(prefix.length);
      const at = rest.indexOf("/");
      if (at >= 0) dirs.add(rest.slice(0, at));
      else files.push(rest);
    }
    const changed = (p: string) => [...MODIFIED].some((m) => m === p || m.startsWith(`${p}/`));
    const sort = (a: string, b: string) => a.toLowerCase().localeCompare(b.toLowerCase());
    return [
      ...[...dirs].sort(sort).map((name): FsEntry => {
        const path = prefix + name;
        return { name, path, kind: "dir", symlink: false, ignored: false, gitStatus: changed(path) ? "modified" : undefined };
      }),
      ...files.sort(sort).map((name): FsEntry => {
        const path = prefix + name;
        return { name, path, kind: "file", symlink: false, ignored: false, gitStatus: MODIFIED.has(path) ? "modified" : undefined };
      }),
    ];
  }

  read(path: string): FileContent {
    const text = this.files.get(path);
    if (text === undefined) throw { code: "not_found", message: "That file or folder doesn't exist" };
    return { path, kind: "text", text, size: text.length, readonly: false, readonlyReason: null, truncated: false, hash: hash(text), mtime: 0, dataUrl: null, hex: null };
  }

  write(path: string, text: string, expected: string | null): { hash: string; mtime: number } {
    const now = this.files.get(path);
    if (now !== undefined && expected !== hash(now)) throw { code: "conflict", message: "File changed on disk" };
    if (now === undefined && expected !== null) throw { code: "conflict", message: "File changed on disk" };
    this.files.set(path, text);
    return { hash: hash(text), mtime: Date.now() };
  }

  compare(path: string, text: string): FileDiff {
    const disk = this.files.get(path) ?? "";
    const a = disk.split("\n");
    const b = text.split("\n");
    const lines = [`--- a/${path}`, `+++ b/${path}`, `@@ -1,${a.length} +1,${b.length} @@`, ...a.map((l) => `-${l}`), ...b.map((l) => `+${l}`)];
    return { path, change: "modified", additions: b.length, deletions: a.length, binary: false, truncated: false, patch: lines.join("\n") };
  }

  savedTabs(): EditorTabs {
    return this.tabs;
  }

  saveTabs(state: EditorTabs): void {
    this.tabs = state;
  }
}
