//! Command registry metadata (F-SH-09). Every user-reachable action is a
//! `CommandSpec`; the palette, menus, the shortcut dispatcher, the docs'
//! shortcut table (`packages/protocol/shortcuts.json`, `pnpm gen:shortcuts`)
//! and later the Conductor tool catalog all read this one list. Handlers are
//! bound in the UI. The list grows with each milestone: a command is listed
//! once something runs it.
//!
//! Shortcuts use `Mod` for ⌘ on macOS and Ctrl elsewhere (UI_SPEC §5).

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
    Help,
}

/// Named predicates the UI evaluates to enable or hide a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum EnabledWhen {
    Always,
    CodeMode,
    WorkspaceOpen,
    PaneFocused,
    /// Thread mode shows a thread.
    ThreadOpen,
    /// A focused pane (Code mode), or a cell of the thread grid (Thread mode).
    PaneOrCell,
    /// The plan has routines (F-RT-04); otherwise the command is absent.
    Routines,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CommandSpec {
    /// Also names the title in the UI catalog: `cmd.<id>`.
    pub id: &'static str,
    pub category: CommandCategory,
    /// macOS binding.
    pub mac: Option<&'static str>,
    /// Windows and Linux binding.
    pub pc: Option<&'static str>,
    /// Windows and Linux with a terminal focused under "Terminal first":
    /// the terminal-safe alias, or none when the key goes to the PTY.
    pub pc_terminal: Option<&'static str>,
    /// Also fires while a text field or dialog has focus (palette,
    /// settings, mode switch).
    pub in_text: bool,
    pub enabled_when: EnabledWhen,
}

use CommandCategory as C;
use EnabledWhen as E;

/// A command whose binding is the same everywhere, terminals included.
const fn same(
    id: &'static str,
    category: CommandCategory,
    keys: Option<&'static str>,
    enabled_when: EnabledWhen,
) -> CommandSpec {
    CommandSpec {
        id,
        category,
        mac: keys,
        pc: keys,
        pc_terminal: keys,
        in_text: false,
        enabled_when,
    }
}

/// A command with a terminal-safe alias (or `None`: the key reaches the PTY).
const fn aliased(
    id: &'static str,
    category: CommandCategory,
    keys: &'static str,
    pc_terminal: Option<&'static str>,
    enabled_when: EnabledWhen,
) -> CommandSpec {
    CommandSpec {
        id,
        category,
        mac: Some(keys),
        pc: Some(keys),
        pc_terminal,
        in_text: false,
        enabled_when,
    }
}

const fn in_text(mut c: CommandSpec) -> CommandSpec {
    c.in_text = true;
    c
}

