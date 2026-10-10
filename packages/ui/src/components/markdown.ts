/** A small GFM subset for agent messages (UI_SPEC §2 MarkdownView): headings,
 * paragraphs, fenced code, block quotes, lists with task items, tables,
 * rules; inline code, strong, emphasis, strikethrough, links and autolinks.
 * The result is data rendered as React elements: raw HTML is never
 * interpreted, so it shows as text. Pure, so it can run in a worker. */

export type Inline =
  | { t: "text"; v: string }
  | { t: "code"; v: string }
  | { t: "strong"; c: Inline[] }
  | { t: "em"; c: Inline[] }
  | { t: "del"; c: Inline[] }
  | { t: "link"; href: string; c: Inline[] }
  | { t: "br" };

export type ListItem = { checked: boolean | null; blocks: Block[] };

export type Block =
  | { t: "heading"; level: 1 | 2 | 3 | 4 | 5 | 6; c: Inline[] }
  | { t: "para"; c: Inline[] }
  | { t: "code"; lang: string; v: string }
  | { t: "quote"; blocks: Block[] }
  | { t: "list"; ordered: boolean; start: number; items: ListItem[] }
  | { t: "table"; align: Array<"left" | "center" | "right" | null>; head: Inline[][]; rows: Inline[][][] }
  | { t: "hr" };

/** Above this, a message renders as plain text (UI_SPEC: oversized input
 * degrades). */
export const MARKDOWN_MAX = 200_000;

