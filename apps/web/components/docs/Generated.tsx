// The /docs content generated from code (F-WB-02), so it can't drift: the
// command registry's shortcuts, the settings schema, the engine registry
// and the release's platforms. MDX pages use these as components.
import registry from "../../../../crates/tandem-engines/registry.json";
import en from "../../../desktop/src/i18n/en.json";
import settings from "@tandem/protocol/settings.json";
import shortcuts from "@tandem/protocol/shortcuts.json";
import { LIMITS, type Plan } from "@tandem/protocol/plans";
import { DOWNLOADS } from "../../lib/release";

const labels = en as Record<string, string>;

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

function ShortcutsTable() {
  return (
    <table>
      <thead>
        <tr>
          <th>Command</th>
          <th>macOS</th>
          <th>Windows and Linux</th>
        </tr>
      </thead>
      <tbody>
        {shortcuts.map((s) => (
          <tr key={s.id} data-command={s.id}>
            <td>{labels[`cmd.${s.id}`] ?? s.id}</td>
            <td>{s.mac ? <kbd>{s.mac}</kbd> : NO_SHORTCUT}</td>
            <td>{s.pc ? <kbd>{s.pc}</kbd> : NO_SHORTCUT}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function SettingsReference() {
  return (
    <>
      {DOCS_SETTINGS_PAGES.map((page) => {
        const rows = settings.filter((s) => s.page === page);
        const label = labels[`settings.${page}`];
        return (
          <section key={page}>
            <h2 id={`settings-${page}`}>{label ?? page[0]?.toUpperCase() + page.slice(1)}</h2>
            {label === undefined && <p>This page isn&apos;t in the app yet.</p>}
            {rows.length > 0 && (
              <table>
                <thead>
                  <tr>
                    <th>Key</th>
                    <th>Default</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((s) => (
                    <tr key={s.key}>
                      <td>
                        <code>{s.key}</code>
                      </td>
                      <td>
                        <code>{JSON.stringify(s.default)}</code>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </section>
        );
      })}
    </>
  );
}

function EnginesTable() {
  const free = LIMITS.free.chatEngines ?? [];
  return (
    <table>
      <thead>
        <tr>
          <th>Engine</th>
          <th>Vendor</th>
          <th>Runs in</th>
        </tr>
      </thead>
      <tbody>
        {registry.map((e) => (
          <tr key={e.id} data-engine={e.id}>
            <td>{e.label}</td>
            <td>{e.vendor}</td>
            <td>
              {e.chatAdapter
                ? `Terminal panes and Thread mode${free.includes(e.id) ? "" : " (threads on Pro)"}`
                : "Terminal panes"}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function PlatformMatrix() {
  return (
    <table>
      <thead>
        <tr>
          <th>Platform</th>
          <th>Requirement</th>
          <th>Build</th>
        </tr>
      </thead>
      <tbody>
        {DOWNLOADS.map((d) => (
          <tr key={d.os} data-platform={d.os}>
            <td>{d.name}</td>
            <td>{d.requirement}</td>
            <td>{d.available ? "Available" : "Not yet"}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

const count = (n: number | null) => (n === null ? "No limit" : String(n));
const engineNames = (ids: readonly string[] | null) =>
  ids === null ? "All" : ids.map((id) => registry.find((e) => e.id === id)?.label ?? id).join(", ");

/** The limits of the features in this build, per plan, from `LIMITS`. */
const LIMIT_ROWS = [
  { key: "panes", label: "Live terminal panes", value: (p: Plan) => count(LIMITS[p].panes) },
  { key: "workspaces", label: "Workspaces", value: (p: Plan) => count(LIMITS[p].workspaces) },
  { key: "threadGrid", label: "Threads side by side", value: (p: Plan) => count(LIMITS[p].threadGrid) },
  { key: "accountsPerEngine", label: "Accounts per engine", value: (p: Plan) => count(LIMITS[p].accountsPerEngine) },
  { key: "chatEngines", label: "Thread mode engines", value: (p: Plan) => engineNames(LIMITS[p].chatEngines) },
] as const;

function PlanLimitsTable() {
  const plans: Plan[] = ["free", "pro", "team"];
  return (
    <table>
      <thead>
        <tr>
          <th>Limit</th>
          <th>Free</th>
          <th>Pro</th>
          <th>Team</th>
        </tr>
      </thead>
      <tbody>
        {LIMIT_ROWS.map((r) => (
          <tr key={r.key} data-limit={r.key}>
            <td>{r.label}</td>
            {plans.map((p) => (
              <td key={p}>{r.value(p)}</td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export const DOCS_COMPONENTS = {
  ShortcutsTable,
  SettingsReference,
  EnginesTable,
  PlatformMatrix,
  PlanLimitsTable,
};