static REGISTRY: &[CommandSpec] = &[
    in_text(same("palette.open", C::App, Some("Mod+K"), E::Always)),
    in_text(same("settings.open", C::App, Some("Mod+,"), E::Always)),
    in_text(same("mode.agent", C::Navigation, Some("Mod+1"), E::Always)),
    in_text(same("mode.code", C::Navigation, Some("Mod+2"), E::Always)),
    in_text(same("mode.thread", C::Navigation, Some("Mod+3"), E::Always)),
    // F-CN-05: the palette's "Find work…".
    same("work.find", C::Navigation, None, E::Always),
    in_text(same(
        "thread.changes",
        C::Navigation,
        Some("Mod+Alt+C"),
        E::ThreadOpen,
    )),
    // F-AG-04: the New teammate sheet (no default shortcut: Mod+Shift+N is
    // Notifications, UI_SPEC UI-70).
    same("teammate.new", C::App, None, E::Always),
    // F-TK-03: Agent mode's Tasks page.
    same("tasks.open", C::Navigation, None, E::Always),
    same("routines.open", C::Navigation, None, E::Always),
    same("routines.pause", C::Navigation, None, E::Routines),
    aliased(
        "rail.toggle",
        C::Navigation,
        "Mod+\\",
        Some("Mod+Shift+\\"),
        E::Always,
    ),
    same(
        "sidebar.toggle",
        C::Navigation,
        Some("Mod+Shift+B"),
        E::Always,
    ),
    // F-CD-19: the sidebar's Files tab (UI_SPEC §5).
    same("sidebar.files", C::Navigation, Some("Mod+Alt+F"), E::Always),
    same(
        "notifications.open",
        C::Navigation,
        Some("Mod+Shift+N"),
        E::Always,
    ),
    same(
        "dashboard.full_view",
        C::Navigation,
        Some("Mod+Alt+Shift+D"),
        E::Always,
    ),
    same("workspace.open_folder", C::Workspace, None, E::Always),
    // The launch sheet (F-CD-14, UI-12).
    same(
        "workspace.launch",
        C::Workspace,
        Some("Mod+Shift+L"),
        E::WorkspaceOpen,
    ),
    aliased(
        "workspace.new_tab",
        C::Workspace,
        "Mod+T",
        None,
        E::WorkspaceOpen,
    ),
    same(
        "workspace.next_tab",
        C::Workspace,
        Some("Ctrl+Tab"),
        E::WorkspaceOpen,
    ),
    same(
        "workspace.prev_tab",
        C::Workspace,
        Some("Ctrl+Shift+Tab"),
        E::WorkspaceOpen,
    ),
    same(
        "workspace.arrange",
        C::Pane,
        Some("Mod+Shift+T"),
        E::PaneFocused,
    ),
    aliased(
        "pane.split_right",
        C::Pane,
        "Mod+D",
        Some("Mod+Shift+E"),
        E::WorkspaceOpen,
    ),
    same(
        "pane.split_down",
        C::Pane,
        Some("Mod+Shift+D"),
        E::WorkspaceOpen,
    ),
    aliased(
        "pane.close",
        C::Pane,
        "Mod+W",
        Some("Mod+Shift+W"),
        E::PaneFocused,
    ),
    same(
        "pane.focus_left",
        C::Pane,
        Some("Mod+Alt+Left"),
        E::PaneOrCell,
    ),
    same(
        "pane.focus_right",
        C::Pane,
        Some("Mod+Alt+Right"),
        E::PaneOrCell,
    ),
    same("pane.focus_up", C::Pane, Some("Mod+Alt+Up"), E::PaneOrCell),
    same(
        "pane.focus_down",
        C::Pane,
        Some("Mod+Alt+Down"),
        E::PaneOrCell,
    ),
    CommandSpec {
        mac: Some("Mod+["),
        pc: Some("Mod+Shift+["),
        pc_terminal: Some("Mod+Shift+["),
        ..same("pane.focus_prev", C::Pane, None, E::PaneFocused)
    },
    CommandSpec {
        mac: Some("Mod+]"),
        pc: Some("Mod+Shift+]"),
        pc_terminal: Some("Mod+Shift+]"),
        ..same("pane.focus_next", C::Pane, None, E::PaneFocused)
    },
    same(
        "pane.maximize",
        C::Pane,
        Some("Mod+Shift+Enter"),
        E::PaneFocused,
    ),
    same("pane.move_to_new_tab", C::Pane, None, E::PaneFocused),
    same("pane.restart", C::Terminal, None, E::PaneFocused),
    aliased(
        "terminal.find",
        C::Terminal,
        "Mod+F",
        Some("Mod+Shift+F"),
        E::PaneFocused,
    ),
    // In a terminal under "Terminal first", Enter / Shift+Enter in the find
    // bar step through matches instead.
    aliased(
        "terminal.find_next",
        C::Terminal,
        "Mod+G",
        None,
        E::PaneFocused,
    ),
    same(
        "terminal.copy_last_output",
        C::Terminal,
        None,
        E::PaneFocused,
    ),
    aliased(
        "terminal.find_prev",
        C::Terminal,
        "Mod+Shift+G",
        None,
        E::PaneFocused,
    ),
    CommandSpec {
        mac: Some("Mod+Down"),
        pc: Some("Ctrl+Shift+End"),
        pc_terminal: Some("Ctrl+Shift+End"),
        ..same("terminal.jump_bottom", C::Terminal, None, E::PaneFocused)
    },
    same("billing.plans", C::App, None, E::Always),
    // The cheatsheet (UI_SPEC §5, UI-113).
    aliased(
        "help.shortcuts",
        C::Help,
        "Mod+/",
        Some("Mod+Shift+/"),
        E::Always,
    ),
    same("help.diagnostics", C::Help, None, E::Always),
    same("help.copy_diagnostics", C::Help, None, E::Always),
    same("help.open_logs", C::Help, None, E::Always),
];

pub fn registry() -> &'static [CommandSpec] {
    REGISTRY
}

pub fn find(id: &str) -> Option<&'static CommandSpec> {
    REGISTRY.iter().find(|c| c.id == id)
}

/// The keys a shortcut can end in, by name (UI_SPEC §5).
const NAMED_KEYS: &[&str] = &[
    "Enter",
    "Tab",
    "Space",
    "Escape",
    "Backspace",
    "Delete",
    "Insert",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "Left",
    "Right",
    "Up",
    "Down",
    "[",
    "]",
    "\\",
    ",",
    ".",
    "/",
    ";",
    "'",
    "`",
    "-",
    "=",
];

fn is_function_key(key: &str) -> bool {
    key.strip_prefix('F')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=24).contains(&n))
}

