/** SKILL.md front matter as the editor's fields (UI-74). The core's parser
 * decides validity; this only splits what the user sees. */
export type SkillFields = { name: string; description: string; body: string };

export const SLUG = /^[a-z0-9]+(-[a-z0-9]+)*$/;
export const NAME_MAX = 48;
export const DESCRIPTION_MAX = 280;

function unquote(v: string): string {
  const t = v.trim();
  if (t.startsWith('"') && t.endsWith('"') && t.length >= 2)
    return t.slice(1, -1).replace(/\\"/g, '"').replace(/\\\\/g, "\\");
  if (t.startsWith("'") && t.endsWith("'") && t.length >= 2) return t.slice(1, -1).replace(/''/g, "'");
  return t;
}

/** The fields, or null when the front matter isn't the simple shape the
 * editor writes (the raw editor takes over). */
export function splitSkill(md: string): SkillFields | null {
  const m = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?([\s\S]*)$/.exec(md.replace(/^\uFEFF/, ""));
  if (!m) return null;
  const head = m[1] ?? "";
  const body = m[2] ?? "";
  let name: string | null = null;
  let description: string | null = null;
  for (const line of head.split(/\r?\n/)) {
    const kv = /^([A-Za-z_]+):(.*)$/.exec(line);
    if (!kv) return null;
    const value = (kv[2] ?? "").trim();
    if (value === ">" || value === "|" || value.startsWith(">") || value.startsWith("|")) return null;
    if (kv[1] === "name") name = unquote(value);
    else if (kv[1] === "description") description = unquote(value);
  }
  if (name === null || description === null) return null;
  return { name, description, body };
}

/** A double-quoted YAML scalar when the text needs one. */
function scalar(v: string): string {
  const plain = /^[A-Za-z0-9][^:#"'\n]*$/.test(v) && !/\s$/.test(v);
  return plain ? v : `"${v.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

export function joinSkill(f: SkillFields): string {
  const description = f.description.replace(/\s*\n\s*/g, " ").trim();
  return `---\nname: ${f.name}\ndescription: ${scalar(description)}\n---\n${f.body}`;
}

/** UI-74's checklist: advisory, except secrets (checked by the caller). */
export function checklist(f: SkillFields): Array<{ id: "when" | "steps" | "check" | "dont"; ok: boolean }> {
  const body = f.body;
  return [
    { id: "when", ok: f.description.trim().length > 0 },
    { id: "steps", ok: /^\s*(\d+[.)]|[-*])\s+\S/m.test(body) },
    { id: "check", ok: /\b(check|verify|confirm|test)\b/i.test(body) },
    { id: "dont", ok: /\b(don't|do not|never|avoid)\b/i.test(body) },
  ];
}

export const TEMPLATE_BODY = "# What it's for\n\n## Steps\n1. \n\n## Check the result\n- \n\n## Don't\n- \n";