const FENCE = /^ {0,3}(`{3,}|~{3,})\s*([\w#+.-]*)/;
const HEADING = /^ {0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;
const HR = /^ {0,3}([-*_])(\s*\1){2,}\s*$/;
const BULLET = /^( {0,3})([-*+])\s+(.*)$/;
const ORDERED = /^( {0,3})(\d{1,9})[.)]\s+(.*)$/;
const TASK = /^\[([ xX])\]\s+(.*)$/;
const TABLE_SEP = /^\s*\|?\s*:?-{1,}:?\s*(\|\s*:?-{1,}:?\s*)*\|?\s*$/;

function cells(row: string): string[] {
  let s = row.trim();
  if (s.startsWith("|")) s = s.slice(1);
  if (s.endsWith("|") && !s.endsWith("\\|")) s = s.slice(0, -1);
  const out: string[] = [];
  let cur = "";
  for (let i = 0; i < s.length; i++) {
    const ch = s[i];
    if (ch === "\\" && s[i + 1] === "|") {
      cur += "|";
      i++;
    } else if (ch === "|") {
      out.push(cur.trim());
      cur = "";
    } else cur += ch;
  }
  out.push(cur.trim());
  return out;
}

/** Only these schemes become links; anything else stays text. */
export function safeHref(href: string): string | null {
  const h = href.trim();
  if (/^(https?:|mailto:)/i.test(h)) return h;
  return null;
}

export function parseInline(src: string): Inline[] {
  const out: Inline[] = [];
  let text = "";
  const flush = () => {
    if (text) out.push({ t: "text", v: text });
    text = "";
  };
  let i = 0;
  while (i < src.length) {
    const ch = src[i] ?? "";
    const rest = src.slice(i);
    if (ch === "\\" && i + 1 < src.length && /[\\`*_{}[\]()#+\-.!|~>]/.test(src[i + 1] ?? "")) {
      text += src[i + 1];
      i += 2;
      continue;
    }
    if (ch === "`") {
      const run = /^`+/.exec(rest)?.[0] ?? "`";
      const end = src.indexOf(run, i + run.length);
      if (end > 0) {
        flush();
        out.push({ t: "code", v: src.slice(i + run.length, end).replace(/^ (.*) $/, "$1") });
        i = end + run.length;
        continue;
      }
    }
    if (rest.startsWith("  \n") || rest.startsWith("\\\n")) {
      flush();
      out.push({ t: "br" });
      i += rest.startsWith("\\\n") ? 2 : 3;
      continue;
    }
    const delim = /^(\*\*|__|~~|\*|_)/.exec(rest)?.[0];
    if (delim) {
      const close = src.indexOf(delim, i + delim.length);
      const inner = close > 0 ? src.slice(i + delim.length, close) : "";
      // `snake_case` stays text: `_` only opens at a word boundary.
      const boundary = delim[0] !== "_" || i === 0 || !/\w/.test(src[i - 1] ?? "");
      if (close > 0 && inner.trim() === inner && inner.length > 0 && boundary) {
        flush();
        const c = parseInline(inner);
        out.push(delim === "~~" ? { t: "del", c } : delim.length === 2 ? { t: "strong", c } : { t: "em", c });
        i = close + delim.length;
        continue;
      }
    }
    if (ch === "[") {
      const m = /^\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/.exec(rest);
      if (m) {
        const href = safeHref(m[2] ?? "");
        flush();
        if (href) out.push({ t: "link", href, c: parseInline(m[1] ?? "") });
        else out.push(...parseInline(m[1] ?? ""));
        i += m[0].length;
        continue;
      }
    }
    const auto = /^(https?:\/\/[^\s<>()]+[^\s<>().,;:!?'"])/.exec(rest);
    if (auto && (i === 0 || /[\s(]/.test(src[i - 1] ?? ""))) {
      flush();
      const href = auto[1] ?? "";
      out.push({ t: "link", href, c: [{ t: "text", v: href }] });
      i += href.length;
      continue;
    }
    text += ch;
    i++;
  }
  flush();
  return out;
}

function paragraph(lines: string[]): Block {
  return { t: "para", c: parseInline(lines.map((l) => l.replace(/^ {0,3}/, "")).join("\n")) };
}

export function parseMarkdown(src: string): Block[] {
  const lines = src.replace(/\r\n?/g, "\n").split("\n");
  return parseLines(lines);
}

/** Where to cut `text` into chunks: the first about `first` long, the rest
 * about `step`, each ending at a line end when there is one. */
export function lineChunks(text: string, first: number, step: number): number[] {
  const ends: number[] = [];
  let at = 0;
  let size = first;
  while (at < text.length) {
    let end = Math.min(text.length, at + size);
    if (end < text.length) {
      const nl = text.lastIndexOf("\n", end - 1);
      if (nl >= at) end = nl + 1;
    }
    ends.push(end);
    at = end;
    size = step;
  }
  return ends;
}

const CACHE_ENTRIES = 256;
const CACHE_MAX_CHARS = 64 * 1024;
const cache = new Map<string, Block[]>();

/** `parseMarkdown` for settled text, remembering the last few hundred
 * messages (each under 64 KB), so a row scrolled back into view doesn't
 * parse again. The blocks are shared: never mutate them. */
export function parseMarkdownCached(src: string): Block[] {
  if (src.length > CACHE_MAX_CHARS) return parseMarkdown(src);
  const hit = cache.get(src);
  if (hit) {
    // Most recently used goes last.
    cache.delete(src);
    cache.set(src, hit);
    return hit;
  }
  const parsed = parseMarkdown(src);
  cache.set(src, parsed);
  if (cache.size > CACHE_ENTRIES) {
    const oldest = cache.keys().next();
    if (!oldest.done) cache.delete(oldest.value);
  }
  return parsed;
}

function parseLines(lines: string[]): Block[] {
  const blocks: Block[] = [];
  let para: string[] = [];
  const endPara = () => {
    if (para.length) blocks.push(paragraph(para));
    para = [];
  };
  let i = 0;
  while (i < lines.length) {
    const line = lines[i] ?? "";
    if (!line.trim()) {
      endPara();
      i++;
      continue;
    }
    const fence = FENCE.exec(line);
    if (fence) {
      endPara();
      const marker = fence[1] ?? "```";
      const body: string[] = [];
      i++;
      while (i < lines.length && !(lines[i] ?? "").trimStart().startsWith(marker)) {
        body.push(lines[i] ?? "");
        i++;
      }
      i++; // the closing fence (or the end: an unclosed fence runs to the end)
      blocks.push({ t: "code", lang: fence[2] ?? "", v: body.join("\n") });
      continue;
    }
    const heading = HEADING.exec(line);
    if (heading) {
      endPara();
      blocks.push({
        t: "heading",
        level: Math.min(6, (heading[1] ?? "#").length) as 1 | 2 | 3 | 4 | 5 | 6,
        c: parseInline(heading[2] ?? ""),
      });
      i++;
      continue;
    }
    if (HR.test(line) && !para.length) {
      blocks.push({ t: "hr" });
      i++;
      continue;
    }
    if (/^ {0,3}>/.test(line)) {
      endPara();
      const inner: string[] = [];
      while (i < lines.length && /^ {0,3}>/.test(lines[i] ?? "")) {
        inner.push((lines[i] ?? "").replace(/^ {0,3}> ?/, ""));
        i++;
      }
      blocks.push({ t: "quote", blocks: parseLines(inner) });
      continue;
    }
    const next = lines[i + 1] ?? "";
    if (line.includes("|") && TABLE_SEP.test(next) && cells(next).length === cells(line).length) {
      endPara();
      const head = cells(line).map(parseInline);
      const align = cells(next).map((c) =>
        c.startsWith(":") && c.endsWith(":") ? "center" : c.endsWith(":") ? "right" : c.startsWith(":") ? "left" : null,
      );
      i += 2;
      const rows: Inline[][][] = [];
      while (i < lines.length && (lines[i] ?? "").includes("|") && (lines[i] ?? "").trim()) {
        const r = cells(lines[i] ?? "");
        rows.push(head.map((_, n) => parseInline(r[n] ?? "")));
        i++;
      }
      blocks.push({ t: "table", align, head, rows });
      continue;
    }
    const bullet = BULLET.exec(line);
    const ordered = ORDERED.exec(line);
    if (bullet || ordered) {
      endPara();
      const isOrdered = !bullet;
      const items: ListItem[] = [];
      const start = ordered ? Number(ordered[2]) : 1;
      while (i < lines.length) {
        const l = lines[i] ?? "";
        const m = isOrdered ? ORDERED.exec(l) : BULLET.exec(l);
        if (!m) break;
        const indent = (m[1] ?? "").length + (isOrdered ? (m[2] ?? "").length + 2 : 2);
        const body: string[] = [m[3] ?? ""];
        i++;
        // Continuation and nested lines: indented, or blank followed by indented.
        while (i < lines.length) {
          const c = lines[i] ?? "";
          if (c.trim() === "") {
            const after = lines[i + 1] ?? "";
            if (after.startsWith(" ".repeat(indent)) && after.trim()) {
              body.push("");
              i++;
              continue;
            }
            break;
          }
          if (c.startsWith(" ".repeat(Math.min(indent, 2))) && !(isOrdered ? ORDERED : BULLET).exec(c)?.[1]?.length) {
            if (BULLET.exec(c) && !c.startsWith("  ")) break;
            body.push(c.replace(new RegExp(`^ {0,${indent}}`), ""));
            i++;
            continue;
          }
          if (/^\s{2,}/.test(c)) {
            body.push(c.replace(new RegExp(`^ {0,${indent}}`), ""));
            i++;
            continue;
          }
          break;
        }
        const task = TASK.exec(body[0] ?? "");
        let checked: boolean | null = null;
        if (task) {
          checked = (task[1] ?? " ").toLowerCase() === "x";
          body[0] = task[2] ?? "";
        }
        items.push({ checked, blocks: parseLines(body) });
      }
      blocks.push({ t: "list", ordered: isOrdered, start, items });
      continue;
    }
    para.push(line);
    i++;
  }
  endPara();
  return blocks;
}

/** Splits a streaming message so only its last block re-parses: the stable
 * prefix (complete blocks) and the tail (UI_SPEC streaming budget). */
export function splitStable(src: string): { stable: string; tail: string } {
  // The last blank line outside a code fence ends the stable part.
  let inFence: string | null = null;
  let cut = 0;
  let pos = 0;
  for (const line of src.split("\n")) {
    const f = FENCE.exec(line);
    if (f) {
      const marker = f[1] ?? "```";
      if (!inFence) inFence = marker;
      else if (line.trimStart().startsWith(inFence)) inFence = null;
    }
    pos += line.length + 1;
    if (!inFence && line.trim() === "") cut = pos;
  }
  return { stable: src.slice(0, cut), tail: src.slice(cut) };
}
