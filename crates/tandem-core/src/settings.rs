//! Typed settings schema (F-SH-04 core). Every key has a default, a validator
//! and the Settings page it lives on (UI-100 navigation).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

use crate::error::{CoreError, ErrorCode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SettingsPage {
    General,
    Appearance,
    Notifications,
    Terminal,
    Engines,
    Accounts,
    Teammates,
    Dictate,
    Conductor,
    Plugins,
    Remote,
    Privacy,
    /// Checkpoints (F-EX-01, UI-117).
    Checkpoints,
    Shortcuts,
    Account,
    Updates,
    Advanced,
    /// Not shown in Settings (UI state that persists).
    Hidden,
}

macro_rules! setting_keys {
    ($($name:ident),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
        #[serde(rename_all = "snake_case")]
        #[ts(export)]
        pub enum SettingKey {
            $($name),*
        }

        impl SettingKey {
            pub const ALL: &'static [SettingKey] = &[$(SettingKey::$name),*];
        }
    };
}

setting_keys! {
    Theme,
    TerminalTheme,
    TerminalsMatchTheme,
    InterfaceTextSize,
    Density,
    ShowStatusPreviews,
    Language,
    AutoSaveFiles,
    LaunchAtLogin,
    KeepRunningInTray,
    UpdateChannel,
    AutoCheckUpdates,
    NotifySystem,
    NotifySound,
    NotifySoundName,
    NotifyAttention,
    NotifyTypes,
    NotifyOnlyUnseen,
    UsageForecastNotify,
    RunawayGuard,
    RunawayAction,
    DefaultShell,
    WslDefaultDistro,
    TermFontFamily,
    TermFontSize,
    TermCursorStyle,
    TermCursorBlink,
    TermScrollback,
    TermCopyOnSelect,
    TermConfirmMultilinePaste,
    TermOptionAsMeta,
    TermScreenReader,
    TermShellIntegration,
    TermShortcutPriority,
    ShortcutOverrides,
    TermGpuRendering,
    OpenLinksInTandem,
    SearchEngine,
    EngineExtraArgs,
    DefaultAccess,
    RoutineAccess,
    AllowTeammateSkills,
    LearnAfterTurn,
    PauseAllMail,
    PauseAllRoutines,
    ResumeAgentsAutomatically,
    TrustWorkspacesIAdd,
    ExposeTasksToTerminalAgents,
    ExposeAllPluginTools,
    GrantPluginsToAllThreads,
    PluginAskBeforeWrites,
    DictateEnabled,
    DictateHotkey,
    DictateToggleHotkey,
    DictateLatch,
    DictateHotkeyVerified,
    DictateCloudConsented,
    DictateEnhanceDisclosed,
    DictateEngine,
    DictateModel,
    DictateLanguage,
    DictateMic,
    DictateBackupMic,
    DictateGain,
    DictateWidgetVisibility,
    DictateWidgetStyle,
    DictateFollowCursor,
    DictateLockPosition,
    DictateSounds,
    DictatePauseMedia,
    DictateOutput,
    DictateAutoEnhance,
    DictateQuickActions,
    DictateFormatting,
    DictateFormattingApps,
    DictateShareHints,
    DictateDictionarySync,
    DictateDictionarySyncState,
    DictateFnFallback,
    DictateGlobePrevious,
    DictateKeepHistory,
    DictateWidgetPosition,
    ConductorName,
    ConductorVoice,
    ConductorTone,
    ConductorVerbosity,
    ConductorSpeed,
    ConductorAboutYou,
    ConductorMemory,
    ConductorReflection,
    ConductorWake,
    ConductorWakeSensitivity,
    ConductorWakePauseVideos,
    ConductorFloatingRing,
    ConductorCallLogs,
    ConductorRingPosition,
    TelemetryUsage,
    TelemetryCrash,
    AiThreadTitles,
    AutoConsent,
    AutoDontAsk,
    FindWorkHosted,
    SendKey,
    ReduceMotion,
    LogLevel,
    PerfHud,
    OrphanPolicy,
    LastMode,
    SettingsLastPage,
    OnboardingStep,
    OfflineStartedAt,
    WhatsNewSeen,
    RailWidth,
    ThreadDefaults,
    ThreadGrid,
    CheckpointsEnabled,
    CheckpointsKeep,
    CheckpointsDays,
    CheckpointsMaxFileMb,
    CommandHistoryEnabled,
    CommandHistoryDays,
    SkillsSeen,
}

/// How a value is checked before it is stored.
#[derive(Debug, Clone, Copy)]
pub enum Validator {
    Bool,
    OneOf(&'static [&'static str]),
    IntRange(i64, i64),
    Text {
        max: usize,
    },
    OptionalText {
        max: usize,
    },
    /// A JSON object whose values are strings, booleans or string lists.
    Map,
    /// Command id → a shortcut in the registry's form, or "" for none
    /// (F-SH-09 rebinding).
    Shortcuts,
    OptionalInt,
    /// Named limits: each value a non-negative number, `null` (off) or a
    /// boolean (F-AC-09's runaway guard).
    Limits,
}

