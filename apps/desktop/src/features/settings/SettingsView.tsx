import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronLeft, Search } from "lucide-react";
import { Callout, Input, PageHeader, SettingsGroup, SettingsRow, cx } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useSettings } from "../../stores/settings";
import { useSession } from "../../stores/session";
import { useUi } from "../../stores/ui";
import { useBoot } from "../shell/useBoot";
import { matchPage, matchRow, settingsPages } from "./pages";

/** UI-100 Settings shell (F-SH-04): the rail becomes the nav, the filter
 * narrows pages and highlights rows, Esc returns to the previous mode. */
export function SettingsView() {
  const platform = useBoot((s) => s.boot?.platform ?? "linux");
  const routines = useSession((s) => !!s.entitlement?.limits.routines);
  const accounts = useSession((s) => !!s.auth?.hosted);
  const pages = useMemo(() => settingsPages(platform, routines, accounts), [platform, routines, accounts]);
  const closeSettings = useUi((s) => s.closeSettings);
  const requested = useUi((s) => s.settingsPage);
  const lastPage = useSettings((s) => s.values.settings_last_page as string | undefined);
  const setSetting = useSettings((s) => s.set);
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(() => requested ?? lastPage ?? "general");
  const [highlight, setHighlight] = useState<string | null>(null);
  const filter = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (requested) setPage(requested);
  }, [requested]);

  useEffect(() => {
    void setSetting("settings_last_page", page);
    invoke("telemetry_capture", { event: "settings_opened", props: { page } }).catch(() => undefined);
  }, [page, setSetting]);

  const visible = pages.filter((p) => matchPage(p, query));
  useEffect(() => {
    if (!query.trim()) {
      setHighlight(null);
      return;
    }
    const first = visible[0];
    if (!first) return;
    if (!visible.some((p) => p.id === page)) setPage(first.id);
    const row = (visible.find((p) => p.id === page) ?? first).rows.find((r) => matchRow(r, query));
    setHighlight(row?.id ?? null);
    const timer = setTimeout(() => setHighlight(null), 2000);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query]);

  // Esc outside a field closes Settings (F-SH-04 AC1).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      const el = e.target instanceof HTMLElement ? e.target : null;
      if (el?.closest("[role=dialog],[role=alertdialog],[role=menu],[role=listbox]")) return;
      if (el && ["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName) && el !== filter.current) return;
      e.preventDefault();
      closeSettings();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [closeSettings]);

  useEffect(() => {
    filter.current?.focus();
  }, []);

  const current = pages.find((p) => p.id === page) ?? pages[0];
  return (
    <>
      <nav
        aria-label={t("settings.nav")}
        className="flex w-[var(--rail-w)] shrink-0 flex-col gap-2 border-r border-border bg-surface-1 p-2"
      >
        <button
          type="button"
          onClick={closeSettings}
          className="inline-flex h-7 items-center gap-1 self-start rounded-sm px-1.5 text-sm text-fg-muted hover:bg-surface-4 hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
        >
          <ChevronLeft size={14} aria-hidden="true" />
          {t("settings.back")}
        </button>
        <Input
          ref={filter}
          label={t("settings.filter")}
          hideLabel
          placeholder={t("settings.filter")}
          prefix={<Search size={14} />}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          clearable
          onClear={() => setQuery("")}
        />
        <ul className="flex flex-col gap-0.5">
          {visible.map((p) => (
            <li key={p.id}>
              <button
                type="button"
                aria-current={p.id === current?.id ? "page" : undefined}
                onClick={() => setPage(p.id)}
                className={cx(
                  "flex h-[30px] w-full items-center rounded-sm px-2 text-left text-base text-fg hover:bg-surface-4 focus-visible:shadow-focus focus-visible:outline-none",
                  p.id === current?.id && "bg-accent-soft hover:bg-accent-soft",
                )}
              >
                {p.label}
              </button>
            </li>
          ))}
        </ul>
        {visible.length === 0 && (
          <p className="px-2 text-sm text-fg-subtle">{t("settings.noMatch", { q: query })}</p>
        )}
      </nav>
      <main id="main" tabIndex={-1} className="min-w-0 flex-1 overflow-y-auto bg-bg focus:outline-none">
        {current && (
          <div
            className="mx-auto flex max-w-[720px] flex-col gap-6 px-6 py-6"
            data-testid={`settings-page-${current.id}`}
          >
            <PageHeader title={current.label} subtitle={current.subtitle} />
            {current.callout && <Callout>{current.callout}</Callout>}
            {current.header}
            {current.rows.length > 0 && (
              <SettingsGroup>
                {current.rows.map((row) => (
                  <SettingsRow
                    key={row.id}
                    settingKey={row.id}
                    label={row.label}
                    description={row.description}
                    highlighted={highlight === row.id}
                    control={row.control}
                  />
                ))}
              </SettingsGroup>
            )}
            {current.extra}
          </div>
        )}
      </main>
    </>
  );
}
