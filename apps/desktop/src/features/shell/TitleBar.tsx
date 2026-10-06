import { Bell, ChevronDown, Command as CommandIcon, Gem, PanelLeft, PanelRight, Search } from "lucide-react";
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
import { totalCredits, useSession } from "../../stores/session";
import { useUi, type Mode } from "../../stores/ui";
import { useLayout } from "../../stores/layout";
import { useWorkspaces } from "../../stores/workspaces";
import { copyDiagnostics, switchMode } from "./actions";
import { LogoMark } from "./LogoMark";
import { isDesktopChrome, useBoot } from "./useBoot";
import { WindowControls } from "./WindowControls";

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
  const boot = useBoot((s) => s.boot);
  const {
    mode,
    railCollapsed,
    sidebarOpen,
    settingsOpen,
    toggleRail,
    toggleSidebar,
    setPalette,
    openSettings,
    openUpgrade,
  } = useUi();
  const auth = useSession((s) => s.auth);
  const entitlement = useSession((s) => s.entitlement);
  const workspaces = useWorkspaces();
  const { show } = useToast();
  const chrome = isDesktopChrome(boot?.platform);
  const credits = auth?.signedIn ? totalCredits(entitlement) : null;
  const grant = entitlement?.monthlyGrant ?? 10_000;
  const offline = entitlement ? !entitlement.online : false;
  const active = workspaces.list.find((w) => w.id === workspaces.activeId);

  const appMenu: MenuEntry[] = [
    { id: "settings", label: t("menu.settings"), kbd: ["Mod", ","], onSelect: () => openSettings() },
    { id: "updates", label: t("menu.checkUpdates"), disabled: true },
    { id: "shortcuts", label: t("menu.shortcuts"), kbd: ["Mod", "/"], disabled: true },
    { id: "sep1", type: "separator" },
    {
      id: "help",
      label: t("menu.help"),
      onSelect: () => invoke("app_open_external", { url: "https://tandem.dev/docs" }).catch(() => undefined),
    },
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
      onSelect: () => invoke("app_quit").catch(() => undefined),
    },
  ];

  const planBadge =
    entitlement?.plan === "pro"
      ? t("plan.pro")
      : entitlement?.plan === "team"
        ? t("plan.team")
        : t("plan.free");
  const accountMenu: MenuEntry[] = [
    { id: "who", type: "label", label: auth?.email ?? t("menu.offlineMode") },
    { id: "billing", label: t("menu.accountBilling"), onSelect: () => openSettings("account") },
    { id: "settings", label: t("menu.settings"), kbd: ["Mod", ","], onSelect: () => openSettings() },
    { id: "plans", label: t("menu.seePlans"), onSelect: () => openUpgrade() },
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

  const activeTab = active ? workspaces.activeTab[active.id] : undefined;
  const cardsOn = useLayout((st) => (activeTab ? st.cards[activeTab] !== undefined : false));
  const inCode = mode === "code" && activeTab !== undefined;
  const workspaceMenu: MenuEntry[] = [
    {
      id: "arrange",
      label: t("menu.ws.arrange"),
      kbd: ["Mod", "Shift", "T"],
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
    { id: "usage", label: t("menu.ws.usage"), disabled: true },
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
        kbd={["Mod", "\\"]}
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
            kbd: ["Mod", m.n],
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
        <span className="flex-1 text-left">{t("titlebar.omnibox")}</span>
        <Kbd keys={["Mod", "K"]} />
      </button>
      <span className="min-[1180px]:hidden">
        <IconButton
          icon={<CommandIcon size={16} />}
          label={t("titlebar.omnibox")}
          kbd={["Mod", "K"]}
          onClick={() => setPalette(true)}
        />
      </span>
      <div className="h-full min-w-4 flex-1" data-tauri-drag-region />
      {credits !== null && (
        <Tooltip label={t("titlebar.creditsTooltip")}>
          <button
            type="button"
            aria-label={t("titlebar.credits", { n: credits })}
            onClick={() => openSettings("account")}
            className={cx(
              "inline-flex h-7 items-center gap-1 rounded-sm px-2 text-sm tabular-nums hover:bg-surface-4 focus-visible:shadow-focus focus-visible:outline-none",
              credits === 0 ? "text-danger" : credits < grant * 0.1 ? "text-warning" : "text-fg-muted",
            )}
          >
            <Gem size={13} aria-hidden="true" />
            {credits.toLocaleString("en-US")}
          </button>
        </Tooltip>
      )}
      <Menu
        align="end"
        entries={[{ id: "empty", type: "label", label: t("notifications.empty") }]}
        trigger={<IconButton icon={<Bell size={16} />} label={t("titlebar.notifications")} />}
      />
      <IconButton
        icon={<PanelRight size={16} />}
        label={t("titlebar.toggleSidebar")}
        pressed={sidebarOpen}
        kbd={["Mod", "Shift", "B"]}
        onClick={toggleSidebar}
      />
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
    </header>
  );
}
