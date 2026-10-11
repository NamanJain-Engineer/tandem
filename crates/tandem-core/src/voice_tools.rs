//! The Conductor's closed tool catalog (F-CN-02, ARCHITECTURE §12.6): every
//! tool the doer may call, the registry command it stands for (or that it is
//! one of the voice-only tools of F-CN-02's table), and what has to happen on
//! screen before it runs (F-CN-03). The API's doer catalog
//! (`apps/api/voice-tools.json`) lists the same names; a test holds the two
//! equal, and another that every mapped command is in the registry.
//!
//! Local-review-only actions (forced lane removal, enabling Full access,
//! plugin spend or destructive approvals) are absent: nothing here can reach
//! them (F-TL-06 rule 5).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::settings::SettingKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum VoiceToolGroup {
    Navigation,
    Awareness,
    Panes,
    Threads,
    Teammates,
    Tasks,
    Find,
    App,
}

/// What has to happen before a tool runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Review {
    /// Runs when the doer calls it.
    None,
    /// Asks first; "yes" by voice or a click both answer (closing panes,
    /// executing tasks, creating a teammate).
    VoiceOrScreen,
    /// Asks first, and only a click in the main window answers (a shell
    /// command): voice can't approve it (F-CN-03, F-TL-06 rule 5).
    ScreenOnly,
}

/// What a tool stands for in the command registry (F-SH-09).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Maps {
    /// One of these registry commands, by the tool's arguments.
    Commands(&'static [&'static str]),
    /// F-CN-02's voice-specific tools: no palette command does the same.
    VoiceOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceToolSpec {
    pub name: &'static str,
    pub group: VoiceToolGroup,
    pub maps: Maps,
    pub review: Review,
}

/// `set_setting` and `get_setting` reach only these (F-CN-02 AC2): theme,
/// notifications, sounds, the Dictate engine and microphone, the teammate
/// mail pause, and Conductor's voice, tone and verbosity.
pub const SETTABLE: &[SettingKey] = &[
    SettingKey::Theme,
    SettingKey::NotifySystem,
    SettingKey::NotifySound,
    SettingKey::DictateEngine,
    SettingKey::DictateMic,
    SettingKey::PauseAllMail,
    SettingKey::ConductorVoice,
    SettingKey::ConductorTone,
    SettingKey::ConductorVerbosity,
];

/// Whether Conductor may read or change `key`.
pub fn settable(key: SettingKey) -> bool {
    SETTABLE.contains(&key)
}

use Review::{None as Plain, ScreenOnly, VoiceOrScreen};
use VoiceToolGroup as G;

const fn tool(
    name: &'static str,
    group: VoiceToolGroup,
    maps: Maps,
    review: Review,
) -> VoiceToolSpec {
    VoiceToolSpec {
        name,
        group,
        maps,
        review,
    }
}

const fn cmd(c: &'static [&'static str]) -> Maps {
    Maps::Commands(c)
}

const VOICE: Maps = Maps::VoiceOnly;

static TOOLS: &[VoiceToolSpec] = &[
    // Navigation
    tool(
        "switch_mode",
        G::Navigation,
        cmd(&["mode.agent", "mode.code", "mode.thread"]),
        Plain,
    ),
    tool(
        "open_settings",
        G::Navigation,
        cmd(&["settings.open"]),
        Plain,
    ),
    tool(
        "toggle_sidebar",
        G::Navigation,
        cmd(&["rail.toggle", "sidebar.toggle"]),
        Plain,
    ),
    tool(
        "open_sidebar_tab",
        G::Navigation,
        cmd(&["sidebar.toggle", "sidebar.files"]),
        Plain,
    ),
    tool(
        "open_launch_wizard",
        G::Navigation,
        cmd(&["workspace.launch"]),
        Plain,
    ),
    tool(
        "open_routines",
        G::Navigation,
        cmd(&["routines.open"]),
        Plain,
    ),
    tool("dismiss", G::Navigation, VOICE, Plain),
    // Awareness
    tool("list_workspaces", G::Awareness, VOICE, Plain),
    tool("list_panes", G::Awareness, VOICE, Plain),
    tool("list_threads", G::Awareness, VOICE, Plain),
    tool("list_teammates", G::Awareness, VOICE, Plain),
    tool(
        "get_waiting",
        G::Awareness,
        cmd(&["notifications.open"]),
        Plain,
    ),
    // Panes and agents
    tool("launch_agents", G::Panes, cmd(&["workspace.launch"]), Plain),
    tool("prompt_pane", G::Panes, VOICE, Plain),
    tool("prompt_panes", G::Panes, VOICE, Plain),
    tool("read_pane", G::Panes, VOICE, Plain),
    tool("wait_for_pane", G::Panes, VOICE, Plain),
    tool(
        "focus_pane",
        G::Panes,
        cmd(&[
            "pane.focus_left",
            "pane.focus_right",
            "pane.focus_up",
            "pane.focus_down",
        ]),
        Plain,
    ),
    tool("close_pane", G::Panes, cmd(&["pane.close"]), VoiceOrScreen),
    tool("tidy", G::Panes, cmd(&["workspace.arrange"]), Plain),
    tool("flip", G::Panes, cmd(&["mode.code", "mode.thread"]), Plain),
    tool("handoff", G::Panes, VOICE, Plain),
    tool("run_command_in_shell", G::Panes, VOICE, ScreenOnly),
    // Threads
    tool("open_thread", G::Threads, cmd(&["mode.thread"]), Plain),
    tool("new_thread", G::Threads, cmd(&["mode.thread"]), Plain),
    tool("send_to_thread", G::Threads, VOICE, Plain),
    tool("read_thread", G::Threads, VOICE, Plain),
    tool("stop_thread", G::Threads, VOICE, Plain),
    tool("set_thread_mode", G::Threads, VOICE, Plain),
    tool("rename_thread", G::Threads, VOICE, Plain),
    tool("archive_thread", G::Threads, VOICE, Plain),
    // Teammates
    tool("open_teammate", G::Teammates, cmd(&["mode.agent"]), Plain),
    tool("message_teammate", G::Teammates, VOICE, Plain),
    tool(
        "create_teammate",
        G::Teammates,
        cmd(&["teammate.new"]),
        VoiceOrScreen,
    ),
    tool("pin_teammate", G::Teammates, VOICE, Plain),
    tool("read_teammate_memory", G::Teammates, VOICE, Plain),
    tool(
        "list_routines",
        G::Teammates,
        cmd(&["routines.open"]),
        Plain,
    ),
    tool("run_routine", G::Teammates, VOICE, Plain),
    // Tasks
    tool("create_task", G::Tasks, cmd(&["tasks.open"]), Plain),
    tool(
        "execute_task",
        G::Tasks,
        cmd(&["tasks.open"]),
        VoiceOrScreen,
    ),
    tool("list_tasks", G::Tasks, cmd(&["tasks.open"]), Plain),
    // Find
    tool("find_work", G::Find, cmd(&["work.find"]), Plain),
    // App
    tool("get_setting", G::App, cmd(&["settings.open"]), Plain),
    tool("set_setting", G::App, cmd(&["settings.open"]), Plain),
    tool("get_usage", G::App, VOICE, Plain),
    tool("get_credits", G::App, cmd(&["billing.plans"]), Plain),
    tool("check_updates", G::App, VOICE, Plain),
];

