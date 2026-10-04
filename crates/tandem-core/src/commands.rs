//! Command registry metadata (F-SH-09 core). Every user-reachable action is a
//! `CommandSpec`; the palette, menus, shortcuts table, docs and the Conductor
//! tool catalog are all generated from this list.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CommandCategory {
    App,
    Navigation,
    Workspace,
    Pane,
    Terminal,
    Thread,
    Teammate,
    Dictate,
    Conductor,
    Help,
}

/// Named predicates the UI evaluates to enable or hide a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum EnabledWhen {
    Always,
    CodeMode,
    ThreadMode,
    AgentMode,
    PaneFocused,
    TerminalFocused,
    ThreadFocused,
    WorkspaceOpen,
    SignedIn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CommandSpec {
    pub id: &'static str,
    /// i18n catalog key for the title.
    pub title_key: &'static str,
    pub category: CommandCategory,
    /// `Mod` = ⌘ on macOS and Ctrl elsewhere.
    pub default_shortcut: Option<&'static str>,
    /// Works when Tandem isn't focused.
    pub global: bool,
    pub enabled_when: EnabledWhen,
    /// Conductor may run this command by voice.
    pub voice: bool,
}

const fn cmd(
    id: &'static str,
    title_key: &'static str,
    category: CommandCategory,
    default_shortcut: Option<&'static str>,
    enabled_when: EnabledWhen,
) -> CommandSpec {
    CommandSpec {
        id,
        title_key,
        category,
        default_shortcut,
        global: false,
        enabled_when,
        voice: false,
    }
}

use CommandCategory as C;
use EnabledWhen as E;

static REGISTRY: &[CommandSpec] = &[
    cmd(
        "palette.open",
        "cmd.palette.open",
        C::App,
        Some("Mod+K"),
        E::Always,
    ),
    cmd(
        "settings.open",
        "cmd.settings.open",
        C::App,
        Some("Mod+,"),
        E::Always,
    ),
    cmd(
        "mode.agent",
        "cmd.mode.agent",
        C::Navigation,
        Some("Mod+1"),
        E::Always,
    ),
    cmd(
        "mode.code",
        "cmd.mode.code",
        C::Navigation,
        Some("Mod+2"),
        E::Always,
    ),
    cmd(
        "mode.thread",
        "cmd.mode.thread",
        C::Navigation,
        Some("Mod+3"),
        E::Always,
    ),
    cmd(
        "rail.toggle",
        "cmd.rail.toggle",
        C::Navigation,
        Some("Mod+\\"),
        E::Always,
    ),
    cmd(
        "sidebar.toggle",
        "cmd.sidebar.toggle",
        C::Navigation,
        Some("Mod+Alt+B"),
        E::Always,
    ),
    cmd(
        "notifications.open",
        "cmd.notifications.open",
        C::Navigation,
        Some("Mod+Shift+N"),
        E::Always,
    ),
    cmd(
        "dashboard.open",
        "cmd.dashboard.open",
        C::Navigation,
        Some("Mod+Shift+D"),
        E::Always,
    ),
    cmd(
        "workspace.open_folder",
        "cmd.workspace.open_folder",
        C::Workspace,
        Some("Mod+O"),
        E::Always,
    ),
    cmd(
        "workspace.new_tab",
        "cmd.workspace.new_tab",
        C::Workspace,
        Some("Mod+T"),
        E::WorkspaceOpen,
    ),
    cmd(
        "workspace.launch_several",
        "cmd.workspace.launch_several",
        C::Workspace,
        Some("Mod+Shift+L"),
        E::WorkspaceOpen,
    ),
    cmd(
        "workspace.arrange",
        "cmd.workspace.arrange",
        C::Workspace,
        Some("Mod+Shift+T"),
        E::CodeMode,
    ),
    cmd(
        "pane.split_right",
        "cmd.pane.split_right",
        C::Pane,
        Some("Mod+D"),
        E::PaneFocused,
    ),
    cmd(
        "pane.split_down",
        "cmd.pane.split_down",
        C::Pane,
        Some("Mod+Shift+E"),
        E::PaneFocused,
    ),
    cmd(
        "pane.close",
        "cmd.pane.close",
        C::Pane,
        Some("Mod+W"),
        E::PaneFocused,
    ),
    cmd(
        "pane.maximize",
        "cmd.pane.maximize",
        C::Pane,
        Some("Mod+Shift+Enter"),
        E::PaneFocused,
    ),
    cmd(
        "pane.focus_next",
        "cmd.pane.focus_next",
        C::Pane,
        Some("Mod+]"),
        E::CodeMode,
    ),
    cmd(
        "pane.focus_prev",
        "cmd.pane.focus_prev",
        C::Pane,
        Some("Mod+["),
        E::CodeMode,
    ),
    cmd(
        "terminal.find",
        "cmd.terminal.find",
        C::Terminal,
        Some("Mod+F"),
        E::TerminalFocused,
    ),
    cmd(
        "terminal.clear",
        "cmd.terminal.clear",
        C::Terminal,
        Some("Mod+Shift+K"),
        E::TerminalFocused,
    ),
    cmd(
        "thread.new",
        "cmd.thread.new",
        C::Thread,
        Some("Mod+N"),
        E::ThreadMode,
    ),
    cmd(
        "thread.plan_act",
        "cmd.thread.plan_act",
        C::Thread,
        Some("Mod+Shift+P"),
        E::ThreadFocused,
    ),
    cmd(
        "thread.model_picker",
        "cmd.thread.model_picker",
        C::Thread,
        Some("Mod+Shift+O"),
        E::ThreadFocused,
    ),
    cmd(
        "thread.stop",
        "cmd.thread.stop",
        C::Thread,
        Some("Mod+."),
        E::ThreadFocused,
    ),
    cmd(
        "teammate.new",
        "cmd.teammate.new",
        C::Teammate,
        None,
        E::AgentMode,
    ),
    cmd(
        "help.shortcuts",
        "cmd.help.shortcuts",
        C::Help,
        Some("Mod+/"),
        E::Always,
    ),
    cmd(
        "help.copy_diagnostics",
        "cmd.help.copy_diagnostics",
        C::Help,
        None,
        E::Always,
    ),
    cmd(
        "help.open_logs",
        "cmd.help.open_logs",
        C::Help,
        None,
        E::Always,
    ),
];

pub fn registry() -> &'static [CommandSpec] {
    REGISTRY
}

pub fn find(id: &str) -> Option<&'static CommandSpec> {
    REGISTRY.iter().find(|c| c.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_and_shortcuts_are_unique() {
        let mut ids = HashSet::new();
        let mut keys = HashSet::new();
        for c in registry() {
            assert!(ids.insert(c.id), "duplicate id {}", c.id);
            if let Some(k) = c.default_shortcut {
                assert!(keys.insert(k), "duplicate shortcut {k}");
            }
            assert!(c.title_key.starts_with("cmd."));
        }
        assert!(find("palette.open").is_some());
    }
}
