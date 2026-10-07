// The TOML subset our own config files use: `[[table]]` arrays of tables,
// `[table]`, and `key = "string" | number | true/false | ["strings"]`.
// Anything else is an error, so a typo never passes silently.
export type TomlValue = string | number | boolean | string[];
export type TomlDoc = Record<string, Record<string, TomlValue>[] | Record<string, TomlValue>>;

function value(raw: string, line: number): TomlValue {
  const v = raw.trim();
  if (v.startsWith('"')) return JSON.parse(v) as string;
  if (v === "true" || v === "false") return v === "true";
  if (/^-?\d+(\.\d+)?$/.test(v)) return Number(v);
  if (v.startsWith("[")) {
    const items = JSON.parse(v) as unknown[];
    if (!items.every((i) => typeof i === "string"))
      throw new Error(`line ${line}: only string arrays are supported`);
    return items as string[];
  }
  throw new Error(`line ${line}: unsupported value ${v}`);
}

export function parseToml(text: string): TomlDoc {
  const doc: TomlDoc = {};
  let current: Record<string, TomlValue> | null = null;
  text.split("\n").forEach((rawLine, i) => {
    const line = rawLine.replace(/\s+#.*$/, "").trim();
    if (!line || line.startsWith("#")) return;
    const arr = /^\[\[([A-Za-z0-9_.-]+)\]\]$/.exec(line);
    const table = /^\[([A-Za-z0-9_.-]+)\]$/.exec(line);
    if (arr?.[1]) {
      const list = (doc[arr[1]] ??= []) as Record<string, TomlValue>[];
      current = {};
      list.push(current);
    } else if (table?.[1]) {
      current = {};
      doc[table[1]] = current;
    } else {
      const kv = /^([A-Za-z0-9_-]+)\s*=\s*(.+)$/.exec(line);
      if (!kv?.[1] || kv[2] === undefined || !current)
        throw new Error(`line ${i + 1}: can't read "${rawLine}"`);
      current[kv[1]] = value(kv[2], i + 1);
    }
  });
  return doc;
}
