import { useEffect, useRef } from "react";
import { UserRound } from "lucide-react";
import type { PaneRecord } from "@tandem/protocol";
import { Menu, useToast, type MenuEntry } from "@tandem/ui";
import { t } from "../../i18n";
import { accountForNext, accountsFor, setOverride, useAccounts } from "../../stores/accounts";
import { useEngines } from "../../stores/engines";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";

/** UI_SPEC §2.29 AccountChip on an agent pane (F-AC-03): the account this
 * pane runs on. Choosing another sets it for *new* panes of the engine in
 * this workspace; a running session never switches. */
export function AccountChip({ pane }: { pane: PaneRecord }) {
  const list = useAccounts((s) => s.list);
  const overrides = useAccounts((s) => s.overrides);
  const loaded = useAccounts((s) => s.loaded);
  const engine = useEngines((s) => s.label(pane.engineId)) ?? "";
  const workspace = useWorkspaces((s) => s.list.find((w) => w.id === pane.workspaceId));
  const { show } = useToast();
  // The list may predate this pane's account (a default created with the
  // pane): reload once for an account it doesn't know.
  const missing = Boolean(pane.accountId) && loaded && !list.some((a) => a.id === pane.accountId);
  const asked = useRef<string | null>(null);
  useEffect(() => {
    if (!loaded) void useAccounts.getState().load();
    else if (missing && asked.current !== pane.accountId) {
      asked.current = pane.accountId;
      void useAccounts.getState().load();
    }
  }, [loaded, missing, pane.accountId]);

  const current = list.find((a) => a.id === pane.accountId);
  if (pane.kind !== "agent" || !pane.engineId || !current) return null;
  // A worktree's new panes follow its repository's choice.
  const scope = workspace?.parentId ?? pane.workspaceId;
  const scopeName = useWorkspaces.getState().list.find((w) => w.id === scope)?.name ?? "";
  const next = accountForNext(list, overrides, pane.engineId, scope);
  const engineId = pane.engineId;
  const choose = async (accountId: string | null, name: string) => {
    const out = await setOverride(scope, engineId, accountId);
    if (!out.ok) show({ tone: "error", title: out.error.message });
    else show({ title: t("accounts.chipSet", { engine, workspace: scopeName, name }) });
  };
  const hasOverride = overrides.some((o) => o.workspaceId === scope && o.engineId === engineId);
  const entries: MenuEntry[] = [
    { id: "current", type: "label", label: t("accounts.chipCurrent", { name: current.label }) },
    { id: "sep-current", type: "separator" },
    ...accountsFor(list, engineId).map((a): MenuEntry => ({
      id: `use-${a.id}`,
      type: "checkbox",
      label: t("accounts.chipUse", { name: a.label, engine }),
      checked: next?.id === a.id,
      onCheckedChange: () => void choose(a.id, a.label),
    })),
    ...(hasOverride
      ? [
          {
            id: "use-default",
            label: t("accounts.chipUseDefault"),
            onSelect: () => void choose(null, t("accounts.default")),
          },
        ]
      : []),
    { id: "sep-manage", type: "separator" },
    {
      id: "manage",
      label: t("accounts.chipManage"),
      onSelect: () => useUi.getState().openSettings("accounts"),
    },
  ];
  return (
    <Menu
      entries={entries}
      trigger={
        <button
          type="button"
          data-testid="account-chip"
          aria-label={t("accounts.chip", { name: current.label })}
          className="inline-flex h-5 max-w-[140px] shrink-0 items-center gap-1 rounded-full border border-border bg-surface-3 px-1.5 text-xs text-fg-muted hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
        >
          <UserRound size={11} aria-hidden="true" className="shrink-0" />
          <span className="truncate">{current.label}</span>
        </button>
      }
    />
  );
}
