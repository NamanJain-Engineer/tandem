// The animated product scenes (F-WB-01 AC9, D-1880): their words. Every
// label the app itself would draw comes from its catalog by key, so a scene
// says exactly what the app says; what's left is sample work (branches,
// files, terminal lines), which the captions call simplified.
import en from "../../desktop/src/i18n/en.json";
import { RELEASE } from "./release";

const CATALOG = en as Record<string, string>;

/** True when `version` is `min` or later (x.y.z). */
export function atLeast(version: string, min: string): boolean {
  const a = version.split(".").map(Number);
  const b = min.split(".").map(Number);
  for (let i = 0; i < 3; i++) if ((a[i] ?? 0) !== (b[i] ?? 0)) return (a[i] ?? 0) > (b[i] ?? 0);
  return true;
}

/** The release the scenes show, and whether it has 0.9.0's worktree landing,
 * pull request checks and intent radar (F-WT-07, F-WT-08, F-WT-10). Before
 * that release ships, the scenes leave them out (D-128). */
export const SCENE_BUILD = RELEASE.version;
export const LANDS = atLeast(SCENE_BUILD, "0.9.0");

/** One of the app's strings, with its `{name}` values and simple plurals
 * (`{n, plural, one {# file} other {# files}}`) filled in. Throws on a key
 * the catalog doesn't have, so a renamed string fails the build. */
export function t(key: string, vars: Record<string, string | number> = {}): string {
  const raw = CATALOG[key];
  if (raw === undefined) throw new Error(`scenes: no "${key}" in the app's catalog`);
  return format(raw, vars);
}

function format(s: string, vars: Record<string, string | number>): string {
  let out = "";
  for (let i = 0; i < s.length; i++) {
    if (s[i] !== "{") {
      out += s[i];
      continue;
    }
    // Find the matching brace.
    let depth = 0;
    let j = i;
    for (; j < s.length; j++) {
      if (s[j] === "{") depth++;
      else if (s[j] === "}" && --depth === 0) break;
    }
    const body = s.slice(i + 1, j);
    const [name, kind] = body.split(",").map((x) => x.trim());
    if (kind === "plural") {
      const n = Number(vars[name ?? ""] ?? 0);
      const forms = pluralForms(body.slice(body.indexOf("plural") + "plural".length));
      const form = forms[`=${n}`] ?? (n === 1 ? forms.one : undefined) ?? forms.other ?? "";
      out += format(form.replace(/#/g, String(n)), vars);
    } else if (kind === "number") out += String(vars[name ?? ""] ?? "");
    else out += String(vars[name ?? ""] ?? `{${name}}`);
    i = j;
  }
  return out;
}

/** A plural's forms (`one {# file} other {# files in {base}}`), braces
 * inside a form included. */
function pluralForms(rest: string): Record<string, string> {
  const forms: Record<string, string> = {};
  const re = /(=\d+|zero|one|two|few|many|other)\s*\{/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(rest))) {
    let depth = 1;
    let j = re.lastIndex;
    for (; j < rest.length && depth > 0; j++) {
      if (rest[j] === "{") depth++;
      else if (rest[j] === "}") depth--;
    }
    forms[m[1] as string] = rest.slice(re.lastIndex, j - 1);
    re.lastIndex = j;
  }
  return forms;
}

/** The sample work the scenes show: one repository and its worktrees. */
export const SAMPLE = {
  workspace: "shop",
  base: "main",
  worktrees: {
    claude: { engine: "Claude Code", branch: "checkout-fix" },
    codex: { engine: "Codex", branch: "coupon-rules" },
    gemini: { engine: "Gemini CLI", branch: "search-ui" },
  },
  symbol: "applyCoupon",
  /** What each agent's terminal prints, in order (illustrative). */
  lines: {
    claude: [
      "› Find why checkout sends people back to /login",
      "  reading src/auth/session.ts",
      "  reading src/cart/checkout.ts",
      "  editing src/cart/checkout.ts  +18 −4",
      "  editing src/cart/coupon.ts  +9 −2",
      "  $ pnpm test cart",
      "  Tests  42 passed",
    ],
    codex: [
      "› Stack coupons by priority",
      "  reading src/cart/coupon.ts",
      "  editing src/cart/coupon.ts  +31 −12",
      "  editing src/cart/rules.ts  +40",
      "  $ pnpm test coupons",
    ],
    gemini: ["› Make search results keyboard friendly", "  reading src/search/results.tsx", "  editing src/search/results.tsx  +22 −6"],
    shell: ["$ pnpm dev", "  ready on localhost:41003", "  GET /checkout 200", "  GET /cart 200"],
  },
  ask: "pnpm test search",
  diff: { files: 4, adds: 62, dels: 18 },
  comments: [
    { line: 12, file: "src/cart/checkout.ts", text: "This returns before the session is refreshed; a second tab still loops." },
    { line: 27, file: "src/cart/checkout.ts", text: "Add a test for an expired cart." },
  ],
  commits: ["a41c9e2", "7d03b5f", "c18e6a0"],
  failedCheck: "test",
} as const;