pub fn catalog() -> &'static [VoiceToolSpec] {
    TOOLS
}

pub fn find(name: &str) -> Option<&'static VoiceToolSpec> {
    TOOLS.iter().find(|t| t.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn api_catalog() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../apps/api/voice-tools.json"
        );
        let text = std::fs::read_to_string(path).expect("apps/api/voice-tools.json");
        serde_json::from_str(&text).expect("json")
    }

    #[test]
    fn f_cn_02_1_every_tool_maps_to_a_registry_command_or_is_voice_only() {
        let mut names = BTreeSet::new();
        for t in catalog() {
            assert!(names.insert(t.name), "duplicate tool {}", t.name);
            if let Maps::Commands(ids) = t.maps {
                assert!(!ids.is_empty(), "{} maps to nothing", t.name);
                for id in ids {
                    assert!(
                        crate::commands::find(id).is_some(),
                        "{} maps to {id}, which isn't a registry command",
                        t.name
                    );
                }
            }
        }
    }

    #[test]
    fn f_cn_02_1_the_doer_catalog_is_this_catalog() {
        let api = api_catalog();
        let theirs: BTreeSet<String> = api["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .map(|t| t["name"].as_str().expect("name").to_string())
            .collect();
        let ours: BTreeSet<String> = catalog().iter().map(|t| t.name.to_string()).collect();
        assert_eq!(theirs, ours);
        for t in api["tools"].as_array().expect("tools") {
            let name = t["name"].as_str().expect("name");
            let spec = find(name).expect("known");
            let group = serde_json::to_value(spec.group).expect("group");
            assert_eq!(t["group"], group, "{name}'s group");
            // The doer asks first for exactly the tools this side asks for.
            let asks = t["confirm"].as_bool().unwrap_or(false);
            assert_eq!(asks, spec.review != Review::None, "{name}'s confirmation");
        }
    }

    #[test]
    fn f_cn_02_2_the_settable_list_is_the_closed_list() {
        let api = api_catalog();
        let theirs: BTreeSet<String> = api["settable_settings"]
            .as_array()
            .expect("settable_settings")
            .iter()
            .map(|k| k.as_str().expect("key").to_string())
            .collect();
        let ours: BTreeSet<String> = SETTABLE.iter().map(|k| k.as_str()).collect();
        assert_eq!(theirs, ours);
        assert!(!settable(SettingKey::TelemetryUsage));
        assert!(!settable(SettingKey::LaunchAtLogin));
        assert!(settable(SettingKey::Theme));
    }

    #[test]
    fn f_cn_03_local_review_actions_are_absent() {
        for name in [
            "remove_lane",
            "force_remove_lane",
            "enable_full_access",
            "set_access",
            "approve_plugin",
            "resolve_approval",
        ] {
            assert!(find(name).is_none(), "{name} must not be a voice tool");
        }
        // A shell command is the one tool only a click can approve.
        let screen: Vec<_> = catalog()
            .iter()
            .filter(|t| t.review == Review::ScreenOnly)
            .map(|t| t.name)
            .collect();
        assert_eq!(screen, ["run_command_in_shell"]);
    }
}
