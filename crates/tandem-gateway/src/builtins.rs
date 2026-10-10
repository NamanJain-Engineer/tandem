//! The gateway's built-in tools (ARCHITECTURE §0.3, §11.2): the plugin id
//! `tandem` is reserved for them, so they're named `tandem__<tool>`. Which
//! ones a session sees is its lease's [`BuiltinSet`]; the app answers them
//! through a [`BuiltinHandler`] scoped by the lease's caller.

use std::sync::Arc;

use async_trait::async_trait;
use rmcp::model::Tool;
use serde_json::{json, Value};

use crate::lease::{BuiltinSet, LeaseContext};
use crate::vendors::ToolOutput;

/// The reserved plugin id.
pub const BUILTIN_PLUGIN: &str = "tandem";
pub const SEARCH_TOOLS: &str = "tandem__search_tools";
pub const CALL: &str = "tandem__call";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Builtin {
    /// F-AG-11.
    SessionSearch,
    /// F-AG-07.
    MemoryWrite,
    /// F-AG-06, F-AG-09.
    ReadSkill,
    /// F-AG-12.
    SendMessage,
    /// F-AG-12.
    ListTeammates,
}

/// One built-in: its wire name, what the engine is told, its input schema
/// and the lease flag that turns it on.
pub struct BuiltinTool {
    pub builtin: Builtin,
    pub name: &'static str,
    pub description: &'static str,
    schema: fn() -> Value,
    enabled: fn(&BuiltinSet) -> bool,
}

impl BuiltinTool {
    pub fn schema(&self) -> Value {
        (self.schema)()
    }

    pub fn enabled(&self, set: &BuiltinSet) -> bool {
        (self.enabled)(set)
    }

    pub(crate) fn tool(&self) -> Tool {
        tool(self.name, self.description, self.schema())
    }
}

/// The built-ins registry. The schedule tools join it with routines (M11).
pub static BUILTINS: &[BuiltinTool] = &[
    BuiltinTool {
        builtin: Builtin::SessionSearch,
        name: "tandem__session_search",
        description: "Searches your past chats with the user (their messages and your replies). Returns the matching chats, most recent first.",
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Words to look for."},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 10}
                },
                "required": ["query"]
            })
        },
        enabled: |s| s.session_search,
    },
    BuiltinTool {
        builtin: Builtin::MemoryWrite,
        name: "tandem__memory_write",
        description: "Replaces one of your memory files with new Markdown: \"notes\" (NOTES.md) or \"about\" (ABOUT_YOU.md, what you know about the user). Keep it within its budget and never store credentials.",
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "file": {"type": "string", "enum": ["notes", "about"]},
                    "markdown": {"type": "string"}
                },
                "required": ["file", "markdown"]
            })
        },
        enabled: |s| s.memory_write,
    },
    BuiltinTool {
        builtin: Builtin::ReadSkill,
        name: "tandem__read_skill",
        description: "Loads the full text of one of your skills by its name from the skills index.",
        schema: || {
            json!({
                "type": "object",
                "properties": {"slug": {"type": "string"}},
                "required": ["slug"]
            })
        },
        enabled: |s| s.read_skill,
    },
    BuiltinTool {
        builtin: Builtin::SendMessage,
        name: "tandem__send_message",
        description: "Sends a message to another teammate by name. They pick it up in a chat of their own.",
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "to": {"type": "string", "description": "The teammate's name."},
                    "body": {"type": "string"}
                },
                "required": ["to", "body"]
            })
        },
        enabled: |s| s.mail,
    },
    BuiltinTool {
        builtin: Builtin::ListTeammates,
        name: "tandem__list_teammates",
        description: "Lists the teammates you can message.",
        schema: || json!({"type": "object", "properties": {}}),
        enabled: |s| s.mail,
    },
];

/// The built-in a wire name stands for, if the lease has it.
pub fn find(name: &str, set: &BuiltinSet) -> Option<&'static BuiltinTool> {
    BUILTINS.iter().find(|b| b.name == name && b.enabled(set))
}

/// Answers the built-ins for the app. Every call is scoped by the lease that
/// made it (its caller, access and conversation).
#[async_trait]
pub trait BuiltinHandler: Send + Sync {
    async fn call(&self, lease: &LeaseContext, builtin: Builtin, args: Value) -> ToolOutput;
}

/// A handler that has nothing to answer with (tests, and before the app
/// binds its services).
pub struct NoBuiltins;

#[async_trait]
impl BuiltinHandler for NoBuiltins {
    async fn call(&self, _lease: &LeaseContext, _builtin: Builtin, _args: Value) -> ToolOutput {
        ToolOutput {
            text: "This tool isn't available right now".into(),
            is_error: true,
        }
    }
}

pub(crate) fn tool(name: &str, description: &str, schema: Value) -> Tool {
    let schema = match schema {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    Tool::new(name.to_string(), description.to_string(), Arc::new(schema))
}

pub(crate) fn search_tool() -> Tool {
    tool(
        SEARCH_TOOLS,
        "Finds tools of the plugins enabled for you. Returns their names, what they do, their risk class and input schema; run one with tandem__call.",
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 20}
            },
            "required": ["query"]
        }),
    )
}

pub(crate) fn call_tool() -> Tool {
    tool(
        CALL,
        "Runs a plugin tool found with tandem__search_tools.",
        json!({
            "type": "object",
            "properties": {
                "tool": {"type": "string", "description": "<plugin>__<tool>, as tandem__search_tools named it."},
                "args": {"type": "object"}
            },
            "required": ["tool"]
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_pl_01_built_ins_are_named_under_the_reserved_plugin_id() {
        for b in BUILTINS {
            assert!(b.name.starts_with("tandem__"), "{}", b.name);
            assert!(b.schema().is_object());
        }
        let teammate = BuiltinSet::for_teammate(true, false);
        assert!(find("tandem__send_message", &teammate).is_some());
        let quiet = BuiltinSet::for_teammate(false, false);
        assert!(find("tandem__send_message", &quiet).is_none());
        assert!(find("tandem__list_teammates", &quiet).is_none());
        assert!(find("tandem__memory_write", &BuiltinSet::for_thread()).is_none());
    }
}
