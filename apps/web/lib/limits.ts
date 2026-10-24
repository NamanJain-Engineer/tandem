import registry from "../../../crates/tandem-engines/registry.json";
import { LIMITS } from "@tandem/protocol/plans";

/** The limits this release's download still keeps from its earlier Free plan,
 * for the docs' limits table. Vibezeo is free, with no paid plans (D-869): the
 * next release removes these, and this file with them. Values come from
 * `LIMITS.free`, the definitions the app enforces. */
export type LimitRow = { key: string; label: string; value: string };

const count = (n: number | null): string => (n === null ? "No limit" : n.toLocaleString("en-US"));
const engineNames = (ids: readonly string[] | null): string =>
  ids === null ? "All" : ids.map((id) => registry.find((e) => e.id === id)?.label ?? id).join(", ");

const free = LIMITS.free;

export const LIMIT_ROWS: readonly LimitRow[] = [
  { key: "panes", label: "Live terminal panes", value: count(free.panes) },
  { key: "workspaces", label: "Workspaces", value: count(free.workspaces) },
  { key: "threadGrid", label: "Threads side by side", value: count(free.threadGrid) },
  { key: "accountsPerEngine", label: "Accounts per engine", value: count(free.accountsPerEngine) },
  { key: "chatEngines", label: "Thread mode engines", value: engineNames(free.chatEngines) },
];