pub struct SettingSpec {
    pub key: SettingKey,
    pub default: fn() -> Value,
    pub validator: Validator,
    pub page: SettingsPage,
    /// Words the Settings filter matches besides the label (F-SH-04 AC4).
    pub keywords: &'static [&'static str],
}

macro_rules! spec {
    ($key:ident, $default:expr, $validator:expr, $page:ident, [$($kw:literal),*]) => {
        SettingSpec {
            key: SettingKey::$key,
            default: || $default,
            validator: $validator,
            page: SettingsPage::$page,
            keywords: &[$($kw),*],
        }
    };
}

use Validator as V;

const ACCESS: &[&str] = &["read", "edits", "full"];

pub static SCHEMA: &[SettingSpec] = &[
    spec!(
        Theme,
        json!("dark"),
        V::OneOf(&["dark", "light", "system"]),
        Appearance,
        ["theme", "dark", "light"]
    ),
    spec!(
        TerminalTheme,
        json!("follow_app"),
        V::OneOf(&["follow_app", "night", "day", "high_contrast"]),
        Appearance,
        ["terminal theme", "colors"]
    ),
    spec!(
        TerminalsMatchTheme,
        json!(true),
        V::Bool,
        Appearance,
        ["terminal", "theme"]
    ),
    spec!(
        InterfaceTextSize,
        json!("default"),
        V::OneOf(&["small", "default", "large"]),
        Appearance,
        ["text size", "font"]
    ),
    spec!(
        Density,
        json!("compact"),
        V::OneOf(&["compact", "comfortable"]),
        Appearance,
        ["density", "rows"]
    ),
    spec!(
        ShowStatusPreviews,
        json!(true),
        V::Bool,
        Appearance,
        ["dashboard", "preview"]
    ),
    spec!(
        Language,
        json!("en"),
        V::OneOf(&["en"]),
        General,
        ["language"]
    ),
    spec!(
        AutoSaveFiles,
        json!(true),
        V::Bool,
        General,
        ["save", "files"]
    ),
    spec!(
        LaunchAtLogin,
        json!(false),
        V::Bool,
        General,
        ["login", "startup"]
    ),
    spec!(
        KeepRunningInTray,
        json!(false),
        V::Bool,
        General,
        ["tray", "background"]
    ),
    spec!(
        UpdateChannel,
        json!("stable"),
        V::OneOf(&["stable", "beta"]),
        Updates,
        ["channel", "beta"]
    ),
    spec!(
        AutoCheckUpdates,
        json!(true),
        V::Bool,
        Updates,
        ["updates", "check"]
    ),
    spec!(
        NotifySystem,
        json!(true),
        V::Bool,
        Notifications,
        ["system notifications"]
    ),
    spec!(NotifySound, json!(true), V::Bool, Notifications, ["sound"]),
    spec!(
        NotifySoundName,
        json!("chime"),
        V::OneOf(&["chime", "soft", "pop", "none"]),
        Notifications,
        ["sound"]
    ),
    spec!(
        NotifyAttention,
        json!(true),
        V::Bool,
        Notifications,
        ["dock", "bounce", "flash", "taskbar"]
    ),
    spec!(NotifyTypes, json!({}), V::Map, Notifications, ["types"]),
    spec!(
        NotifyOnlyUnseen,
        json!(true),
        V::Bool,
        Notifications,
        ["background", "unfocused", "visible"]
    ),
    spec!(
        UsageForecastNotify,
        json!(false),
        V::Bool,
        Notifications,
        ["usage", "limit", "forecast", "runs out"]
    ),
    spec!(
        DefaultShell,
        json!(null),
        V::OptionalText { max: 512 },
        Terminal,
        ["shell", "bash", "zsh", "powershell"]
    ),
    spec!(
        WslDefaultDistro,
        json!(null),
        V::OptionalText { max: 128 },
        Terminal,
        ["wsl"]
    ),
    spec!(
        TermFontFamily,
        json!("JetBrains Mono"),
        V::Text { max: 128 },
        Appearance,
        ["font", "terminal"]
    ),
    spec!(
        TermFontSize,
        json!(13),
        V::IntRange(11, 18),
        Appearance,
        ["font size"]
    ),
    spec!(
        TermCursorStyle,
        json!("bar"),
        V::OneOf(&["bar", "block", "underline"]),
        Terminal,
        ["cursor"]
    ),
    spec!(
        TermCursorBlink,
        json!(false),
        V::Bool,
        Terminal,
        ["cursor", "blink"]
    ),
    spec!(
        TermScrollback,
        json!(10_000),
        V::IntRange(1_000, 100_000),
        Terminal,
        ["scrollback", "history", "lines"]
    ),
    spec!(
        TermCopyOnSelect,
        json!(false),
        V::Bool,
        Terminal,
        ["copy", "select"]
    ),
    spec!(
        TermConfirmMultilinePaste,
        json!(true),
        V::Bool,
        General,
        ["paste", "multiple lines"]
    ),
    spec!(
        TermOptionAsMeta,
        json!(true),
        V::Bool,
        Terminal,
        ["option", "meta", "alt"]
    ),
    spec!(
        TermScreenReader,
        json!(false),
        V::Bool,
        Terminal,
        ["screen reader", "accessibility"]
    ),
    spec!(
        TermShellIntegration,
        json!({"bash": true, "zsh": true, "fish": true, "pwsh": true}),
        V::Map,
        Terminal,
        ["shell integration"]
    ),
    spec!(
        TermShortcutPriority,
        json!("terminal"),
        V::OneOf(&["terminal", "tandem"]),
        Terminal,
        ["shortcut priority"]
    ),
    // Rebound app shortcuts (F-SH-09 P2, UI-113): command id → shortcut, or
    // "" for none. Commands not listed keep the registry's default.
    spec!(
        ShortcutOverrides,
        json!({}),
        V::Shortcuts,
        Shortcuts,
        ["shortcuts", "keyboard", "keys", "rebind", "hotkey"]
    ),
    spec!(
        TermGpuRendering,
        json!("auto"),
        V::OneOf(&["auto", "off"]),
        Terminal,
        ["gpu", "webgl", "rendering"]
    ),
    spec!(
        OpenLinksInTandem,
        json!(true),
        V::Bool,
        General,
        ["links", "browser"]
    ),
    spec!(
        SearchEngine,
        json!("duckduckgo"),
        V::OneOf(&["duckduckgo", "google", "bing", "kagi"]),
        General,
        ["search"]
    ),
    spec!(
        EngineExtraArgs,
        json!({}),
        V::Map,
        Engines,
        ["arguments", "flags"]
    ),
    spec!(
        RunawayGuard,
        json!({"workingMinutes": 90, "windowPoints": 30, "subagents": 20, "circles": true}),
        V::Limits,
        Teammates,
        ["runaway", "guard", "limit", "subagents", "loop"]
    ),
    spec!(
        RunawayAction,
        json!("notify"),
        V::OneOf(&["notify", "pause"]),
        Teammates,
        ["runaway", "pause", "guard"]
    ),
    spec!(
        DefaultAccess,
        json!("edits"),
        V::OneOf(ACCESS),
        Teammates,
        ["access", "permissions"]
    ),
    spec!(
        RoutineAccess,
        json!("read"),
        V::OneOf(&["read", "edits"]),
        Teammates,
        ["routine", "access"]
    ),
    spec!(
        AllowTeammateSkills,
        json!(true),
        V::Bool,
        Teammates,
        ["skills"]
    ),
    spec!(
        LearnAfterTurn,
        json!(true),
        V::Bool,
        Teammates,
        ["learn", "memory"]
    ),
    spec!(
        PauseAllMail,
        json!(false),
        V::Bool,
        Teammates,
        ["mail", "pause"]
    ),
    spec!(
        PauseAllRoutines,
        json!(false),
        V::Bool,
        Teammates,
        ["routines", "pause", "schedule"]
    ),
    spec!(
        ResumeAgentsAutomatically,
        json!(false),
        V::Bool,
        General,
        ["resume", "restart"]
    ),
    spec!(
        TrustWorkspacesIAdd,
        json!(false),
        V::Bool,
        General,
        ["trust"]
    ),
    spec!(
        ExposeTasksToTerminalAgents,
        json!(true),
        V::Bool,
        Teammates,
        ["tasks", "mcp"]
    ),
    spec!(
        ExposeAllPluginTools,
        json!(false),
        V::Bool,
        Plugins,
        ["tools"]
    ),
    spec!(
        GrantPluginsToAllThreads,
        json!(false),
        V::Bool,
        Plugins,
        ["threads", "grant"]
    ),
    spec!(
        PluginAskBeforeWrites,
        json!(true),
        V::Bool,
        Plugins,
        ["approve", "write"]
    ),
    spec!(
        DictateEnabled,
        json!(true),
        V::Bool,
        Dictate,
        ["dictate", "voice"]
    ),
    spec!(
        DictateHotkey,
        json!(null),
        V::OptionalText { max: 64 },
        Dictate,
        ["hotkey", "shortcut"]
    ),
    spec!(
        DictateToggleHotkey,
        json!(null),
        V::OptionalText { max: 64 },
        Dictate,
        ["toggle", "shortcut"]
    ),
    spec!(
        DictateLatch,
        json!(true),
        V::Bool,
        Dictate,
        ["double-tap", "hands-free", "lock"]
    ),
    spec!(DictateHotkeyVerified, json!(false), V::Bool, Hidden, []),
    // F-DI-05's consent dialog was accepted; F-DI-10's disclosure was shown.
    spec!(DictateCloudConsented, json!(false), V::Bool, Hidden, []),
    spec!(DictateEnhanceDisclosed, json!(false), V::Bool, Hidden, []),
    spec!(
        DictateEngine,
        json!("local"),
        V::OneOf(&["local", "cloud"]),
        Dictate,
        ["engine", "cloud"]
    ),
    spec!(
        DictateModel,
        json!(null),
        V::OptionalText { max: 128 },
        Dictate,
        ["model", "whisper", "parakeet"]
    ),
    spec!(
        DictateLanguage,
        json!("auto"),
        V::Text { max: 16 },
        Dictate,
        ["language"]
    ),
    spec!(
        DictateMic,
        json!(null),
        V::OptionalText { max: 256 },
        Dictate,
        ["microphone", "mic"]
    ),
    spec!(
        DictateBackupMic,
        json!(null),
        V::OptionalText { max: 256 },
        Dictate,
        ["backup microphone"]
    ),
    spec!(
        DictateGain,
        json!("off"),
        V::OneOf(&["off", "2x", "4x", "6x", "8x"]),
        Dictate,
        ["gain", "boost", "volume"]
    ),
    spec!(
        DictateWidgetVisibility,
        json!("while_recording"),
        V::OneOf(&["always", "while_recording", "hidden"]),
        Dictate,
        ["widget"]
    ),
    spec!(
        DictateWidgetStyle,
        json!("logo_text"),
        V::OneOf(&["logo_text", "logo", "mono"]),
        Dictate,
        ["widget style"]
    ),
    spec!(
        DictateFollowCursor,
        json!(true),
        V::Bool,
        Dictate,
        ["follow cursor"]
    ),
    spec!(
        DictateLockPosition,
        json!(false),
        V::Bool,
        Dictate,
        ["lock position"]
    ),
    spec!(
        DictateSounds,
        json!(true),
        V::Bool,
        Dictate,
        ["sounds", "chime"]
    ),
    spec!(
        DictatePauseMedia,
        json!(false),
        V::Bool,
        Dictate,
        ["pause media", "audio"]
    ),
    spec!(
        DictateOutput,
        json!("paste"),
        V::OneOf(&["paste", "clipboard"]),
        Dictate,
        ["output", "paste", "clipboard"]
    ),
    spec!(
        DictateAutoEnhance,
        json!(false),
        V::Bool,
        Dictate,
        ["enhance", "polish"]
    ),
    spec!(
        DictateQuickActions,
        json!(true),
        V::Bool,
        Dictate,
        ["enhance", "polish", "after each take"]
    ),
    // App id → formatting instructions (≤2,000 characters each), F-DI-10.
    spec!(
        DictateFormattingApps,
        json!({}),
        V::Map,
        Dictate,
        ["formatting", "per app"]
    ),
    spec!(
        DictateFormatting,
        json!(null),
        V::OptionalText { max: 2_000 },
        Dictate,
        ["formatting", "instructions"]
    ),
    spec!(
        DictateShareHints,
        json!(false),
        V::Bool,
        Privacy,
        ["dictionary hints", "cloud"]
    ),
    spec!(
        DictateDictionarySync,
        json!(false),
        V::Bool,
        Privacy,
        ["dictionary", "sync", "devices"]
    ),
    spec!(DictateDictionarySyncState, json!({}), V::Map, Hidden, []),
    // F-DI-02: null, "switched" (Fn went unheard; the hold key became
    // Ctrl+Option) or "kept_fn" (the user chose Fn again).
    spec!(
        DictateFnFallback,
        json!(null),
        V::OptionalText { max: 16 },
        Hidden,
        []
    ),
    // F-DI-11: {"usage": <the 🌐 preference before the app changed it, or
    // "unset">}; empty when the app hasn't changed it.
    spec!(DictateGlobePrevious, json!({}), V::Map, Hidden, []),
    spec!(
        DictateKeepHistory,
        json!("forever"),
        V::OneOf(&["forever", "90d", "30d", "7d"]),
        Dictate,
        ["history", "keep"]
    ),
    spec!(
        DictateWidgetPosition,
        json!(null),
        V::OptionalText { max: 32 },
        Hidden,
        []
    ),
    spec!(
        ConductorName,
        json!("Conductor"),
        V::Text { max: 40 },
        Conductor,
        ["name"]
    ),
    spec!(
        ConductorVoice,
        json!("default"),
        V::Text { max: 64 },
        Conductor,
        ["voice"]
    ),
    spec!(
        ConductorTone,
        json!("neutral"),
        V::OneOf(&["neutral", "warm", "brief"]),
        Conductor,
        ["tone"]
    ),
    spec!(
        ConductorVerbosity,
        json!("normal"),
        V::OneOf(&["short", "normal", "detailed"]),
        Conductor,
        ["verbosity"]
    ),
    spec!(
        ConductorSpeed,
        json!(3),
        V::IntRange(1, 5),
        Conductor,
        ["speed"]
    ),
    spec!(
        ConductorAboutYou,
        json!(null),
        V::OptionalText { max: 1_000 },
        Conductor,
        ["about you"]
    ),
    spec!(
        ConductorMemory,
        json!(false),
        V::Bool,
        Conductor,
        ["memory"]
    ),
    spec!(
        ConductorReflection,
        json!("ask"),
        V::OneOf(&["ask", "auto", "off"]),
        Conductor,
        ["reflection", "journal"]
    ),
    spec!(
        ConductorWake,
        json!(false),
        V::Bool,
        Conductor,
        ["wake", "hey conductor"]
    ),
    spec!(
        ConductorWakeSensitivity,
        json!("medium"),
        V::OneOf(&["low", "medium", "high"]),
        Conductor,
        ["wake", "sensitivity"]
    ),
    spec!(
        ConductorWakePauseVideos,
        json!(true),
        V::Bool,
        Conductor,
        ["wake", "videos", "pause"]
    ),
    spec!(
        ConductorFloatingRing,
        json!(false),
        V::Bool,
        Conductor,
        ["floating", "ring"]
    ),
    spec!(
        ConductorCallLogs,
        json!("7"),
        V::OneOf(&["off", "7", "30"]),
        Conductor,
        ["call log", "history", "keep"]
    ),
    spec!(
        ConductorRingPosition,
        json!(null),
        V::OptionalText { max: 32 },
        Hidden,
        []
    ),
    spec!(
        TelemetryUsage,
        json!(false),
        V::Bool,
        Privacy,
        ["usage statistics", "analytics", "telemetry"]
    ),
    spec!(
        TelemetryCrash,
        json!(false),
        V::Bool,
        Privacy,
        ["crash reports"]
    ),
    // D-118: thread titles from the hosted AI send the first message off the
    // machine, so they're opt-in; the local title is used otherwise.
    spec!(
        AiThreadTitles,
        json!(false),
        V::Bool,
        Privacy,
        ["titles", "ai", "thread names"]
    ),
    spec!(
        AutoConsent,
        json!(false),
        V::Bool,
        Privacy,
        ["auto", "terminal output"]
    ),
    spec!(AutoDontAsk, json!(false), V::Bool, Hidden, []),
    // D-1302: Find work's hosted ranking sends the search and the candidates'
    // redacted names and last lines off the machine, so it's opt-in.
    spec!(
        FindWorkHosted,
        json!(false),
        V::Bool,
        Privacy,
        ["find work", "search", "ranking", "ai"]
    ),
    spec!(
        SendKey,
        json!("enter"),
        V::OneOf(&["enter", "mod_enter"]),
        General,
        ["send", "enter"]
    ),
    spec!(
        ReduceMotion,
        json!("system"),
        V::OneOf(&["system", "on", "off"]),
        Appearance,
        ["motion", "animation"]
    ),
    spec!(
        LogLevel,
        json!("normal"),
        V::OneOf(&["normal", "verbose"]),
        Advanced,
        ["log level", "verbose"]
    ),
    spec!(
        PerfHud,
        json!(false),
        V::Bool,
        Advanced,
        ["performance", "hud", "fps"]
    ),
    spec!(
        OrphanPolicy,
        json!("ask"),
        V::OneOf(&["ask", "always", "never"]),
        Advanced,
        ["orphaned processes"]
    ),
    spec!(
        LastMode,
        json!("code"),
        V::OneOf(&["agent", "code", "thread"]),
        Hidden,
        []
    ),
    spec!(
        SettingsLastPage,
        json!("general"),
        V::Text { max: 32 },
        Hidden,
        []
    ),
    spec!(
        OnboardingStep,
        json!(null),
        V::OptionalText { max: 32 },
        Hidden,
        []
    ),
    spec!(OfflineStartedAt, json!(null), V::OptionalInt, Hidden, []),
    // F-WB-03: the last version whose "What's new" was shown.
    spec!(
        WhatsNewSeen,
        json!(null),
        V::OptionalText { max: 64 },
        Hidden,
        []
    ),
    // The Code rail's width (F-SH-03: 180–360 px, default 248). The Agent and
    // Thread rails get their own keys with their lists.
    spec!(RailWidth, json!(248), V::IntRange(180, 360), Hidden, []),
    // Thread mode's remembered defaults (F-TH-07 AC4): `last_engine`, and
    // per engine `<engine>.model`, `<engine>.effort`, `<engine>.access`.
    spec!(ThreadDefaults, json!({}), V::Map, Hidden, []),
    spec!(ThreadGrid, json!({"ids": []}), V::Map, Hidden, []),
    // F-EX-01: checkpoints before agent turns, kept per folder.
    spec!(
        CheckpointsEnabled,
        json!(true),
        V::Bool,
        Checkpoints,
        ["checkpoints", "rewind", "undo", "restore"]
    ),
    spec!(
        CheckpointsKeep,
        json!(100),
        V::IntRange(10, 1000),
        Checkpoints,
        ["checkpoints", "keep"]
    ),
    spec!(
        CheckpointsDays,
        json!(14),
        V::IntRange(1, 90),
        Checkpoints,
        ["checkpoints", "days"]
    ),
    spec!(
        CheckpointsMaxFileMb,
        json!(5),
        V::IntRange(1, 100),
        Checkpoints,
        ["checkpoints", "large files"]
    ),
    // F-EX-02: command recall, local only.
    spec!(
        CommandHistoryEnabled,
        json!(true),
        V::Bool,
        Terminal,
        ["command recall", "history"]
    ),
    spec!(
        CommandHistoryDays,
        json!(90),
        V::IntRange(1, 3650),
        Terminal,
        ["command recall", "history", "retention"]
    ),
    // Per teammate, the skills the user made or opened; any other is the
    // teammate's own and "New" (F-AG-09).
    spec!(SkillsSeen, json!({}), V::Map, Hidden, []),
];

