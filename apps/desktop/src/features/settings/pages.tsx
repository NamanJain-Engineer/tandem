import type { ReactNode } from "react";
import { t } from "../../i18n";
import { AccountPage } from "./AccountPage";
import { EnginesPage } from "./EnginesPage";
import { AccountsPage } from "./AccountsPage";
import { NotificationTypes, SoundPreview } from "./NotificationTypes";
import { AdvancedExtras, PrivacyFlows } from "./extras";
import { UpdatesPanel } from "../updates/UpdatesPanel";
import { RunSetupAgain } from "../onboarding/RunSetupAgain";
import {
  BoolControl,
  FontControl,
  NumberControl,
  SegmentControl,
  SelectControl,
  ShellIntegrationControl,
  TextControl,
  ThemeCards,
} from "./controls";

type Ids = { id: string; describedBy?: string };

export type RowDef = {
  id: string;
  label: string;
  description?: string;
  keywords?: string[];
  control: (ids: Ids) => ReactNode;
};

export type PageDef = {
  id: string;
  label: string;
  subtitle: string;
  keywords?: string[];
  callout?: ReactNode;
  rows: RowDef[];
  /** Custom content after the rows (Account, privacy table, diagnostics). */
  extra?: ReactNode;
};

const opts = (keys: string[], prefix: string) =>
  keys.map((k) => ({ value: k, label: t(`${prefix}.${k}` as Parameters<typeof t>[0]) }));
const segs = (keys: string[], prefix: string) =>
  keys.map((k) => ({ id: k, label: t(`${prefix}.${k}` as Parameters<typeof t>[0]) }));

