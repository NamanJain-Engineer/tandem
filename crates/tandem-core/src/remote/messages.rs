//! What travels inside sealed frames (F-RM-03, ARCHITECTURE §13.3): the
//! phone's commands, their grants, and the step-up rule for approvals.
//! Local-review actions have no phone command (F-TL-06 rule 5).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// The capabilities a pairing has, each granted on the desktop. View
/// (status, Dashboard, transcripts) comes with pairing; the rest start off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct PhoneGrants {
    /// Send to threads.
    pub chat: bool,
    /// Type into agent panes (never plain shells).
    pub terminal: bool,
    /// Answer approvals (allow still needs local arming).
    pub approvals: bool,
    /// Run a routine now.
    pub routines: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PhoneGrant {
    View,
    Chat,
    Terminal,
    Approvals,
    Routines,
}

impl PhoneGrants {
    pub fn allows(&self, grant: PhoneGrant) -> bool {
        match grant {
            PhoneGrant::View => true,
            PhoneGrant::Chat => self.chat,
            PhoneGrant::Terminal => self.terminal,
            PhoneGrant::Approvals => self.approvals,
            PhoneGrant::Routines => self.routines,
        }
    }
}

/// Every command the phone can send. Nothing here forces a lane removal,
/// enables Full access, runs a shell command or answers a local-review
/// prompt (F-TL-06 rule 5; `tests/invariants/tests/rule_5.rs`).
pub const PHONE_COMMANDS: [&str; 9] = [
    "view.snapshot",
    "thread.read",
    "thread.send",
    "pane.type",
    "approval.decide",
    "routine.run_now",
    "push.register",
    "push.unregister",
    // F-CN-09: the review item and its diff, read only.
    "review.current",
];

/// Where the phone's push token comes from (F-RM-06).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PushPlatform {
    Fcm,
    Apns,
}

impl PushPlatform {
    pub fn as_str(self) -> &'static str {
        match self {
            PushPlatform::Fcm => "fcm",
            PushPlatform::Apns => "apns",
        }
    }
}

