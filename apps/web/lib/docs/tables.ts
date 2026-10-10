// The /docs content generated from code (F-WB-02), so it can't drift: the
// command registry's shortcuts, the settings schema, the engine registry,
// the release's platforms and the plan limits. The docs pages render these
// tables as HTML (components/docs/Generated.tsx) and /llms-full.txt as
// Markdown (F-WB-04), from the same rows.
import registry from "../../../../crates/tandem-engines/registry.json";
import en from "../../../desktop/src/i18n/en.json";
import settings from "@tandem/protocol/settings.json";
import shortcuts from "@tandem/protocol/shortcuts.json";
import { LIMITS, type Plan } from "@tandem/protocol/plans";
import { DOWNLOADS } from "../release";

const labels = en as Record<string, string>;

export type Cell = { text: string; as?: "kbd" | "code" };
export type Row = { key: string; cells: Cell[] };
/** `attr` names the row's data attribute (`data-<attr>="<key>"`). */
export type Table = { attr: string; head: string[]; rows: Row[] };

const text = (t: string): Cell => ({ text: t });

/** Every Settings page the docs anchor (the app's pages, F-SH-04). */
export const DOCS_SETTINGS_PAGES = [
  "general",
  "appearance",
  "notifications",
  "terminal",
  "engines",
  "accounts",
  "teammates",
  "dictate",
  "conductor",
  "plugins",
  "remote",
  "privacy",
  "shortcuts",
  "account",
  "updates",
  "advanced",
];

/** What a command without a default shortcut shows. */
export const NO_SHORTCUT = "None";

const keys = (k: string | null): Cell => (k ? { text: k, as: "kbd" } : text(NO_SHORTCUT));

export function shortcutsTable(): Table {
  return {
    attr: "command",
    head: ["Command", "macOS", "Windows and Linux"],
    rows: shortcuts.map((s) => ({
      key: s.id,
      cells: [text(labels[`cmd.${s.id}`] ?? s.id), keys(s.mac), keys(s.pc)],
    })),
  };
}

export type SettingsSection = { page: string; title: string; inApp: boolean; table: Table };

export function settingsSections(): SettingsSection[] {
  return DOCS_SETTINGS_PAGES.map((page) => {
    const label = labels[`settings.${page}`];
    return {
      page,
      title: label ?? page[0]?.toUpperCase() + page.slice(1),
      inApp: label !== undefined,
      table: {
        attr: "setting",
        head: ["Key", "Default"],
        rows: settings
          .filter((s) => s.page === page)
          .map((s) => ({
            key: s.key,
            cells: [
              { text: s.key, as: "code" },
              { text: JSON.stringify(s.default), as: "code" },
            ],
          })),
      },
    };
  });
}

export function enginesTable(): Table {
  const free = LIMITS.free.chatEngines ?? [];
  return {
    attr: "engine",
    head: ["Engine", "Vendor", "Runs in"],
    rows: registry.map((e) => ({
      key: e.id,
      cells: [
        text(e.label),
        text(e.vendor),
        text(
          e.chatAdapter
            ? `Terminal panes and Thread mode${free.includes(e.id) ? "" : " (threads on Pro)"}`
            : "Terminal panes",
        ),
      ],
    })),
  };
}

export function platformTable(): Table {
  return {
    attr: "platform",
    head: ["Platform", "Requirement", "Build"],
    rows: DOWNLOADS.map((d) => ({
      key: d.os,
      cells: [text(d.name), text(d.requirement), text(d.available ? "Available" : "Not yet")],
    })),
  };
}

const count = (n: number | null) => (n === null ? "No limit" : String(n));
const engineNames = (ids: readonly string[] | null) =>
  ids === null ? "All" : ids.map((id) => registry.find((e) => e.id === id)?.label ?? id).join(", ");

/** The limits of the features in this build, per plan, from `LIMITS`. */
const LIMIT_ROWS = [
  { key: "panes", label: "Live terminal panes", value: (p: Plan) => count(LIMITS[p].panes) },
  { key: "workspaces", label: "Workspaces", value: (p: Plan) => count(LIMITS[p].workspaces) },
  { key: "threadGrid", label: "Threads side by side", value: (p: Plan) => count(LIMITS[p].threadGrid) },
  {
    key: "accountsPerEngine",
    label: "Accounts per engine",
    value: (p: Plan) => count(LIMITS[p].accountsPerEngine),
  },
  {
    key: "chatEngines",
    label: "Thread mode engines",
    value: (p: Plan) => engineNames(LIMITS[p].chatEngines),
  },
] as const;

export function planLimitsTable(): Table {
  const plans: Plan[] = ["free", "pro", "team"];
  return {
    attr: "limit",
    head: ["Limit", "Free", "Pro", "Team"],
    rows: LIMIT_ROWS.map((r) => ({
      key: r.key,
      cells: [text(r.label), ...plans.map((p) => text(r.value(p)))],
    })),
  };
}

const mdCell = (c: Cell) => {
  const t = c.text.replace(/\|/g, "\\|");
  return c.as ? `\`${t}\`` : t;
};

export function tableMarkdown(t: Table): string {
  const line = (cells: string[]) => `| ${cells.join(" | ")} |`;
  return [line(t.head), line(t.head.map(() => "---")), ...t.rows.map((r) => line(r.cells.map(mdCell)))].join(
    "\n",
  );
}

/** The Markdown each docs component stands for, by its MDX tag. */
export const DOCS_MARKDOWN: Record<string, () => string> = {
  ShortcutsTable: () => tableMarkdown(shortcutsTable()),
  SettingsReference: () =>
    settingsSections()
      .map(
        (s) =>
          `## ${s.title}\n\n` +
          (s.inApp ? "" : "This page isn't in the app yet.\n\n") +
          (s.table.rows.length ? tableMarkdown(s.table) : ""),
      )
      .join("\n\n"),
  EnginesTable: () => tableMarkdown(enginesTable()),
  PlatformMatrix: () => tableMarkdown(platformTable()),
  PlanLimitsTable: () => tableMarkdown(planLimitsTable()),
};
