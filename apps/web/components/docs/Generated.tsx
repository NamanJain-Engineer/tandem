// The docs pages' generated tables (F-WB-02) as HTML; the rows come from
// lib/docs/tables.ts, which /llms-full.txt renders as Markdown.
import {
  enginesTable,
  planLimitsTable,
  platformTable,
  settingsSections,
  shortcutsTable,
  type Cell,
  type Table,
} from "../../lib/docs/tables";

export { DOCS_SETTINGS_PAGES, NO_SHORTCUT } from "../../lib/docs/tables";

function CellView({ cell }: { cell: Cell }) {
  if (cell.as === "kbd") return <kbd>{cell.text}</kbd>;
  if (cell.as === "code") return <code>{cell.text}</code>;
  return <>{cell.text}</>;
}

function TableView({ table }: { table: Table }) {
  return (
    <table>
      <thead>
        <tr>
          {table.head.map((h) => (
            <th key={h}>{h}</th>
          ))}
        </tr>
      </thead>
      <tbody>
        {table.rows.map((r) => (
          <tr key={r.key} {...{ [`data-${table.attr}`]: r.key }}>
            {r.cells.map((c, i) => (
              <td key={i}>
                <CellView cell={c} />
              </td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function SettingsReference() {
  return (
    <>
      {settingsSections().map((s) => (
        <section key={s.page}>
          <h2 id={`settings-${s.page}`}>{s.title}</h2>
          {!s.inApp && <p>This page isn&apos;t in the app yet.</p>}
          {s.table.rows.length > 0 && <TableView table={s.table} />}
        </section>
      ))}
    </>
  );
}

export const DOCS_COMPONENTS = {
  ShortcutsTable: () => <TableView table={shortcutsTable()} />,
  SettingsReference,
  EnginesTable: () => <TableView table={enginesTable()} />,
  PlatformMatrix: () => <TableView table={platformTable()} />,
  PlanLimitsTable: () => <TableView table={planLimitsTable()} />,
};
