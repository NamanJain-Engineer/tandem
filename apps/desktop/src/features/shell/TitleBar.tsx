import { useState } from "react";
import { ChevronDown, Command as CommandIcon, PanelLeft, PanelRight, Search } from "lucide-react";
import {
  Avatar,
  Badge,
  Button,
  IconButton,
  Kbd,
  Menu,
  SegmentedControl,
  Tooltip,
  cx,
  useToast,
  type MenuEntry,
} from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { availableCredits, creditLevel, useSession } from "../../stores/session";
import { useUi, type Mode } from "../../stores/ui";
import { flushLayouts, useLayout } from "../../stores/layout";
import { useSidebar } from "../../stores/sidebar";
import { NotificationsBell } from "../notifications/NotificationsPopover";
import { UpdatePill } from "../updates/UpdatePill";
import { UsageChip } from "./UsageChip";
import { ConductorButton } from "../conductor/ConductorButton";
import { UsageDialog } from "../usage/UsagePage";
import { useWorkspaces } from "../../stores/workspaces";
import { copyDiagnostics, switchMode } from "./actions";
import { LogoMark } from "./LogoMark";
import { isDesktopChrome, useBoot } from "./useBoot";
import { WindowControls } from "./WindowControls";
import { useShortcutKeys } from "../commands/useShortcutKeys";

const MODES: Array<{
  id: Mode;
  key: "titlebar.mode.agent" | "titlebar.mode.code" | "titlebar.mode.thread";
  n: string;
}> = [
  { id: "agent", key: "titlebar.mode.agent", n: "1" },
  { id: "code", key: "titlebar.mode.code", n: "2" },
  { id: "thread", key: "titlebar.mode.thread", n: "3" },
];

/** UI-01 title bar (F-SH-01). Drag region everywhere except the controls;
 * Tab order follows the visual order. */
