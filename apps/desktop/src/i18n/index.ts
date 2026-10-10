import { useSyncExternalStore } from "react";
import { IntlMessageFormat, type PrimitiveType } from "intl-messageformat";
import en from "./en.json";

export type MessageKey = keyof typeof en;
type Catalog = Record<string, string>;

let catalog: Catalog = en;
let locale = "en";
const cache = new Map<string, IntlMessageFormat>();
let version = 0;
const listeners = new Set<() => void>();

/** Switches the active catalog (en-XA pseudo-locale is used by layout tests). */
export function setCatalog(next: Catalog, nextLocale: string): void {
  catalog = next;
  locale = nextLocale;
  cache.clear();
  version += 1;
  for (const l of listeners) l();
}

/** Bumps when the catalog changes; the root keys on it to re-render. */
export function useCatalogVersion(): number {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => version,
  );
}

/** Formats a catalog message with ICU MessageFormat (plurals, selects). */
export function t(key: MessageKey, values?: Record<string, PrimitiveType>): string {
  const source = catalog[key] ?? en[key] ?? key;
  let fmt = cache.get(key);
  if (!fmt) {
    fmt = new IntlMessageFormat(source, locale);
    cache.set(key, fmt);
  }
  const out = fmt.format(values);
  return typeof out === "string" ? out : String(out);
}
