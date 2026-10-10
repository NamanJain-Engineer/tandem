//! Vendor modules (ARCHITECTURE §11.4, F-PL-02): plugins are code, one module
//! per vendor under `vendors/<id>/`. Each declares what it is, how it signs
//! in, the hosts its credentials may go to and its tools with their risk
//! classes. The catalog's real vendors, credentials and connect flows arrive
//! with M12; M10 has the interface, the registry and the test modules
//! (behind the `test-vendors` feature).

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(any(test, feature = "test-vendors"))]
pub mod test_echo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Dev,
    Business,
    Communication,
    Research,
    Media,
    Growth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    OauthDcr,
    OauthStatic,
    ApiKey,
    LocalPort,
    None,
}

impl AuthKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthKind::OauthDcr => "oauth_dcr",
            AuthKind::OauthStatic => "oauth_static",
            AuthKind::ApiKey => "api_key",
            AuthKind::LocalPort => "local_port",
            AuthKind::None => "none",
        }
    }
}

/// What a tool may do (F-PL-06); decides whether a call asks first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskClass {
    Read,
    Write,
    ExternalPublish,
    Spend,
    CreditConsuming,
    Destructive,
    Unclassified,
}

/// A host a module's credentials may be sent to: scheme, host, port and
/// path prefix must all match (F-PL-04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedHost {
    pub scheme: &'static str,
    pub host: &'static str,
    pub port: u16,
    pub path_prefix: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleStatus {
    Ready,
    /// Listed, but can't be connected.
    Preview,
}

#[derive(Debug, Clone)]
pub struct VendorManifest {
    pub id: &'static str,
    pub label: &'static str,
    pub category: Category,
    pub description: &'static str,
    pub auth: AuthKind,
    pub pinned: Vec<PinnedHost>,
    pub status: ModuleStatus,
}

/// One tool of a module, before the gateway names it `<plugin>__<tool>`.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub risk_class: RiskClass,
}

/// What a call returns: text for the engine, or an error it can read.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutput {
    pub text: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VendorError {
    /// The vendor can't be reached or the connection needs attention; the
    /// other plugins keep working (F-PL-01).
    #[error("{0}")]
    Unavailable(String),
    #[error("{0}")]
    UnknownTool(String),
    #[error("{0}")]
    Invalid(String),
}

/// One connected account of a module, as the gateway knows it (no secrets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionInfo {
    pub id: tandem_core::ids::ConnectionId,
    pub plugin_id: String,
    pub account_label: Option<String>,
    pub connected: bool,
}

#[async_trait]
pub trait VendorModule: Send + Sync {
    fn manifest(&self) -> &VendorManifest;
    /// The module's tools for a connection (a remote MCP upstream discovers
    /// them).
    async fn tools(&self, conn: &ConnectionInfo) -> Result<Vec<ToolDef>, VendorError>;
    async fn call(
        &self,
        conn: &ConnectionInfo,
        tool: &str,
        args: Value,
    ) -> Result<ToolOutput, VendorError>;
}

/// Every module this build has.
pub fn registry() -> Vec<Arc<dyn VendorModule>> {
    #[allow(unused_mut)]
    let mut modules: Vec<Arc<dyn VendorModule>> = Vec::new();
    #[cfg(any(test, feature = "test-vendors"))]
    modules.extend(test_echo::modules());
    modules
}

/// The module for a plugin id.
pub fn find(modules: &[Arc<dyn VendorModule>], plugin_id: &str) -> Option<Arc<dyn VendorModule>> {
    modules
        .iter()
        .find(|m| m.manifest().id == plugin_id)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_pl_02_1_every_module_pins_its_hosts_and_classes_each_static_tool() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        for module in registry() {
            let m = module.manifest();
            assert!(!m.pinned.is_empty(), "{} pins no host", m.id);
            assert!(
                m.id.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "{}: plugin ids are snake_case",
                m.id
            );
            assert_ne!(m.id, crate::BUILTIN_PLUGIN, "the tandem id is reserved");
            let conn = ConnectionInfo {
                id: tandem_core::ids::ConnectionId::new(),
                plugin_id: m.id.to_string(),
                account_label: None,
                connected: true,
            };
            if let Ok(tools) = rt.block_on(module.tools(&conn)) {
                for t in tools {
                    assert!(
                        !t.name.contains("__"),
                        "{}: tool names can't hold __",
                        t.name
                    );
                    assert!(t.input_schema.is_object(), "{}: schema", t.name);
                }
            }
        }
    }
}
