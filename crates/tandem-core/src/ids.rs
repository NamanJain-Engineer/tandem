//! Locally generated ids are `<prefix>_<ULID>` (ARCHITECTURE §0.2).

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use ts_rs::TS;

use crate::error::{CoreError, ErrorCode};

/// Generates a fresh `<prefix>_<ULID>` string.
pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", ulid::Ulid::new())
}

/// True when `value` is `<prefix>_` followed by a 26-char Crockford ULID.
pub fn has_prefix(value: &str, prefix: &str) -> bool {
    value
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_prefix('_'))
        .is_some_and(|ulid| ulid.len() == 26 && ulid::Ulid::from_string(ulid).is_ok())
}

macro_rules! prefixed_id {
    ($(#[$meta:meta])* $name:ident, $prefix:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
        #[serde(transparent)]
        #[ts(export)]
        pub struct $name(String);

        impl $name {
            pub const PREFIX: &'static str = $prefix;

            pub fn new() -> Self {
                Self(new_id($prefix))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Wraps an id read back from Tandem's own database.
            pub fn from_db(raw: String) -> Self {
                Self(raw)
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = CoreError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                if has_prefix(s, $prefix) {
                    Ok(Self(s.to_string()))
                } else {
                    Err(CoreError::new(
                        "ids",
                        format!("expected a {} id", $prefix),
                        "pass an id returned by Tandem",
                    )
                    .with_code(ErrorCode::Invalid))
                }
            }
        }
    };
}

prefixed_id!(WorkspaceId, "ws");
prefixed_id!(TabId, "tab");
prefixed_id!(PaneId, "pn");
prefixed_id!(ThreadId, "th");
prefixed_id!(TurnId, "turn");
prefixed_id!(SessionId, "ses");
prefixed_id!(HeldDraftId, "hd");
prefixed_id!(LeaseId, "ls");
prefixed_id!(AttachmentId, "att");
prefixed_id!(PairingId, "pr");
prefixed_id!(TakeId, "take");
prefixed_id!(OpId, "op");
prefixed_id!(TeammateId, "tm");
prefixed_id!(RoutineId, "rt");
prefixed_id!(RunId, "run");
prefixed_id!(AccountId, "acct");
prefixed_id!(ConnectionId, "pc");
prefixed_id!(ApprovalId, "ap");
prefixed_id!(MailId, "ml");
prefixed_id!(AutoRunId, "au");
prefixed_id!(CallId, "call");
prefixed_id!(NotificationId, "ntf");
prefixed_id!(AttemptId, "atm");
prefixed_id!(PresetId, "pst");
prefixed_id!(PhoneQueueId, "pq");

/// `local` is the fixed id of this machine; remote hosts are `host_<ULID>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(transparent)]
#[ts(export)]
pub struct HostId(String);

impl HostId {
    pub fn local() -> Self {
        Self("local".to_string())
    }

    pub fn new_remote() -> Self {
        Self(new_id("host"))
    }

    /// A stored id, trusted as written.
    pub fn from_db(raw: String) -> Self {
        Self(raw)
    }

    pub fn is_local(&self) -> bool {
        self.0 == "local"
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for HostId {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "local" || has_prefix(s, "host") {
            Ok(Self(s.to_string()))
        } else {
            Err(
                CoreError::new("ids", "expected a host id", "pass 'local' or a host id")
                    .with_code(ErrorCode::Invalid),
            )
        }
    }
}

impl fmt::Display for HostId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_carry_prefix_and_round_trip() {
        let id = WorkspaceId::new();
        assert!(id.as_str().starts_with("ws_"));
        assert_eq!(id.as_str().len(), 3 + 26);
        let parsed: WorkspaceId = id.as_str().parse().expect("parse");
        assert_eq!(parsed, id);
    }

    #[test]
    fn wrong_prefix_is_rejected() {
        let pane = PaneId::new();
        assert!(pane.as_str().parse::<WorkspaceId>().is_err());
        assert!("ws_notaulid".parse::<WorkspaceId>().is_err());
    }

    #[test]
    fn host_id_accepts_local() {
        assert!("local".parse::<HostId>().expect("local").is_local());
        let remote = HostId::new_remote();
        assert!(remote.as_str().parse::<HostId>().is_ok());
    }
}
