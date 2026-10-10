//! A Team org's admin policy (F-BL-06, ARCHITECTURE §14.4). It arrives in the
//! signed entitlement token's `policy` claim and mirrors
//! `@tandem/protocol/team`'s `OrgPolicy`. The app enforces it (the composer's
//! Full option, engine choice, telemetry toggles); the server refuses hosted
//! features it disables.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::access::Access;

/// The org's rules. Missing fields take the permissive defaults, so a newer
/// server's extra fields and an older one's missing ones both parse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct OrgPolicy {
    /// Engine ids members may run; `None` means every engine.
    pub allowed_engines: Option<Vec<String>>,
    /// Plugin ids members may connect; `None` means every plugin (M12).
    pub allowed_plugins: Option<Vec<String>>,
    /// The highest access level a thread may use.
    pub max_access: Access,
    pub force_telemetry_off: bool,
    pub disable_auto: bool,
    pub disable_conductor: bool,
    pub disable_phone: bool,
    /// The app reports Full-access use and plugin write approvals (F-TL-10).
    pub audit_enabled: bool,
}

impl Default for OrgPolicy {
    fn default() -> Self {
        Self {
            allowed_engines: None,
            allowed_plugins: None,
            max_access: Access::Full,
            force_telemetry_off: false,
            disable_auto: false,
            disable_conductor: false,
            disable_phone: false,
            audit_enabled: false,
        }
    }
}

fn rank(a: Access) -> u8 {
    match a {
        Access::Read => 0,
        Access::Edits => 1,
        Access::Full => 2,
    }
}

impl OrgPolicy {
    /// Whether `access` is within the org's maximum.
    pub fn allows_access(&self, access: Access) -> bool {
        rank(access) <= rank(self.max_access)
    }

    /// `access`, lowered to the org's maximum when it's above it.
    pub fn clamp_access(&self, access: Access) -> Access {
        if self.allows_access(access) {
            access
        } else {
            self.max_access
        }
    }

    pub fn allows_engine(&self, engine_id: &str) -> bool {
        self.allowed_engines
            .as_ref()
            .is_none_or(|list| list.iter().any(|e| e == engine_id))
    }

    pub fn allows_plugin(&self, plugin_id: &str) -> bool {
        self.allowed_plugins
            .as_ref()
            .is_none_or(|list| list.iter().any(|p| p == plugin_id))
    }
}

/// Every check against an optional policy: no policy allows everything.
pub fn allows_access(policy: Option<&OrgPolicy>, access: Access) -> bool {
    policy.is_none_or(|p| p.allows_access(access))
}

pub fn clamp_access(policy: Option<&OrgPolicy>, access: Access) -> Access {
    policy.map_or(access, |p| p.clamp_access(access))
}

pub fn allows_engine(policy: Option<&OrgPolicy>, engine_id: &str) -> bool {
    policy.is_none_or(|p| p.allows_engine(engine_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_bl_06_1_max_access_edits_refuses_full_and_clamps_it() {
        let p = OrgPolicy {
            max_access: Access::Edits,
            ..OrgPolicy::default()
        };
        assert!(!p.allows_access(Access::Full));
        assert!(p.allows_access(Access::Edits));
        assert!(p.allows_access(Access::Read));
        assert_eq!(p.clamp_access(Access::Full), Access::Edits);
        assert_eq!(p.clamp_access(Access::Read), Access::Read);
        let read = OrgPolicy {
            max_access: Access::Read,
            ..OrgPolicy::default()
        };
        assert_eq!(read.clamp_access(Access::Edits), Access::Read);
        assert_eq!(clamp_access(None, Access::Full), Access::Full);
    }

    #[test]
    fn f_bl_06_allowlists_and_no_policy() {
        let p = OrgPolicy {
            allowed_engines: Some(vec!["claude".into()]),
            allowed_plugins: Some(vec![]),
            ..OrgPolicy::default()
        };
        assert!(p.allows_engine("claude"));
        assert!(!p.allows_engine("codex"));
        assert!(!p.allows_plugin("github"));
        assert!(allows_engine(None, "codex"));
        assert!(OrgPolicy::default().allows_plugin("github"));
    }

    #[test]
    fn f_bl_06_parses_the_server_shape_leniently() {
        let p: OrgPolicy = serde_json::from_value(serde_json::json!({
            "allowedEngines": null, "allowedPlugins": ["linear"], "maxAccess": "edits",
            "forceTelemetryOff": true, "disableAuto": false, "disableConductor": true,
            "disablePhone": false, "auditEnabled": true, "somethingNew": 1
        }))
        .expect("parse");
        assert_eq!(p.max_access, Access::Edits);
        assert!(p.force_telemetry_off && p.audit_enabled && p.disable_conductor);
        let partial: OrgPolicy =
            serde_json::from_value(serde_json::json!({ "maxAccess": "read" })).expect("parse");
        assert_eq!(partial.max_access, Access::Read);
        assert_eq!(partial.allowed_engines, None);
    }
}