/** The pages shipped so far (UI-101, 102, 104, 105, 112, 114, 115, 116), in UI-100 order. */
export function settingsPages(platform: string): PageDef[] {
  const mac = platform === "macos";
  return [
    {
      id: "general",
      label: t("settings.general"),
      subtitle: t("settings.general.subtitle"),
      rows: [
        {
          id: "launch_at_login",
          label: t("settings.launchAtLogin"),
          description: t("settings.launchAtLogin.desc"),
          keywords: ["startup", "login", "autostart"],
          control: (ids) => <BoolControl k="launch_at_login" ids={ids} label={t("settings.launchAtLogin")} />,
        },
        {
          id: "keep_running_in_tray",
          label: t("settings.keepInTray"),
          keywords: ["tray", "background"],
          control: (ids) => (
            <BoolControl k="keep_running_in_tray" ids={ids} label={t("settings.keepInTray")} />
          ),
        },
        {
          id: "open_links_in_tandem",
          label: t("settings.openLinks"),
          description: t("settings.openLinks.desc"),
          keywords: ["browser", "links"],
          control: (ids) => (
            <BoolControl k="open_links_in_tandem" ids={ids} label={t("settings.openLinks")} />
          ),
        },
        {
          id: "auto_save_files",
          label: t("settings.autoSave"),
          keywords: ["save", "files"],
          control: (ids) => <BoolControl k="auto_save_files" ids={ids} label={t("settings.autoSave")} />,
        },
        {
          id: "term_confirm_multiline_paste",
          label: t("settings.confirmPaste"),
          keywords: ["paste"],
          control: (ids) => (
            <BoolControl k="term_confirm_multiline_paste" ids={ids} label={t("settings.confirmPaste")} />
          ),
        },
        {
          id: "resume_agents_automatically",
          label: t("settings.resumeAgents"),
          description: t("settings.resumeAgents.desc"),
          keywords: ["restore", "restart"],
          control: (ids) => (
            <BoolControl k="resume_agents_automatically" ids={ids} label={t("settings.resumeAgents")} />
          ),
        },
        {
          id: "language",
          label: t("settings.language"),
          keywords: ["language", "locale"],
          control: (ids) => (
            <SelectControl
              k="language"
              ids={ids}
              fallback="en"
              options={[{ value: "en", label: t("settings.language.en") }]}
            />
          ),
        },
        {
          id: "run_setup",
          label: t("settings.runSetup"),
          description: t("settings.runSetup.desc"),
          keywords: ["onboarding", "setup", "wizard", "welcome"],
          control: (ids) => <RunSetupAgain id={ids.id} />,
        },
      ],
    },
    {
      id: "appearance",
      label: t("settings.appearance"),
      subtitle: t("settings.appearance.subtitle"),
      rows: [
        {
          id: "theme",
          label: t("settings.theme"),
          keywords: ["dark", "light", "system", "colors"],
          control: () => <ThemeCards label={t("settings.theme")} />,
        },
        {
          id: "terminal_theme",
          label: t("settings.terminalTheme"),
          keywords: ["palette", "colors", "night", "day"],
          control: (ids) => (
            <SelectControl
              k="terminal_theme"
              ids={ids}
              fallback="follow_app"
              options={opts(["follow_app", "night", "day", "high_contrast"], "settings.terminalTheme")}
            />
          ),
        },
        {
          id: "interface_text_size",
          label: t("settings.textSize"),
          keywords: ["font", "zoom", "size"],
          control: () => (
            <SegmentControl
              k="interface_text_size"
              label={t("settings.textSize")}
              fallback="default"
              options={segs(["small", "default", "large"], "settings.textSize")}
            />
          ),
        },
        {
          id: "term_font_family",
          label: t("settings.termFont"),
          keywords: ["font", "monospace"],
          control: () => <FontControl k="term_font_family" label={t("settings.termFont")} />,
        },
        {
          id: "term_font_size",
          label: t("settings.termFontSize"),
          keywords: ["font", "size"],
          control: (ids) => (
            <NumberControl
              k="term_font_size"
              ids={ids}
              min={11}
              max={18}
              fallback={13}
              label={t("settings.termFontSize")}
            />
          ),
        },
        {
          id: "density",
          label: t("settings.density"),
          keywords: ["rows", "compact"],
          control: () => (
            <SegmentControl
              k="density"
              label={t("settings.density")}
              fallback="compact"
              options={segs(["compact", "comfortable"], "settings.density")}
            />
          ),
        },
        {
          id: "reduce_motion",
          label: t("settings.reduceMotion"),
          keywords: ["animation", "motion"],
          control: (ids) => (
            <SelectControl
              k="reduce_motion"
              ids={ids}
              fallback="system"
              options={opts(["system", "on", "off"], "settings.reduceMotion")}
            />
          ),
        },
        {
          id: "show_status_previews",
          label: t("settings.statusPreviews"),
          keywords: ["dashboard", "preview"],
          control: (ids) => (
            <BoolControl k="show_status_previews" ids={ids} label={t("settings.statusPreviews")} />
          ),
        },
      ],
    },
    {
      id: "notifications",
      label: t("settings.notifications"),
      subtitle: t("settings.notifications.subtitle"),
      keywords: ["notifications", "alerts", "sound", "bell", "badge"],
      rows: [
        {
          id: "notify_system",
          label: t("settings.notify.systemToggle"),
          keywords: ["system", "os", "desktop notifications"],
          control: (ids) => (
            <BoolControl k="notify_system" ids={ids} label={t("settings.notify.systemToggle")} />
          ),
        },
        {
          id: "notify_only_unseen",
          label: t("settings.notify.onlyUnseen"),
          description: t("settings.notify.onlyUnseen.desc"),
          keywords: ["background", "focused", "visible"],
          control: (ids) => (
            <BoolControl k="notify_only_unseen" ids={ids} label={t("settings.notify.onlyUnseen")} />
          ),
        },
        {
          id: "notify_attention",
          label: mac ? t("settings.notify.bounce") : t("settings.notify.flash"),
          keywords: ["dock", "bounce", "taskbar", "flash"],
          control: (ids) => (
            <BoolControl
              k="notify_attention"
              ids={ids}
              label={mac ? t("settings.notify.bounce") : t("settings.notify.flash")}
            />
          ),
        },
        {
          id: "notify_sound",
          label: t("settings.notify.soundToggle"),
          keywords: ["sound", "chime", "audio"],
          control: (ids) => (
            <BoolControl k="notify_sound" ids={ids} label={t("settings.notify.soundToggle")} />
          ),
        },
        {
          id: "notify_sound_name",
          label: t("settings.notify.soundName"),
          keywords: ["sound", "chime", "tone"],
          control: (ids) => (
            <span className="flex items-center gap-1">
              <SelectControl
                k="notify_sound_name"
                ids={ids}
                fallback="chime"
                options={opts(["chime", "soft", "pop", "none"], "settings.notify.tone")}
              />
              <SoundPreview />
            </span>
          ),
        },
      ],
      extra: <NotificationTypes />,
    },
    {
      id: "terminal",
      label: t("settings.terminal"),
      subtitle: t("settings.terminal.subtitle"),
      rows: [
        {
          id: "default_shell",
          label: t("settings.defaultShell"),
          description: t("settings.defaultShell.desc"),
          keywords: ["shell", "bash", "zsh", "fish", "powershell"],
          control: (ids) => (
            <TextControl
              k="default_shell"
              ids={ids}
              monospace
              placeholder={t("settings.defaultShell.login")}
              label={t("settings.defaultShell")}
            />
          ),
        },
        {
          id: "term_cursor_style",
          label: t("settings.cursor"),
          keywords: ["cursor", "caret"],
          control: () => (
            <SegmentControl
              k="term_cursor_style"
              label={t("settings.cursor")}
              fallback="bar"
              options={segs(["bar", "block", "underline"], "settings.cursor")}
            />
          ),
        },
        {
          id: "term_cursor_blink",
          label: t("settings.cursorBlink"),
          keywords: ["cursor", "blink"],
          control: (ids) => <BoolControl k="term_cursor_blink" ids={ids} label={t("settings.cursorBlink")} />,
        },
        {
          id: "term_scrollback",
          label: t("settings.scrollback"),
          description: t("settings.scrollback.desc"),
          keywords: ["scroll", "history", "buffer"],
          control: (ids) => (
            <NumberControl
              k="term_scrollback"
              ids={ids}
              min={1000}
              max={100000}
              fallback={10000}
              label={t("settings.scrollback")}
            />
          ),
        },
        {
          id: "term_copy_on_select",
          label: t("settings.copyOnSelect"),
          keywords: ["copy", "clipboard"],
          control: (ids) => (
            <BoolControl k="term_copy_on_select" ids={ids} label={t("settings.copyOnSelect")} />
          ),
        },
        ...(mac
          ? [
              {
                id: "term_option_as_meta",
                label: t("settings.optionAsMeta"),
                keywords: ["option", "alt", "meta"],
                control: (ids: Ids) => (
                  <BoolControl k="term_option_as_meta" ids={ids} label={t("settings.optionAsMeta")} />
                ),
              },
            ]
          : [
              {
                id: "term_shortcut_priority",
                label: t("settings.shortcutPriority"),
                keywords: ["shortcut", "keys"],
                control: (ids: Ids) => (
                  <SelectControl
                    k="term_shortcut_priority"
                    ids={ids}
                    fallback="terminal"
                    options={opts(["terminal", "tandem"], "settings.shortcutPriority")}
                  />
                ),
              },
            ]),
        {
          id: "term_shell_integration",
          label: t("settings.shellIntegration"),
          description: t("settings.shellIntegration.desc"),
          keywords: ["shell integration", "cwd", "prompt", "bash", "zsh", "fish", "powershell"],
          control: (ids) => <ShellIntegrationControl ids={ids} label={t("settings.shellIntegration")} />,
        },
        {
          id: "term_gpu_rendering",
          label: t("settings.gpu"),
          keywords: ["gpu", "webgl", "lite"],
          control: (ids) => (
            <SelectControl
              k="term_gpu_rendering"
              ids={ids}
              fallback="auto"
              options={opts(["auto", "off"], "settings.gpu")}
            />
          ),
        },
      ],
    },
    {
      id: "engines",
      label: t("settings.engines"),
      subtitle: t("settings.engines.subtitle"),
      keywords: ["engines", "agents", "claude", "codex", "install", "path", "arguments", "detect"],
      rows: [],
      extra: <EnginesPage />,
    },
    {
      id: "accounts",
      label: t("settings.accounts"),
      subtitle: t("settings.accounts.subtitle"),
      keywords: ["accounts", "sign in", "login", "default", "override", "claude", "codex", "provider"],
      rows: [],
      extra: <AccountsPage />,
    },
    {
      id: "teammates",
      label: t("settings.teammates"),
      subtitle: t("settings.teammates.subtitle"),
      keywords: ["teammates", "agent", "skills", "memory"],
      rows: [
        {
          id: "allow_teammate_skills",
          label: t("settings.teammateSkills"),
          description: t("settings.teammateSkills.desc"),
          keywords: ["skills", "write"],
          control: (ids) => (
            <BoolControl k="allow_teammate_skills" ids={ids} label={t("settings.teammateSkills")} />
          ),
        },
      ],
    },
    {
      id: "privacy",
      label: t("settings.privacy"),
      subtitle: t("settings.privacy.subtitle"),
      keywords: ["telemetry", "analytics", "privacy"],
      callout: t("settings.privacy.callout"),
      rows: [
        {
          id: "telemetry_usage",
          label: t("settings.usageStats"),
          description: t("settings.usageStats.desc"),
          keywords: ["analytics", "telemetry", "statistics"],
          control: (ids) => <BoolControl k="telemetry_usage" ids={ids} label={t("settings.usageStats")} />,
        },
        {
          id: "telemetry_crash",
          label: t("settings.crashReports"),
          description: t("settings.crashReports.desc"),
          keywords: ["crash", "errors"],
          control: (ids) => <BoolControl k="telemetry_crash" ids={ids} label={t("settings.crashReports")} />,
        },
        {
          id: "ai_thread_titles",
          label: t("settings.aiThreadTitles"),
          description: t("settings.aiThreadTitles.desc"),
          keywords: ["titles", "ai", "thread names"],
          control: (ids) => (
            <BoolControl k="ai_thread_titles" ids={ids} label={t("settings.aiThreadTitles")} />
          ),
        },
        {
          id: "auto_consent",
          label: t("settings.autoConsent"),
          description: t("settings.autoConsent.desc"),
          keywords: ["auto", "terminal output"],
          control: (ids) => <BoolControl k="auto_consent" ids={ids} label={t("settings.autoConsent")} />,
        },
        {
          id: "dictate_share_hints",
          label: t("settings.dictionaryHints"),
          keywords: ["dictionary", "dictate"],
          control: (ids) => (
            <BoolControl k="dictate_share_hints" ids={ids} label={t("settings.dictionaryHints")} />
          ),
        },
      ],
      extra: <PrivacyFlows />,
    },
    {
      id: "account",
      label: t("settings.account"),
      subtitle: t("settings.account.subtitle"),
      keywords: ["plan", "billing", "credits", "subscription", "sign out"],
      rows: [],
      extra: <AccountPage />,
    },
    {
      id: "updates",
      label: t("settings.updates"),
      subtitle: t("settings.updates.subtitle"),
      keywords: ["update", "version", "channel", "beta", "release"],
      rows: [
        {
          id: "auto_check_updates",
          label: t("settings.autoCheckUpdates"),
          keywords: ["automatic", "check"],
          control: (ids) => (
            <BoolControl k="auto_check_updates" ids={ids} label={t("settings.autoCheckUpdates")} />
          ),
        },
      ],
      extra: <UpdatesPanel />,
    },
    {
      id: "advanced",
      label: t("settings.advanced"),
      subtitle: t("settings.advanced.subtitle"),
      keywords: ["diagnostics", "logs", "about", "version"],
      rows: [
        {
          id: "log_level",
          label: t("settings.logLevel"),
          keywords: ["logs", "verbose", "debug"],
          control: (ids) => (
            <SelectControl
              k="log_level"
              ids={ids}
              fallback="normal"
              options={opts(["normal", "verbose"], "settings.logLevel")}
            />
          ),
        },
        {
          id: "perf_hud",
          label: t("settings.perfHud"),
          keywords: ["performance", "fps", "hud"],
          control: (ids) => <BoolControl k="perf_hud" ids={ids} label={t("settings.perfHud")} />,
        },
        {
          id: "orphan_policy",
          label: t("settings.orphans"),
          keywords: ["processes", "orphans"],
          control: (ids) => (
            <SelectControl
              k="orphan_policy"
              ids={ids}
              fallback="ask"
              options={opts(["ask", "always", "never"], "settings.orphans")}
            />
          ),
        },
      ],
      extra: <AdvancedExtras />,
    },
  ];
}

/** Rows whose label, description or keywords contain `q` (F-SH-04 AC4). */
export function matchRow(row: RowDef, q: string): boolean {
  const needle = q.trim().toLowerCase();
  if (!needle) return false;
  return [row.label, row.description ?? "", ...(row.keywords ?? [])].some((s) =>
    s.toLowerCase().includes(needle),
  );
}

export function matchPage(page: PageDef, q: string): boolean {
  const needle = q.trim().toLowerCase();
  if (!needle) return true;
  return (
    page.label.toLowerCase().includes(needle) ||
    (page.keywords ?? []).some((k) => k.includes(needle)) ||
    page.rows.some((r) => matchRow(r, needle))
  );
}
