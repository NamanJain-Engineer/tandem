import type { ReactNode } from "react";
import { t } from "../../i18n";
import { AccountPage } from "./AccountPage";
import { EnginesPage } from "./EnginesPage";
import { ShortcutsPage } from "./ShortcutsPage";
import { AccountsPage } from "./AccountsPage";
import { RemoteHosts } from "../remote/RemoteHosts";
import { WslDistros } from "../remote/WslDistros";
import { RemotePage } from "./RemotePage";
import { PluginsPage } from "../plugins/PluginsPage";
import { NotificationTypes, SoundPreview } from "./NotificationTypes";
import { AdvancedExtras, PrivacyFlows } from "./extras";
import { UpdatesPanel } from "../updates/UpdatesPanel";
import { RunSetupAgain } from "../onboarding/RunSetupAgain";
import { CheckpointsSettingsExtra } from "../checkpoints/CheckpointsSettings";
import { RecallSettingsExtra } from "../recall/RecallSettings";
import { DictateExtras, MicSelect, ShortcutField } from "../dictate/DictatePage";
import { LanguageSelect } from "../dictate/LanguageSelect";
import { AboutYou, MemoryPanel, NameControl, SpeedControl, WakeControl } from "../conductor/ConductorSettings";
import { ConductorCallSettings } from "../conductor/CallSettingsSlot";
import { EngineControl } from "../dictate/EngineControl";
import { AutoEnhanceControl, FormattingControl, QuickActionsControl } from "../dictate/EnhanceControls";
import {
  BoolControl,
  FontControl,
  NumberControl,
  PolicyBoolControl,
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
  /** Content before the rows, unboxed (Settings › Dictate's home card). */
  header?: ReactNode;
  rows: RowDef[];
  /** Custom content after the rows (Account, privacy table, diagnostics). */
  extra?: ReactNode;
};

const opts = (keys: string[], prefix: string) =>
  keys.map((k) => ({ value: k, label: t(`${prefix}.${k}` as Parameters<typeof t>[0]) }));
const segs = (keys: string[], prefix: string) =>
  keys.map((k) => ({ id: k, label: t(`${prefix}.${k}` as Parameters<typeof t>[0]) }));

/** The pages shipped so far (UI-101, 102, 104, 105, 112, 113, 114, 115, 116), in UI-100 order.
 * `routines`: the plan has routines; without them "Pause routines" is absent.
 * `accounts`: the build talks to Vibezeo (not TANDEM_HOSTED=0); without it,
 * rows for hosted services are absent (D-833). */