/// Whether `keys` is a shortcut in the registry's form (F-SH-09 rebinding):
/// modifiers from `Mod`, `Ctrl`, `Alt` and `Shift`, each at most once, then
/// one key. A shortcut needs `Mod`, `Ctrl` or `Alt`, except an F-key alone
/// or with Shift.
pub fn valid_binding(keys: &str) -> bool {
    let parts: Vec<&str> = keys.split('+').collect();
    let Some((key, mods)) = parts.split_last() else {
        return false;
    };
    let mut seen: Vec<&str> = Vec::new();
    for m in mods {
        if !["Mod", "Ctrl", "Alt", "Shift"].contains(m) || seen.contains(m) {
            return false;
        }
        seen.push(m);
    }
    let single = key.len() == 1
        && key
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
    let known = single || is_function_key(key) || NAMED_KEYS.contains(key);
    let held = seen.iter().any(|m| *m != "Shift");
    known && (held || is_function_key(key))
}

/// The Windows/Linux terminal intercept list (UI_SPEC §5): with a terminal
/// focused, Tandem only takes `Ctrl+Shift+…`, `Ctrl+Alt+…`, `Ctrl+K`,
/// `Ctrl+,`, `Ctrl+1..3`, `Ctrl+Tab`/`Ctrl+Shift+Tab` and `F6`.
pub fn terminal_may_intercept(keys: &str) -> bool {
    let parts: Vec<&str> = keys.split('+').collect();
    let has = |m: &str| parts.contains(&m);
    let key = parts.last().copied().unwrap_or("");
    let ctrl = has("Mod") || has("Ctrl");
    key == "F6"
        || (ctrl && (has("Shift") || has("Alt")))
        || (ctrl && parts.len() == 2 && matches!(key, "K" | "," | "1" | "2" | "3" | "Tab"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn f_sh_09_ids_and_bindings_are_unique_per_context() {
        let mut ids = HashSet::new();
        let (mut mac, mut pc, mut term) = (HashSet::new(), HashSet::new(), HashSet::new());
        for c in registry() {
            assert!(ids.insert(c.id), "duplicate id {}", c.id);
            if let Some(k) = c.mac {
                assert!(mac.insert(k), "duplicate macOS shortcut {k}");
            }
            if let Some(k) = c.pc {
                assert!(pc.insert(k), "duplicate shortcut {k}");
            }
            if let Some(k) = c.pc_terminal {
                assert!(term.insert(k), "duplicate terminal shortcut {k}");
            }
        }
        assert!(find("palette.open").is_some());
    }

    #[test]
    fn f_sh_09_1_terminal_bindings_follow_the_passthrough_rule() {
        for c in registry() {
            if let Some(k) = c.pc_terminal {
                assert!(
                    terminal_may_intercept(k),
                    "{} would take {k} from the terminal",
                    c.id
                );
            }
        }
        // Ctrl+W/T/D/[/] reach the shell (F-SH-09 AC1)...
        for k in ["Mod+W", "Mod+T", "Mod+D", "Mod+[", "Mod+]"] {
            assert!(!terminal_may_intercept(k), "{k}");
            assert!(registry().iter().all(|c| c.pc_terminal != Some(k)), "{k}");
        }
        // ...and Ctrl+Shift+D splits (AC2).
        assert_eq!(
            find("pane.split_down").and_then(|c| c.pc_terminal),
            Some("Mod+Shift+D")
        );
    }

    #[test]
    fn f_sh_09_6_a_shortcut_is_modifiers_then_one_key() {
        for c in registry() {
            for k in [c.mac, c.pc, c.pc_terminal].into_iter().flatten() {
                assert!(valid_binding(k), "{}: {k}", c.id);
            }
        }
        for k in [
            "Mod+Alt+R",
            "Ctrl+Shift+Tab",
            "Mod+/",
            "Alt+Up",
            "F2",
            "Shift+F6",
            "Mod+Shift+'",
        ] {
            assert!(valid_binding(k), "{k}");
        }
        // A shortcut needs Mod, Ctrl or Alt (F-keys aside), one key, and no
        // modifier twice.
        for k in [
            "",
            "R",
            "Shift+R",
            "Mod+",
            "Mod+Mod+R",
            "Hyper+R",
            "Mod+Foo",
            "Mod+F25",
            "Mod+r",
            "Mod+R+T",
        ] {
            assert!(!valid_binding(k), "{k}");
        }
    }

    #[test]
    fn f_sh_09_the_cheatsheet_has_a_terminal_safe_alias() {
        let c = find("help.shortcuts").expect("help.shortcuts");
        assert_eq!((c.mac, c.pc), (Some("Mod+/"), Some("Mod+/")));
        assert_eq!(c.pc_terminal, Some("Mod+Shift+/"));
    }

    /// `pnpm gen:shortcuts` → `packages/protocol/shortcuts.json`, the table
    /// the docs and the UI read.
    #[test]
    fn export_shortcuts() {
        let Ok(out) = std::env::var("TANDEM_SHORTCUTS_OUT") else {
            return;
        };
        let json = serde_json::to_string_pretty(registry()).expect("json");
        std::fs::write(out, json + "\n").expect("write shortcuts.json");
    }
}
