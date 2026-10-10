//! What the gateway asks of the app for plugin calls (F-PL-06..08): the
//! approval card, held drafts, the grant re-check at execution and "needs
//! attention" when a sign-in stops working.

use async_trait::async_trait;
use serde_json::Value;
use tandem_core::ids::ConnectionId;

use crate::lease::LeaseContext;
use crate::vendors::{ConnectionInfo, Cost, RiskClass};

/// Everything the approval card shows (F-PL-07). No credential is ever in
/// it: `request` is the call's arguments with secret-shaped values masked.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginAsk {
    pub connection_id: ConnectionId,
    pub plugin_id: String,
    pub plugin_label: String,
    pub logo: String,
    pub account_label: Option<String>,
    pub tool: String,
    pub risk_class: RiskClass,
    /// "Create issue in tandem/app".
    pub action: String,
    pub target: Option<String>,
    pub cost: Option<Cost>,
    pub request: Value,
    /// "Allow for this conversation" is offered.
    pub offer_conversation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginVerdict {
    Allow {
        conversation: bool,
    },
    Deny {
        reason: Option<String>,
    },
    /// Nobody answered in 30 minutes: a deny the agent is told about.
    Expired,
    /// The session went away.
    Cancelled,
}

/// An ask-class call from an unattended session, to keep as a draft.
#[derive(Debug, Clone, PartialEq)]
pub struct HeldCall {
    pub connection_id: ConnectionId,
    pub plugin_id: String,
    pub tool: String,
    pub args: Value,
    pub risk_class: RiskClass,
    /// The session's `Attended::Unattended { origin }`.
    pub origin: String,
}

#[async_trait]
pub trait PluginHooks: Send + Sync {
    /// Shows the card and waits for the answer (or the 30-minute expiry).
    async fn ask(&self, lease: &LeaseContext, ask: PluginAsk) -> PluginVerdict;

    /// Stores a held draft; returns its display id (`D-12`).
    async fn hold(&self, lease: &LeaseContext, call: HeldCall) -> Result<String, String>;

    /// Whether the session's subject still has the grant (re-checked at
    /// execution, F-PL-06).
    async fn grant_present(&self, _lease: &LeaseContext, _connection: &ConnectionId) -> bool {
        true
    }

    /// A refresh failed or the vendor refused the credential: the plugin
    /// needs a reconnect (F-PL-04 AC3).
    async fn needs_attention(&self, _connection: &ConnectionInfo) {}
}

/// No app attached: every ask is denied and nothing is held.
pub struct NoPluginHooks;

#[async_trait]
impl PluginHooks for NoPluginHooks {
    async fn ask(&self, _lease: &LeaseContext, _ask: PluginAsk) -> PluginVerdict {
        PluginVerdict::Deny {
            reason: Some("Nobody can approve plugin actions here".into()),
        }
    }

    async fn hold(&self, _lease: &LeaseContext, _call: HeldCall) -> Result<String, String> {
        Err("Held drafts aren't available here".into())
    }
}

/// The arguments as the card shows them: every string through the shared
/// redactor (masks secret-shaped values) and the connection's own secrets.
pub fn mask_request(args: &Value, mask: &dyn Fn(&str) -> String) -> Value {
    match args {
        Value::String(s) => Value::String(mask(&tandem_core::redact::redact(s))),
        Value::Array(items) => Value::Array(items.iter().map(|v| mask_request(v, mask)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), mask_request(v, mask)))
                .collect(),
        ),
        other => other.clone(),
    }
}
