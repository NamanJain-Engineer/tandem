// The /docs content generated from code (F-WB-02), so it can't drift: the
// command registry's shortcuts, the settings schema, the engine registry,
// the release's platforms and the plan limits. The docs pages render these
// tables as HTML (components/docs/Generated.tsx) and /llms-full.txt as
// Markdown (F-WB-04), from the same rows.
import registry from "../../../../crates/tandem-engines/registry.json";
import voiceTools from "../../../api/voice-tools.json";
import en from "../../../desktop/src/i18n/en.json";
import settings from "@tandem/protocol/settings.json";
import shortcuts from "@tandem/protocol/shortcuts.json";
import { LIMITS } from "@tandem/protocol/plans";
import { LIMIT_ROWS } from "../limits";
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
  "checkpoints",
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
            ? `Terminal panes and Thread mode${free.includes(e.id) ? "" : " (threads in the next release)"}`
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

/** The docs' limits table: what this release's download still limits (D-869). */
export function planLimitsTable(): Table {
  return {
    attr: "limit",
    head: ["Limit", "This release"],
    rows: LIMIT_ROWS.map((r) => ({ key: r.key, cells: [text(r.label), text(r.value)] })),
  };
}

const mdCell = (c: Cell) => {
  const t = c.text.replace(/\|/g, "\\|");
  return c.as ? `\`${t}\`` : t;
};

/** Conductor's tool groups, as PRODUCT_SPEC F-CN-02's table names them. */
export const CONDUCTOR_GROUPS: Record<string, string> = {
  navigation: "Navigation",
  awareness: "Awareness",
  panes: "Panes and agents",
  threads: "Threads",
  teammates: "Teammates",
  tasks: "Tasks",
  find: "Find",
  app: "App",
  review: "Review",
};

/** Conductor's closed tool catalog (`apps/api/voice-tools.json`, the file the
 * API's doer and the app's executor both check calls against), by group. */
export function conductorToolsTable(): Table {
  const order = Object.keys(CONDUCTOR_GROUPS);
  const tools = [...voiceTools.tools].sort((a, b) => order.indexOf(a.group) - order.indexOf(b.group));
  return {
    attr: "tool",
    head: ["Group", "Tool", "What it does", "Asks first"],
    rows: tools.map((t) => ({
      key: t.name,
      cells: [
        text(CONDUCTOR_GROUPS[t.group] ?? t.group),
        { text: t.name, as: "code" },
        text(t.description),
        text(t.confirm ? "Yes" : "No"),
      ],
    })),
  };
}

/** The only settings Conductor's `set_setting` may change (F-CN-02 AC2). */
export function conductorSettingsTable(): Table {
  return {
    attr: "setting",
    head: ["Key", "Settings page"],
    rows: voiceTools.settable_settings.map((key) => {
      const page = settings.find((s) => s.key === key)?.page ?? "";
      return {
        key,
        cells: [{ text: key, as: "code" }, text(labels[`settings.${page}`] ?? page)],
      };
    }),
  };
}

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
  ConductorToolsTable: () => tableMarkdown(conductorToolsTable()),
  ConductorSettingsTable: () => tableMarkdown(conductorSettingsTable()),
};