pub fn spec(key: SettingKey) -> &'static SettingSpec {
    SCHEMA
        .iter()
        .find(|s| s.key == key)
        .unwrap_or_else(|| panic!("setting {key:?} missing from SCHEMA"))
}

pub fn default_value(key: SettingKey) -> Value {
    (spec(key).default)()
}

impl SettingKey {
    /// The snake_case name stored in SQLite.
    pub fn as_str(&self) -> String {
        match serde_json::to_value(self) {
            Ok(Value::String(s)) => s,
            _ => format!("{self:?}"),
        }
    }

    pub fn parse(s: &str) -> Option<SettingKey> {
        serde_json::from_value(Value::String(s.to_string())).ok()
    }
}

fn invalid(key: SettingKey, why: &str) -> CoreError {
    CoreError::new(
        "settings",
        format!("{} {why}", key.as_str()),
        "pick a value from the list",
    )
    .with_code(ErrorCode::Invalid)
}

pub fn validate(key: SettingKey, v: &Value) -> Result<(), CoreError> {
    match spec(key).validator {
        Validator::Bool => v
            .as_bool()
            .map(|_| ())
            .ok_or_else(|| invalid(key, "must be on or off")),
        Validator::OneOf(options) => match v.as_str() {
            Some(s) if options.contains(&s) => Ok(()),
            _ => Err(invalid(key, "is not one of the allowed values")),
        },
        Validator::IntRange(min, max) => match v.as_i64() {
            Some(n) if (min..=max).contains(&n) => Ok(()),
            _ => Err(invalid(
                key,
                &format!("must be a whole number from {min} to {max}"),
            )),
        },
        Validator::Text { max } => match v.as_str() {
            Some(s) if !s.is_empty() && s.chars().count() <= max => Ok(()),
            _ => Err(invalid(
                key,
                &format!("must be text up to {max} characters"),
            )),
        },
        Validator::OptionalText { max } => match v {
            Value::Null => Ok(()),
            Value::String(s) if s.chars().count() <= max => Ok(()),
            _ => Err(invalid(
                key,
                &format!("must be empty or text up to {max} characters"),
            )),
        },
        Validator::Map => match v.as_object() {
            Some(map)
                if map.values().all(|x| {
                    x.is_string()
                        || x.is_boolean()
                        || x.as_array().is_some_and(|a| a.iter().all(Value::is_string))
                }) =>
            {
                Ok(())
            }
            _ => Err(invalid(key, "must be a map")),
        },
        Validator::Limits => match v.as_object() {
            Some(map)
                if map.values().all(|x| {
                    x.is_null() || x.is_boolean() || x.as_f64().is_some_and(|n| n >= 0.0)
                }) =>
            {
                Ok(())
            }
            _ => Err(invalid(key, "must name limits as numbers, off or on")),
        },
        Validator::OptionalInt => match v {
            Value::Null => Ok(()),
            v if v.is_i64() => Ok(()),
            _ => Err(invalid(key, "must be empty or a number")),
        },
        Validator::Shortcuts => match v.as_object() {
            Some(map)
                if map.iter().all(|(id, keys)| {
                    crate::commands::find(id).is_some()
                        && keys
                            .as_str()
                            .is_some_and(|k| k.is_empty() || crate::commands::valid_binding(k))
                }) =>
            {
                Ok(())
            }
            _ => Err(invalid(key, "must map commands to shortcuts")),
        },
    }
}

