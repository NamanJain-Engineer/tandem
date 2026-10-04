// Writes apps/desktop/src/i18n/en-XA.json: accented, ~35% longer strings with
// ICU placeholders intact. Layout tests use it to catch truncation (§5.5).
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const root = new URL("..", import.meta.url).pathname;
const dir = join(root, "apps/desktop/src/i18n");
const en = JSON.parse(readFileSync(join(dir, "en.json"), "utf8")) as Record<string, string>;

const accents: Record<string, string> = {
  a: "à",
  e: "é",
  i: "î",
  o: "ô",
  u: "ü",
  c: "ç",
  n: "ñ",
  y: "ý",
  A: "À",
  E: "É",
  I: "Î",
  O: "Ô",
  U: "Ü",
  C: "Ç",
  N: "Ñ",
  Y: "Ý",
};

export function pseudo(message: string): string {
  let depth = 0;
  let out = "";
  for (const ch of message) {
    if (ch === "{") depth++;
    if (ch === "}") depth--;
    out += depth === 0 ? (accents[ch] ?? ch) : ch;
  }
  const visible = message.replace(/\{[^}]*\}/g, "").length;
  return `[${out}${"~".repeat(Math.ceil(visible * 0.35))}]`;
}

const out = Object.fromEntries(Object.entries(en).map(([k, v]) => [k, pseudo(v)]));
writeFileSync(join(dir, "en-XA.json"), JSON.stringify(out, null, 2) + "\n");
console.log(`en-XA: ${Object.keys(out).length} messages`);
