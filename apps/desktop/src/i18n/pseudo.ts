// The en-XA pseudo-locale (§5.5): accented, ~35% longer strings with ICU
// placeholders intact, so layout tests catch truncation. scripts/gen-pseudo.ts
// writes it to a file; debug builds can switch to it live (E2E).
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


export function pseudoCatalog(source: Record<string, string>): Record<string, string> {
  return Object.fromEntries(Object.entries(source).map(([k, v]) => [k, pseudo(v)]));
}