export function TitleBar() {
  const keys = useShortcutKeys();
  const [usageOpen, setUsageOpen] = useState(false);
  const boot = useBoot((s) => s.boot);
  const { mode, railCollapsed, settingsOpen, toggleRail, setPalette, openSettings, openUpgrade } = useUi();
  const sidebarOpen = useSidebar((s) => s.open);
  const auth = useSession((s) => s.auth);
  const entitlement = useSession((s) => s.entitlement);
  const workspaces = useWorkspaces();
  const { show } = useToast();
  const chrome = isDesktopChrome(boot?.platform);
  const credits = auth?.signedIn ? availableCredits(entitlement) : null;
  const level = creditLevel(entitlement);
  const offline = entitlement ? !entitlement.online : false;
  const active = workspaces.list.find((w) => w.id === workspaces.activeId);

  const appMenu: MenuEntry[] = [
    { id: "settings", label: t("menu.settings"), kbd: keys("settings.open"), onSelect: () => openSettings() },
    { id: "updates", label: t("menu.checkUpdates"), disabled: true },
    { id: "shortcuts", label: t("menu.shortcuts"), kbd: keys("help.shortcuts"), onSelect: () => useUi.getState().setCheatsheet(true) },
    { id: "sep1", type: "separator" },
    {
      id: "help",
      label: t("menu.help"),
      onSelect: () =>
        invoke("app_open_external", { url: "https://vibezeo.com/download/#getting-started" }).catch(
          () => undefined,
        ),
    },
    { id: "diagnostics", label: t("menu.diagnostics"), onSelect: () => useUi.getState().setDiagnostics(true) },
    { id: "diag", label: t("menu.copyDiagnostics"), onSelect: () => void copyDiagnostics(show) },
    {
      id: "logs",
      label: t("menu.openLogs"),
      onSelect: () => invoke("app_open_logs_folder").catch(() => undefined),
    },
    { id: "sep2", type: "separator" },
    auth?.signedIn
      ? {
          id: "signout",
          label: t("menu.signOut"),
          onSelect: () => invoke("auth_sign_out").catch(() => undefined),
        }
      : {
          id: "signin",
          label: t("menu.signIn"),
          onSelect: () => invoke("auth_sign_in").catch(() => undefined),
        },
    {
      id: "quit",
      label: t("menu.quit"),
      kbd: ["Mod", "Q"],
      onSelect: () => {
        // Pending layout saves are stored before the app exits (F-SH-07).
        void flushLayouts().then(() => invoke("app_quit").catch(() => undefined));
      },
    },
  ];

  const planBadge =
    entitlement?.plan === "pro"
      ? t("plan.pro")
      : entitlement?.plan === "team"
        ? t("plan.team")
        : t("plan.free");
  const accountAction: MenuEntry[] = [
    { id: "sep", type: "separator" },
    auth?.signedIn
      ? {
          id: "signout",
          label: t("menu.signOut"),
          onSelect: () => invoke("auth_sign_out").catch(() => undefined),
        }
      : {
          id: "signin",
          label: t("menu.signIn"),
          onSelect: () => invoke("auth_sign_in").catch(() => undefined),
        },
  ];
  const accountMenu: MenuEntry[] = [
    {
      id: "who",
      type: "label",
      label: auth?.email ?? (auth && !auth.hosted ? t("menu.earlyAccess") : t("menu.offlineMode")),
    },
    { id: "billing", label: t("menu.accountBilling"), onSelect: () => openSettings("account") },
    { id: "settings", label: t("menu.settings"), kbd: keys("settings.open"), onSelect: () => openSettings() },
    { id: "plans", label: t("menu.seePlans"), onSelect: () => openUpgrade() },
    ...(auth && !auth.hosted ? [] : accountAction),
  ];

  const activeTab = active ? workspaces.activeTab[active.id] : undefined;
  const cardsOn = useLayout((st) => (activeTab ? st.cards[activeTab] !== undefined : false));
  const inCode = mode === "code" && activeTab !== undefined;
  const workspaceMenu: MenuEntry[] = [
    {
      id: "arrange",
      label: t("menu.ws.arrange"),
      kbd: keys("workspace.arrange"),
      disabled: !inCode,
      onSelect: () => activeTab && useLayout.getState().arrange(activeTab),
    },
    {
      id: "cards",
      type: "checkbox",
      label: t("menu.ws.cards"),
      checked: cardsOn,
      disabled: !inCode,
      onCheckedChange: () => activeTab && useLayout.getState().toggleCards(activeTab),
    },
    {
      id: "sessions",
      type: "checkbox",
      label: t("menu.ws.sessions"),
      checked: !(active?.sessionsCollapsed ?? false),
      disabled: !active,
      onCheckedChange: (show) => active && void workspaces.setSessionsCollapsed(active.id, !show),
    },
    { id: "usage", label: t("menu.ws.usage"), onSelect: () => setUsageOpen(true) },
  ];

  return (
    <header
      role="banner"
      aria-label={t("titlebar.label")}
      data-tauri-drag-region
      className="relative z-[var(--z-titlebar)] flex h-[var(--titlebar-h)] shrink-0 select-none items-center gap-1.5 border-b border-divider bg-surface-1 pl-2"
    >
      {!chrome && <div className="w-[72px] shrink-0" data-tauri-drag-region />}
      {chrome ? (
        <Menu
          entries={appMenu}
          trigger={
            <button
              type="button"
              aria-label={t("titlebar.appMenu")}
              className="inline-flex h-7 items-center gap-1.5 rounded-sm px-1.5 hover:bg-surface-4 focus-visible:shadow-focus focus-visible:outline-none"
            >
              <LogoMark size={18} />
              <span className="text-base font-semibold text-fg">{t("app.name")}</span>
              {boot?.dev && <Badge variant="waiting">{t("app.dev")}</Badge>}
              <ChevronDown size={14} aria-hidden="true" className="text-fg-subtle" />
            </button>
          }
        />
      ) : (
        <span className="inline-flex items-center gap-1.5 px-1" data-tauri-drag-region>
          <LogoMark size={18} />
          <span className="text-base font-semibold text-fg">{t("app.name")}</span>
          {boot?.dev && <Badge variant="waiting">{t("app.dev")}</Badge>}
        </span>
      )}
      <IconButton
        icon={<PanelLeft size={16} />}
        label={t("titlebar.toggleRail")}
        pressed={!railCollapsed}
        kbd={keys("rail.toggle")}
        onClick={toggleRail}
      />
      {!settingsOpen && (
        <SegmentedControl
          label={t("titlebar.workMode")}
          modeSwitch
          size="sm"
          value={mode}
          onValueChange={(v) => switchMode(v as Mode)}
          items={MODES.map((m) => ({
            id: m.id,
            label: t(m.key),
            tooltip: t("titlebar.mode.tooltip", { mode: t(m.key) }),
            kbd: keys(`mode.${m.id}`),
          }))}
        />
      )}
      {mode === "code" && !settingsOpen && active && (
        <Menu
          entries={workspaceMenu}
          trigger={
            <Button
              size="sm"
              variant="ghost"
              aria-label={`${t("titlebar.workspaceMenu")}: ${active.name}`}
              iconRight={<ChevronDown size={14} />}
            >
              {active.name}
            </Button>
          }
        />
      )}
      <div className="h-full min-w-4 flex-1" data-tauri-drag-region />
      <button
        type="button"
        onClick={() => setPalette(true)}
        className="hidden h-7 w-[360px] shrink items-center gap-2 rounded-sm border border-border bg-surface-2 px-2.5 text-sm text-fg-subtle hover:border-border-strong focus-visible:shadow-focus focus-visible:outline-none min-[1180px]:inline-flex"
      >
        <Search size={14} aria-hidden="true" />
        <span className="min-w-0 flex-1 truncate text-left">{t("titlebar.omnibox")}</span>
        {keys("palette.open") && <Kbd keys={keys("palette.open") ?? []} />}
      </button>
      <span className="min-[1180px]:hidden">
        <IconButton
          icon={<CommandIcon size={16} />}
          label={t("titlebar.omnibox")}
          kbd={keys("palette.open")}
          onClick={() => setPalette(true)}
        />
      </span>
      <div className="h-full min-w-4 flex-1" data-tauri-drag-region />
      {credits !== null && (
        <Tooltip label={t("titlebar.creditsTooltip")}>
          <button
            type="button"
            aria-label={t("titlebar.credits", { n: credits })}
            data-testid="titlebar-credits"
            data-level={level ?? "normal"}
            onClick={() => openSettings("account")}
            className={cx(
              "inline-flex h-7 items-center gap-1 rounded-sm px-2 text-sm tabular-nums hover:bg-surface-4 focus-visible:shadow-focus focus-visible:outline-none",
              level === "danger" ? "text-danger" : level === "warning" ? "text-warning" : "text-fg-muted",
            )}
          >
            <span aria-hidden="true">{t("titlebar.creditsGlyph")}</span>
            {credits.toLocaleString("en-US")}
          </button>
        </Tooltip>
      )}
      <UsageChip />
      <UpdatePill />
      <NotificationsBell />
      <IconButton
        icon={<PanelRight size={16} />}
        label={t("titlebar.toggleSidebar")}
        pressed={sidebarOpen}
        kbd={keys("sidebar.toggle")}
        data-sidebar-toggle=""
        data-testid="sidebar-toggle"
        onClick={() => useSidebar.getState().toggle()}
      />
      <ConductorButton />
      <Menu
        align="end"
        entries={[{ id: "plan", type: "label", label: planBadge }, ...accountMenu]}
        trigger={
          <button
            type="button"
            aria-label={t("titlebar.account")}
            className="mx-1 inline-flex rounded-md focus-visible:shadow-focus focus-visible:outline-none"
          >
            <Avatar name={auth?.email ?? t("app.name")} id={auth?.email ?? "offline"} size={24} />
          </button>
        }
      />
      {chrome && <WindowControls />}
      {offline && <span aria-hidden="true" className="absolute inset-x-0 bottom-0 h-0.5 bg-warning" />}
      <UsageDialog open={usageOpen} onOpenChange={setUsageOpen} />
    </header>
  );
}