export function settingsPages(platform: string, routines = false, accounts = false): PageDef[] {
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
        {
          id: "command_history_enabled",
          label: t("settings.commandHistoryEnabled"),
          description: t("settings.commandHistoryEnabled.description"),
          keywords: ["command recall", "history", "commands"],
          control: (ids) => (
            <BoolControl k="command_history_enabled" ids={ids} label={t("settings.commandHistoryEnabled")} />
          ),
        },
        {
          id: "command_history_days",
          label: t("settings.commandHistoryDays"),
          keywords: ["command recall", "history", "retention"],
          control: (ids) => (
            <NumberControl
              k="command_history_days"
              ids={ids}
              min={1}
              max={3650}
              fallback={90}
              label={t("settings.commandHistoryDays")}
            />
          ),
        },
      ],
      extra: <RecallSettingsExtra />,
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
      id: "remote",
      label: t("settings.remote"),
      // WSL exists on Windows only (F-RM-02): elsewhere the page is SSH and the phone,
      // with no WSL heading or skeleton to flash and hide.
      subtitle: platform === "windows" ? t("settings.remote.subtitle") : t("settings.remote.subtitleSsh"),
      keywords: [
        "ssh",
        "remote",
        "host",
        "server",
        ...(platform === "windows" ? ["wsl", "linux"] : []),
        "phone",
        "pair",
        "mobile",
        "qr",
        "relay",
        "approvals",
      ],
      rows: [],
      extra: (
        <div className="flex flex-col gap-6">
          <RemoteHosts />
          {platform === "windows" && <WslDistros />}
          <RemotePage />
        </div>
      ),
    },
    {
      id: "dictate",
      label: t("settings.dictate"),
      subtitle: t("settings.dictate.subtitle"),
      keywords: ["dictate", "voice", "speech", "microphone", "whisper", "parakeet", "push to talk"],
      header: <DictateExtras />,
      rows: [
        {
          id: "dictate_enabled",
          label: t("dictate.enabled"),
          keywords: ["on", "off"],
          control: (ids) => <BoolControl k="dictate_enabled" ids={ids} label={t("dictate.enabled")} />,
        },
        {
          id: "dictate_engine",
          label: t("dictate.engine.label"),
          keywords: ["engine", "cloud", "local", "transcribe", "pro"],
          control: () => <EngineControl />,
        },
        {
          id: "dictate_language",
          label: t("dictate.language"),
          keywords: ["language", "auto-detect"],
          control: () => <LanguageSelect />,
        },
        {
          id: "dictate_mic",
          label: t("dictate.mic.label"),
          description: t("dictate.mic.desc"),
          keywords: ["microphone", "input", "bluetooth"],
          control: () => (
            <MicSelect k="dictate_mic" label={t("dictate.mic.label")} none={t("dictate.mic.none")} />
          ),
        },
        {
          id: "dictate_backup_mic",
          label: t("dictate.mic.backup"),
          keywords: ["backup", "fallback"],
          control: () => (
            <MicSelect k="dictate_backup_mic" label={t("dictate.mic.backup")} none={t("dictate.mic.none")} />
          ),
        },
        {
          id: "dictate_gain",
          label: t("dictate.gain"),
          keywords: ["boost", "gain", "volume"],
          control: () => (
            <SegmentControl
              k="dictate_gain"
              label={t("dictate.gain")}
              fallback="off"
              options={segs(["off", "2x", "4x", "6x", "8x"], "dictate.gain")}
            />
          ),
        },
        {
          id: "dictate_hotkey",
          label: t("dictate.shortcut.hold"),
          description: mac ? t("dictate.shortcut.fnHint") : undefined,
          keywords: ["shortcut", "hotkey", "key", "hold"],
          control: () => <ShortcutField mode="hold" label={t("dictate.shortcut.hold")} />,
        },
        {
          id: "dictate_toggle_hotkey",
          label: t("dictate.shortcut.toggle"),
          keywords: ["toggle", "shortcut"],
          control: () => <ShortcutField mode="toggle" label={t("dictate.shortcut.toggle")} />,
        },
        {
          id: "dictate_latch",
          label: t("dictate.latch"),
          keywords: ["double-tap", "hands-free", "lock"],
          control: (ids) => <BoolControl k="dictate_latch" ids={ids} label={t("dictate.latch")} />,
        },
        {
          id: "dictate_output",
          label: t("dictate.output"),
          description: platform === "windows" ? t("dictate.output.windowsNote") : undefined,
          keywords: ["paste", "clipboard", "insert"],
          control: (ids) => (
            <SelectControl
              k="dictate_output"
              ids={ids}
              fallback="paste"
              options={opts(["paste", "clipboard"], "dictate.output")}
            />
          ),
        },
        ...(platform === "windows"
          ? []
          : [
              {
                id: "dictate_pause_media",
                label: t("dictate.pauseMedia"),
                keywords: ["mute", "music", "pause"],
                control: (ids: Ids) => (
                  <BoolControl k="dictate_pause_media" ids={ids} label={t("dictate.pauseMedia")} />
                ),
              },
            ]),
        {
          id: "dictate_sounds",
          label: t("dictate.sounds"),
          keywords: ["chime", "sound"],
          control: (ids) => <BoolControl k="dictate_sounds" ids={ids} label={t("dictate.sounds")} />,
        },
        {
          id: "dictate_widget_visibility",
          label: t("dictate.widget.visibility"),
          keywords: ["widget", "hud", "pill"],
          control: (ids) => (
            <SelectControl
              k="dictate_widget_visibility"
              ids={ids}
              fallback="while_recording"
              options={opts(["always", "while_recording", "hidden"], "dictate.widget.visibility")}
            />
          ),
        },
        {
          id: "dictate_widget_style",
          label: t("dictate.widget.style"),
          keywords: ["widget style"],
          control: (ids) => (
            <SelectControl
              k="dictate_widget_style"
              ids={ids}
              fallback="logo_text"
              options={opts(["logo_text", "logo", "mono"], "dictate.widget.style")}
            />
          ),
        },
        {
          id: "dictate_follow_cursor",
          label: t("dictate.widget.follow"),
          keywords: ["displays", "monitor", "cursor"],
          control: (ids) => (
            <BoolControl k="dictate_follow_cursor" ids={ids} label={t("dictate.widget.follow")} />
          ),
        },
        {
          id: "dictate_lock_position",
          label: t("dictate.widget.lock"),
          keywords: ["position", "drag"],
          control: (ids) => (
            <BoolControl k="dictate_lock_position" ids={ids} label={t("dictate.widget.lock")} />
          ),
        },
        {
          id: "dictate_quick_actions",
          label: t("dictate.enhance.quickActions"),
          keywords: ["enhance", "polish", "after each take", "pro"],
          control: () => <QuickActionsControl label={t("dictate.enhance.quickActions")} />,
        },
        {
          id: "dictate_auto_enhance",
          label: t("dictate.enhance.auto"),
          description: t("dictate.enhance.disclosure"),
          keywords: ["enhance", "auto", "prompt", "pro"],
          control: () => <AutoEnhanceControl label={t("dictate.enhance.auto")} />,
        },
        {
          id: "dictate_formatting",
          label: t("dictate.enhance.formatting"),
          description: t("dictate.enhance.formattingDesc"),
          keywords: ["formatting", "instructions", "per app", "lowercase"],
          control: () => <FormattingControl label={t("dictate.enhance.formatting")} />,
        },
        {
          id: "dictate_keep_history",
          label: t("dictate.keepHistory"),
          keywords: ["history", "retention"],
          control: (ids) => (
            <SelectControl
              k="dictate_keep_history"
              ids={ids}
              fallback="forever"
              options={opts(["forever", "90d", "30d", "7d"], "dictate.keepHistory")}
            />
          ),
        },
      ],
    },
    {
      id: "conductor",
      label: t("settings.conductor"),
      subtitle: t("settings.conductor.subtitle"),
      keywords: ["conductor", "voice", "persona", "memory", "wake", "hey conductor"],
      callout: t("conductor.privacy"),
      rows: [
        {
          id: "conductor_name",
          label: t("conductor.persona.name"),
          description: t("conductor.persona.name.desc"),
          keywords: ["name", "persona"],
          control: (ids) => <NameControl ids={ids} />,
        },
        {
          id: "conductor_voice",
          label: t("conductor.voice"),
          description: t("conductor.voice.desc"),
          keywords: ["voice"],
          control: (ids) => (
            <SelectControl
              k="conductor_voice"
              ids={ids}
              fallback="default"
              options={[{ value: "default", label: t("conductor.voice.default") }]}
            />
          ),
        },
        {
          id: "conductor_tone",
          label: t("conductor.tone"),
          keywords: ["tone", "warm", "brief", "neutral"],
          control: () => (
            <SegmentControl
              k="conductor_tone"
              label={t("conductor.tone")}
              fallback="neutral"
              options={segs(["neutral", "warm", "brief"], "conductor.tone")}
            />
          ),
        },
        {
          id: "conductor_verbosity",
          label: t("conductor.verbosity"),
          keywords: ["verbosity", "short", "detailed"],
          control: () => (
            <SegmentControl
              k="conductor_verbosity"
              label={t("conductor.verbosity")}
              fallback="normal"
              options={segs(["short", "normal", "detailed"], "conductor.verbosity")}
            />
          ),
        },
        {
          id: "conductor_speed",
          label: t("conductor.speed"),
          description: t("conductor.speed.desc"),
          keywords: ["speed", "faster", "slower"],
          control: () => <SpeedControl />,
        },
        {
          id: "conductor_memory",
          label: t("conductor.memoryToggle"),
          description: t("conductor.memoryToggle.desc"),
          keywords: ["memory", "remember", "forget"],
          control: (ids) => <BoolControl k="conductor_memory" ids={ids} label={t("conductor.memoryToggle")} />,
        },
        {
          id: "conductor_reflection",
          label: t("conductor.reflection"),
          keywords: ["reflection", "after each call", "memories"],
          control: () => (
            <SegmentControl
              k="conductor_reflection"
              label={t("conductor.reflection")}
              fallback="ask"
              options={segs(["ask", "auto", "off"], "conductor.reflection")}
            />
          ),
        },
        {
          id: "conductor_wake",
          label: t("conductor.wake"),
          description: t("conductor.wake.desc"),
          keywords: ["wake", "hey conductor", "listening"],
          control: (ids) => <WakeControl ids={ids} />,
        },
        {
          id: "conductor_wake_sensitivity",
          label: t("conductor.wake.sensitivity"),
          keywords: ["wake", "sensitivity"],
          control: () => (
            <SegmentControl
              k="conductor_wake_sensitivity"
              label={t("conductor.wake.sensitivity")}
              fallback="medium"
              options={segs(["low", "medium", "high"], "conductor.wake.sensitivity")}
            />
          ),
        },
        {
          id: "conductor_wake_pause_videos",
          label: t("conductor.wake.pauseVideos"),
          description: t("conductor.wake.pauseVideos.desc"),
          keywords: ["videos", "media", "pause"],
          control: (ids) => (
            <BoolControl k="conductor_wake_pause_videos" ids={ids} label={t("conductor.wake.pauseVideos")} />
          ),
        },
      ],
      extra: (
        <>
          <ConductorCallSettings />
          <AboutYou />
          <MemoryPanel />
          <p className="text-sm text-fg-muted">{t("conductor.safety")}</p>
        </>
      ),
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
        {
          id: "learn_after_turn",
          label: t("settings.learnAfterTurn"),
          description: t("settings.learnAfterTurn.desc"),
          keywords: ["learn", "memory", "notes"],
          control: (ids) => (
            <BoolControl k="learn_after_turn" ids={ids} label={t("settings.learnAfterTurn")} />
          ),
        },
        {
          id: "pause_all_mail",
          label: t("settings.pauseAllMail"),
          description: t("settings.pauseAllMail.desc"),
          keywords: ["mail", "pause", "message"],
          control: (ids) => <BoolControl k="pause_all_mail" ids={ids} label={t("settings.pauseAllMail")} />,
        },
        ...(routines
          ? [
              {
                id: "pause_all_routines",
                label: t("settings.pauseAllRoutines"),
                description: t("settings.pauseAllRoutines.desc"),
                keywords: ["routines", "pause", "schedule"],
                control: (ids: Ids) => (
                  <BoolControl k="pause_all_routines" ids={ids} label={t("settings.pauseAllRoutines")} />
                ),
              },
            ]
          : []),
      ],
    },
    {
      id: "plugins",
      label: t("settings.plugins"),
      subtitle: t("settings.plugins.subtitle"),
      keywords: ["plugins", "connect", "github", "oauth", "api key", "grants", "approvals", "held"],
      rows: [],
      extra: <PluginsPage />,
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
          control: (ids) => (
            <PolicyBoolControl
              k="telemetry_usage"
              ids={ids}
              label={t("settings.usageStats")}
              lockedBy={(p) => p.forceTelemetryOff}
            />
          ),
        },
        {
          id: "telemetry_crash",
          label: t("settings.crashReports"),
          description: t("settings.crashReports.desc"),
          keywords: ["crash", "errors"],
          control: (ids) => (
            <PolicyBoolControl
              k="telemetry_crash"
              ids={ids}
              label={t("settings.crashReports")}
              lockedBy={(p) => p.forceTelemetryOff}
            />
          ),
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
          control: (ids) => (
            <PolicyBoolControl
              k="auto_consent"
              ids={ids}
              label={t("settings.autoConsent")}
              lockedBy={(p) => p.disableAuto}
            />
          ),
        },
        ...(accounts
          ? [
              {
                id: "find_work_hosted",
                label: t("settings.findWorkHosted"),
                description: t("settings.findWorkHosted.desc"),
                keywords: ["find work", "search", "ranking", "ai"],
                control: (ids: Ids) => (
                  <BoolControl k="find_work_hosted" ids={ids} label={t("settings.findWorkHosted")} />
                ),
              },
            ]
          : []),
        ...(accounts
          ? [
              {
                id: "dictate_dictionary_sync",
                label: t("settings.dictionarySync"),
                description: t("settings.dictionarySync.desc"),
                keywords: ["dictionary", "sync", "devices"],
                control: (ids: Ids) => (
                  <BoolControl k="dictate_dictionary_sync" ids={ids} label={t("settings.dictionarySync")} />
                ),
              },
            ]
          : []),
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
      id: "checkpoints",
      label: t("settings.checkpoints"),
      subtitle: t("settings.checkpoints.subtitle"),
      keywords: ["checkpoints", "rewind", "undo", "restore", "snapshots"],
      rows: [
        {
          id: "checkpoints_enabled",
          label: t("settings.checkpointsEnabled"),
          description: t("settings.checkpointsEnabled.description"),
          keywords: ["checkpoints", "rewind"],
          control: (ids) => (
            <BoolControl k="checkpoints_enabled" ids={ids} label={t("settings.checkpointsEnabled")} />
          ),
        },
        {
          id: "checkpoints_keep",
          label: t("settings.checkpointsKeep"),
          description: t("settings.checkpointsKeep.description"),
          control: (ids) => (
            <NumberControl
              k="checkpoints_keep"
              ids={ids}
              min={10}
              max={1000}
              fallback={100}
              label={t("settings.checkpointsKeep")}
            />
          ),
        },
        {
          id: "checkpoints_days",
          label: t("settings.checkpointsDays"),
          control: (ids) => (
            <NumberControl
              k="checkpoints_days"
              ids={ids}
              min={1}
              max={90}
              fallback={14}
              label={t("settings.checkpointsDays")}
            />
          ),
        },
        {
          id: "checkpoints_max_file_mb",
          label: t("settings.checkpointsMaxFileMb"),
          control: (ids) => (
            <NumberControl
              k="checkpoints_max_file_mb"
              ids={ids}
              min={1}
              max={100}
              fallback={5}
              label={t("settings.checkpointsMaxFileMb")}
            />
          ),
        },
      ],
      extra: <CheckpointsSettingsExtra />,
    },
    {
      id: "shortcuts",
      label: t("settings.shortcuts"),
      subtitle: t("settings.shortcuts.subtitle"),
      keywords: ["shortcuts", "keyboard", "keys", "hotkeys", "rebind"],
      rows: [],
      extra: <ShortcutsPage />,
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