/// The most a phone may type or send in one command.
pub const MAX_PHONE_TEXT: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PhoneDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhoneCommand {
    ViewSnapshot,
    ThreadRead {
        thread_id: String,
        before: Option<i64>,
        limit: u32,
    },
    ThreadSend {
        thread_id: String,
        text: String,
    },
    PaneType {
        pane_id: String,
        text: String,
    },
    ApprovalDecide {
        approval_id: String,
        decision: PhoneDecision,
    },
    RoutineRunNow {
        routine_id: String,
    },
    /// Turns push on for this phone: its token and a fresh 32-byte key that
    /// seals every alert (F-RM-06, D-2021).
    PushRegister {
        platform: PushPlatform,
        token: String,
        key: [u8; 32],
    },
    PushUnregister,
    /// The review queue's current item and its diff (F-CN-09), read only.
    ReviewCurrent,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    #[error("unknown command")]
    Unknown,
    #[error("{0}")]
    Invalid(&'static str),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ThreadReadArgs {
    thread_id: String,
    #[serde(default)]
    before: Option<i64>,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ThreadSendArgs {
    thread_id: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PaneTypeArgs {
    pane_id: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ApprovalArgs {
    approval_id: String,
    decision: PhoneDecision,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RoutineArgs {
    routine_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PushArgs {
    platform: PushPlatform,
    token: String,
    key: String,
}

fn args<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, CommandError> {
    let v = if v.is_null() {
        Value::Object(Default::default())
    } else {
        v
    };
    serde_json::from_value(v)
        .map_err(|_| CommandError::Invalid("The command's arguments are invalid."))
}

fn id_ok(id: &str) -> Result<(), CommandError> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if ok {
        Ok(())
    } else {
        Err(CommandError::Invalid("That id isn't valid."))
    }
}

/// Text from the phone may hold line breaks and tabs, never other control
/// characters (an escape sequence typed into a terminal could do anything).
fn text_ok(text: &str) -> Result<(), CommandError> {
    if text.trim().is_empty() {
        return Err(CommandError::Invalid("The text is empty."));
    }
    if text.len() > MAX_PHONE_TEXT {
        return Err(CommandError::Invalid("The text is too long."));
    }
    if text
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(CommandError::Invalid("The text has control characters."));
    }
    Ok(())
}

impl PhoneCommand {
    pub fn parse(cmd: &str, raw: Value) -> Result<Self, CommandError> {
        Ok(match cmd {
            "view.snapshot" => PhoneCommand::ViewSnapshot,
            "thread.read" => {
                let a: ThreadReadArgs = args(raw)?;
                id_ok(&a.thread_id)?;
                PhoneCommand::ThreadRead {
                    thread_id: a.thread_id,
                    before: a.before,
                    limit: a.limit.unwrap_or(50).clamp(1, 200),
                }
            }
            "thread.send" => {
                let a: ThreadSendArgs = args(raw)?;
                id_ok(&a.thread_id)?;
                text_ok(&a.text)?;
                PhoneCommand::ThreadSend {
                    thread_id: a.thread_id,
                    text: a.text,
                }
            }
            "pane.type" => {
                let a: PaneTypeArgs = args(raw)?;
                id_ok(&a.pane_id)?;
                text_ok(&a.text)?;
                PhoneCommand::PaneType {
                    pane_id: a.pane_id,
                    text: a.text,
                }
            }
            "approval.decide" => {
                let a: ApprovalArgs = args(raw)?;
                id_ok(&a.approval_id)?;
                PhoneCommand::ApprovalDecide {
                    approval_id: a.approval_id,
                    decision: a.decision,
                }
            }
            "routine.run_now" => {
                let a: RoutineArgs = args(raw)?;
                id_ok(&a.routine_id)?;
                PhoneCommand::RoutineRunNow {
                    routine_id: a.routine_id,
                }
            }
            "push.register" => {
                let a: PushArgs = args(raw)?;
                let token_ok = a.token.len() >= 8
                    && a.token.len() <= 4096
                    && a.token.chars().all(|c| c.is_ascii_graphic());
                if !token_ok {
                    return Err(CommandError::Invalid("That push token isn't valid."));
                }
                use base64::Engine;
                let key: [u8; 32] = base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(&a.key)
                    .ok()
                    .and_then(|k| k.try_into().ok())
                    .ok_or(CommandError::Invalid("That push key isn't valid."))?;
                PhoneCommand::PushRegister {
                    platform: a.platform,
                    token: a.token,
                    key,
                }
            }
            "push.unregister" => PhoneCommand::PushUnregister,
            "review.current" => PhoneCommand::ReviewCurrent,
            _ => return Err(CommandError::Unknown),
        })
    }

    pub fn name(&self) -> &'static str {
        match self {
            PhoneCommand::ViewSnapshot => "view.snapshot",
            PhoneCommand::ThreadRead { .. } => "thread.read",
            PhoneCommand::ThreadSend { .. } => "thread.send",
            PhoneCommand::PaneType { .. } => "pane.type",
            PhoneCommand::ApprovalDecide { .. } => "approval.decide",
            PhoneCommand::RoutineRunNow { .. } => "routine.run_now",
            PhoneCommand::PushRegister { .. } => "push.register",
            PhoneCommand::PushUnregister => "push.unregister",
            PhoneCommand::ReviewCurrent => "review.current",
        }
    }

    pub fn grant(&self) -> PhoneGrant {
        match self {
            PhoneCommand::ViewSnapshot
            | PhoneCommand::ThreadRead { .. }
            | PhoneCommand::ReviewCurrent => PhoneGrant::View,
            PhoneCommand::ThreadSend { .. } => PhoneGrant::Chat,
            PhoneCommand::PaneType { .. } => PhoneGrant::Terminal,
            PhoneCommand::ApprovalDecide { .. } => PhoneGrant::Approvals,
            PhoneCommand::RoutineRunNow { .. } => PhoneGrant::Routines,
            // Push only tells this phone what View already shows it.
            PhoneCommand::PushRegister { .. } | PhoneCommand::PushUnregister => PhoneGrant::View,
        }
    }
}

/// What happens to a phone's approval decision (F-RM-03 AC3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalRoute {
    /// Apply it now.
    Apply,
    /// Hold it in `phone_queue` until someone at the desktop confirms.
    Queue,
}

/// Deny always applies. Allow applies only while the desktop has armed
/// "Allow approvals from phone for the next 8 hours".
pub fn approval_route(decision: PhoneDecision, armed: bool) -> ApprovalRoute {
    match (decision, armed) {
        (PhoneDecision::Deny, _) | (PhoneDecision::Allow, true) => ApprovalRoute::Apply,
        (PhoneDecision::Allow, false) => ApprovalRoute::Queue,
    }
}

/// How long one arming lasts.
pub const ARM_HOURS: i64 = 8;

/// Messages from the phone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "t", rename_all = "snake_case")]
#[ts(export)]
pub enum FromPhone {
    /// Sent on every connection; the desktop answers with `welcome`.
    Hi {
        device: String,
    },
    Cmd {
        #[ts(type = "number")]
        id: u64,
        cmd: String,
        #[serde(default)]
        args: Value,
    },
    RekeyAck {
        eph: String,
    },
    /// The phone unpairs itself; the desktop revokes the pairing.
    Bye,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ResError {
    pub code: String,
    pub message: String,
}

/// Messages to the phone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "t", rename_all = "snake_case")]
#[ts(export)]
pub enum ToPhone {
    Welcome {
        desktop: String,
        grants: PhoneGrants,
        #[serde(
            rename = "armedUntil",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional, rename = "armedUntil")]
        armed_until: Option<i64>,
    },
    Res {
        #[ts(type = "number")]
        id: u64,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        data: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        error: Option<ResError>,
    },
    Rekey {
        eph: String,
    },
    Changed,
    Reject {
        reason: String,
    },
    Bye {
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn f_rm_03_3_allow_without_arming_is_queued_and_deny_always_applies() {
        assert_eq!(
            approval_route(PhoneDecision::Allow, false),
            ApprovalRoute::Queue
        );
        assert_eq!(
            approval_route(PhoneDecision::Allow, true),
            ApprovalRoute::Apply
        );
        assert_eq!(
            approval_route(PhoneDecision::Deny, false),
            ApprovalRoute::Apply
        );
        assert_eq!(
            approval_route(PhoneDecision::Deny, true),
            ApprovalRoute::Apply
        );
    }

    #[test]
    fn f_rm_03_each_command_needs_its_grant_and_view_comes_with_pairing() {
        let none = PhoneGrants::default();
        let all = PhoneGrants {
            chat: true,
            terminal: true,
            approvals: true,
            routines: true,
        };
        for (cmd, args, grant) in [
            ("view.snapshot", json!({}), PhoneGrant::View),
            ("thread.read", json!({"threadId": "th_1"}), PhoneGrant::View),
            ("review.current", json!({}), PhoneGrant::View),
            (
                "thread.send",
                json!({"threadId": "th_1", "text": "hi"}),
                PhoneGrant::Chat,
            ),
            (
                "pane.type",
                json!({"paneId": "pn_1", "text": "go on"}),
                PhoneGrant::Terminal,
            ),
            (
                "approval.decide",
                json!({"approvalId": "ap_1", "decision": "deny"}),
                PhoneGrant::Approvals,
            ),
            (
                "routine.run_now",
                json!({"routineId": "rt_1"}),
                PhoneGrant::Routines,
            ),
        ] {
            let parsed = PhoneCommand::parse(cmd, args).expect(cmd);
            assert_eq!(parsed.grant(), grant, "{cmd}");
            assert_eq!(parsed.name(), cmd);
            assert!(all.allows(grant));
            assert_eq!(none.allows(grant), grant == PhoneGrant::View, "{cmd}");
        }
    }

    #[test]
    fn f_rm_03_commands_outside_the_set_and_bad_arguments_are_refused() {
        for cmd in [
            "lane.remove",
            "access.full",
            "shell.run",
            "settings.set",
            "",
        ] {
            assert_eq!(
                PhoneCommand::parse(cmd, json!({})),
                Err(CommandError::Unknown),
                "{cmd}"
            );
        }
        assert!(PhoneCommand::parse("thread.send", json!({"threadId": "th_1"})).is_err());
        assert!(
            PhoneCommand::parse("thread.send", json!({"threadId": "../x", "text": "a"})).is_err()
        );
        assert!(
            PhoneCommand::parse("thread.send", json!({"threadId": "th_1", "text": "  "})).is_err()
        );
        assert!(
            PhoneCommand::parse(
                "pane.type",
                json!({"paneId": "pn_1", "text": "rm -rf /\u{1b}[200~"})
            )
            .is_err(),
            "escape sequences never reach a terminal"
        );
        assert!(PhoneCommand::parse(
            "pane.type",
            json!({"paneId": "pn_1", "text": "x".repeat(MAX_PHONE_TEXT + 1)})
        )
        .is_err());
        assert!(
            PhoneCommand::parse(
                "approval.decide",
                json!({"approvalId": "ap_1", "decision": "allow", "scope": "conversation"})
            )
            .is_err(),
            "the phone can't widen an approval's scope"
        );
        assert!(PhoneCommand::parse(
            "thread.send",
            json!({"threadId": "th_1", "text": "line\nnext\tcol"})
        )
        .is_ok());
    }

    #[test]
    fn f_rm_03_messages_have_the_documented_shape() {
        let welcome = serde_json::to_value(ToPhone::Welcome {
            desktop: "Work laptop".into(),
            grants: PhoneGrants::default(),
            armed_until: Some(5),
        })
        .expect("json");
        assert_eq!(
            welcome,
            json!({"t": "welcome", "desktop": "Work laptop", "armedUntil": 5,
                   "grants": {"chat": false, "terminal": false, "approvals": false, "routines": false}})
        );
        assert_eq!(
            serde_json::to_value(ToPhone::Changed).expect("json"),
            json!({"t": "changed"})
        );
        let cmd: FromPhone =
            serde_json::from_value(json!({"t": "cmd", "id": 3, "cmd": "view.snapshot"}))
                .expect("cmd");
        assert_eq!(
            cmd,
            FromPhone::Cmd {
                id: 3,
                cmd: "view.snapshot".into(),
                args: Value::Null
            }
        );
    }
}
