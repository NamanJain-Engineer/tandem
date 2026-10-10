// The keyboard shortcuts cheatsheet (UI_SPEC §5, UI-113): every shortcut in
// effect, rebound ones included, in §5's groups. Mod+/ toggles it.
import { useState } from "react";
import { Search } from "lucide-react";
import { Button, Dialog, Input, Kbd } from "@tandem/ui";
import { t } from "../../i18n";
import { useSettings } from "../../stores/settings";
import { useUi } from "../../stores/ui";
import { useBoot } from "../shell/useBoot";
import type { ShortcutOverrides } from "./commands";
import { keysOf } from "./keys";
import { groupRows, keysLabel, shortcutRows } from "./shortcutRows";

const NONE: ShortcutOverrides = {};

export function Cheatsheet() {
  const open = useUi((s) => s.cheatsheetOpen);
  const mac = useBoot((s) => s.boot?.platform === "macos");
  const overrides = useSettings((s) => (s.values.shortcut_overrides as ShortcutOverrides | undefined) ?? NONE);
  const terminalFirst = useSettings((s) => (s.values.term_shortcut_priority ?? "terminal") === "terminal");
  const [query, setQuery] = useState("");
  const close = (o: boolean) => {
    useUi.getState().setCheatsheet(o);
    if (!o) setQuery("");
  };
  const groups = groupRows(
    shortcutRows(mac, overrides).filter((r) => r.keys !== null),
    query,
    mac,
  );
  return (
    <Dialog
      open={open}
      onOpenChange={close}
      title={t("shortcuts.title")}
      size="lg"
      closeLabel={t("common.close")}
      footer={
        <Button
          variant="secondary"
          onClick={() => {
            close(false);
            useUi.getState().openSettings("shortcuts");
          }}
        >
          {t("shortcuts.change")}
        </Button>
      }
    >
      <div className="flex min-h-0 flex-col gap-4">
        <Input
          type="search"
          label={t("shortcuts.search")}
          hideLabel
          placeholder={t("shortcuts.search")}
          prefix={<Search size={14} aria-hidden="true" />}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        {groups.length === 0 && <p className="py-4 text-center text-base text-fg-muted">{t("shortcuts.noMatch", { query: query.trim() })}</p>}
        {groups.map((g) => (
          <section key={g.id} aria-label={t(g.label)} className="flex flex-col gap-1">
            <h3 className="text-2xs font-semibold uppercase tracking-caps text-fg-subtle">{t(g.label)}</h3>
            <ul className="flex flex-col">
              {g.rows.map((r) => (
                <li key={r.id} data-testid="cheatsheet-row" data-id={r.id} className="flex items-baseline gap-3 border-b border-divider py-1.5 last:border-b-0">
                  <span className="min-w-0 flex-1 text-base text-fg">{r.title}</span>
                  <span className="flex shrink-0 flex-col items-end gap-0.5">
                    {r.keys && <Kbd keys={keysOf(r.keys)} />}
                    {!mac && terminalFirst && r.terminal && r.terminal !== r.keys && (
                      <span className="text-xs text-fg-subtle">{t("shortcuts.inTerminal", { keys: keysLabel(r.terminal, mac) })}</span>
                    )}
                  </span>
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
    </Dialog>
  );
}