/// A value in a type that satisfies the key's validator but differs from the
/// default. Used by round-trip tests.
pub fn sample_non_default(key: SettingKey) -> Value {
    let default = default_value(key);
    match spec(key).validator {
        Validator::Bool => json!(!default.as_bool().unwrap_or(false)),
        Validator::OneOf(options) => {
            let pick = options
                .iter()
                .find(|o| Some(**o) != default.as_str())
                .unwrap_or(&options[0]);
            json!(pick)
        }
        Validator::IntRange(min, max) => {
            let d = default.as_i64().unwrap_or(min);
            json!(if d == max { min } else { d + 1 })
        }
        Validator::Text { .. } => json!("Sample"),
        Validator::OptionalText { .. } => json!("sample"),
        Validator::Map => json!({"sample": true}),
        Validator::OptionalInt => json!(1_700_000_000_000_i64),
        Validator::Limits => json!({"sample": 5, "other": null, "flag": false}),
        Validator::Shortcuts => json!({"pane.split_right": "Mod+Alt+R"}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_key_has_one_spec_with_a_valid_default() {
        let mut seen = HashSet::new();
        for s in SCHEMA {
            assert!(seen.insert(s.key), "duplicate {:?}", s.key);
            validate(s.key, &(s.default)()).unwrap_or_else(|e| panic!("{:?}: {e}", s.key));
            let sample = sample_non_default(s.key);
            validate(s.key, &sample).unwrap_or_else(|e| panic!("{:?} sample: {e}", s.key));
            assert_eq!(SettingKey::parse(&s.key.as_str()), Some(s.key));
        }
        assert_eq!(seen.len(), SCHEMA.len());
        assert_eq!(
            SettingKey::ALL.len(),
            SCHEMA.len(),
            "every key needs a spec"
        );
    }

    #[test]
    fn validators_reject_out_of_range_values() {
        assert!(validate(SettingKey::TermScrollback, &json!(999)).is_err());
        assert!(validate(SettingKey::TermScrollback, &json!(100_001)).is_err());
        assert!(validate(SettingKey::TermScrollback, &json!(5_000)).is_ok());
        assert!(validate(SettingKey::Theme, &json!("pink")).is_err());
        assert!(validate(SettingKey::DefaultAccess, &json!("full")).is_ok());
        assert!(validate(SettingKey::RoutineAccess, &json!("full")).is_err());
    }

    #[test]
    fn f_ac_09_6_runaway_limits_take_numbers_off_and_on() {
        let ok =
            json!({"workingMinutes": 30, "windowPoints": null, "subagents": 5, "circles": false});
        assert!(validate(SettingKey::RunawayGuard, &ok).is_ok());
        assert!(validate(SettingKey::RunawayGuard, &json!({"workingMinutes": -1})).is_err());
        assert!(validate(SettingKey::RunawayGuard, &json!({"workingMinutes": "90"})).is_err());
        assert!(validate(SettingKey::RunawayGuard, &json!(90)).is_err());
        assert!(validate(SettingKey::RunawayAction, &json!("pause")).is_ok());
        assert!(validate(SettingKey::RunawayAction, &json!("kill")).is_err());
    }

    #[test]
    fn f_sh_09_6_shortcut_overrides_name_commands_and_valid_shortcuts() {
        let k = SettingKey::ShortcutOverrides;
        assert_eq!(default_value(k), json!({}));
        assert_eq!(spec(k).page, SettingsPage::Shortcuts);
        // A new shortcut, or "" for none.
        assert!(validate(
            k,
            &json!({"pane.split_right": "Mod+Alt+R", "pane.close": ""})
        )
        .is_ok());
        assert!(validate(k, &json!({"no.such_command": "Mod+Alt+R"})).is_err());
        assert!(validate(k, &json!({"pane.split_right": "R"})).is_err());
        assert!(validate(k, &json!({"pane.split_right": true})).is_err());
        assert!(validate(k, &json!(["pane.split_right"])).is_err());
    }

    /// `pnpm gen:protocol` / `gen:docs` (F-WB-02): writes every setting's
    /// key, page and default to `TANDEM_SETTINGS_OUT` for the docs.
    #[test]
    fn export_settings() {
        let Ok(out) = std::env::var("TANDEM_SETTINGS_OUT") else {
            return;
        };
        let rows: Vec<_> = SCHEMA
            .iter()
            .filter(|s| s.page != SettingsPage::Hidden)
            .map(|s| json!({ "key": s.key, "page": s.page, "default": (s.default)() }))
            .collect();
        let json = serde_json::to_string_pretty(&rows).expect("json");
        std::fs::write(out, json + "\n").expect("write settings.json");
    }

    #[test]
    fn privacy_toggles_default_off() {
        for key in [
            SettingKey::TelemetryUsage,
            SettingKey::TelemetryCrash,
            SettingKey::AiThreadTitles,
            SettingKey::AutoConsent,
            SettingKey::DictateShareHints,
            SettingKey::DictateDictionarySync,
            SettingKey::FindWorkHosted,
        ] {
            assert_eq!(default_value(key), json!(false), "{key:?}");
        }
    }
}
