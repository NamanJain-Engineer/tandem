// Writes apps/desktop/src/i18n/en-XA.json: accented, ~35% longer strings with
// ICU placeholders intact. Layout tests use it to catch truncation (§5.5).
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pseudoCatalog } from "../apps/desktop/src/i18n/pseudo";

const root = new URL("..", import.meta.url).pathname;
const dir = join(root, "apps/desktop/src/i18n");
const en = JSON.parse(readFileSync(join(dir, "en.json"), "utf8")) as Record<string, string>;

const out = pseudoCatalog(en);
writeFileSync(join(dir, "en-XA.json"), JSON.stringify(out, null, 2) + "\n");
console.log(`en-XA: ${Object.keys(out).length} messages`);
