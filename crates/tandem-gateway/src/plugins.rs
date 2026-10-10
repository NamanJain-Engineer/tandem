//! Plugin tools over a lease (ARCHITECTURE §11.3, F-PL-01): only the
//! connections the lease was granted, by default behind
//! `tandem__search_tools` + `tandem__call`, or listed one by one when "Expose
//! all plugin tools directly" is on. A vendor that fails only takes its own
//! tools away.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use rmcp::model::Tool;
use serde_json::{json, Value};
use tandem_core::ids::ConnectionId;

use crate::builtins;
use crate::lease::LeaseContext;
use crate::vendors::{self, ConnectionInfo, RiskClass, ToolDef, ToolOutput, VendorModule};

/// The connections the app has, as the gateway sees them (no secrets).
#[derive(Default)]
pub(crate) struct Connections {
    by_id: RwLock<HashMap<ConnectionId, ConnectionInfo>>,
    /// Bumped by every change: in-flight leases see it on their next call.
    generation: AtomicU64,
}

impl Connections {
    pub(crate) fn replace(&self, all: Vec<ConnectionInfo>) {
        let mut map = match self.by_id.write() {
            Ok(m) => m,
            Err(p) => p.into_inner(),
        };
        *map = all.into_iter().map(|c| (c.id.clone(), c)).collect();
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    pub(crate) fn remove(&self, id: &ConnectionId) {
        let mut map = match self.by_id.write() {
            Ok(m) => m,
            Err(p) => p.into_inner(),
        };
        map.remove(id);
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    fn get(&self, id: &ConnectionId) -> Option<ConnectionInfo> {
        let map = match self.by_id.read() {
            Ok(m) => m,
            Err(p) => p.into_inner(),
        };
        map.get(id).cloned()
    }

    fn any_of_plugin(&self, plugin: &str) -> bool {
        let map = match self.by_id.read() {
            Ok(m) => m,
            Err(p) => p.into_inner(),
        };
        map.values().any(|c| c.plugin_id == plugin && c.connected)
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
}

pub(crate) struct Plugins {
    pub(crate) modules: Vec<Arc<dyn VendorModule>>,
    pub(crate) connections: Connections,
    pub(crate) expose_all: AtomicBool,
}

fn text(s: impl Into<String>) -> ToolOutput {
    ToolOutput {
        text: s.into(),
        is_error: false,
    }
}

fn error(s: impl Into<String>) -> ToolOutput {
    ToolOutput {
        text: s.into(),
        is_error: true,
    }
}

fn risk_name(r: RiskClass) -> &'static str {
    match r {
        RiskClass::Read => "read",
        RiskClass::Write => "write",
        RiskClass::ExternalPublish => "external_publish",
        RiskClass::Spend => "spend",
        RiskClass::CreditConsuming => "credit_consuming",
        RiskClass::Destructive => "destructive",
        RiskClass::Unclassified => "unclassified",
    }
}

impl Plugins {
    pub(crate) fn new(modules: Vec<Arc<dyn VendorModule>>) -> Self {
        Self {
            modules,
            connections: Connections::default(),
            expose_all: AtomicBool::new(false),
        }
    }

    /// The lease's granted connections that are still connected, each with
    /// its module (checked on every call: a disconnect counts at once).
    fn granted(&self, lease: &LeaseContext) -> Vec<(ConnectionInfo, Arc<dyn VendorModule>)> {
        let mut out = Vec::new();
        for id in &lease.session.granted {
            let Some(conn) = self.connections.get(id) else {
                continue;
            };
            if !conn.connected {
                continue;
            }
            if let Some(module) = vendors::find(&self.modules, &conn.plugin_id) {
                out.push((conn, module));
            }
        }
        out
    }

    /// What `tools/list` shows for the lease's plugins.
    pub(crate) async fn list(&self, lease: &LeaseContext) -> Vec<Tool> {
        let granted = self.granted(lease);
        if granted.is_empty() {
            return Vec::new();
        }
        if !self.expose_all.load(Ordering::SeqCst) {
            return vec![builtins::search_tool(), builtins::call_tool()];
        }
        let mut tools = Vec::new();
        for (conn, module) in granted {
            match module.tools(&conn).await {
                Ok(defs) => tools.extend(defs.into_iter().map(|d| {
                    builtins::tool(
                        &format!("{}__{}", conn.plugin_id, d.name),
                        &d.description,
                        d.input_schema,
                    )
                })),
                Err(e) => {
                    tracing::warn!(code = "gateway.plugin_unavailable", plugin = %conn.plugin_id, error = %e, "a plugin's tools aren't available");
                }
            }
        }
        tools
    }

    /// `tandem__search_tools`: the granted tools whose name or description
    /// holds every word of the query (an empty query lists them all).
    pub(crate) async fn search(&self, lease: &LeaseContext, args: &Value) -> ToolOutput {
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_lowercase();
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(10)
            .clamp(1, 20) as usize;
        let words: Vec<&str> = query.split_whitespace().collect();
        let mut found = Vec::new();
        let mut unavailable = Vec::new();
        for (conn, module) in self.granted(lease) {
            match module.tools(&conn).await {
                Ok(defs) => {
                    for d in defs {
                        let name = format!("{}__{}", conn.plugin_id, d.name);
                        let hay = format!("{} {}", name, d.description).to_lowercase();
                        if words.iter().all(|w| hay.contains(w)) {
                            found.push(json!({
                                "name": name,
                                "description": d.description,
                                "risk_class": risk_name(d.risk_class),
                                "input_schema": d.input_schema,
                            }));
                        }
                    }
                }
                Err(e) => unavailable.push(json!({
                    "plugin": conn.plugin_id,
                    "error": e.to_string(),
                })),
            }
        }
        found.truncate(limit);
        let mut out = json!({ "tools": found });
        if !unavailable.is_empty() {
            out["unavailable"] = Value::Array(unavailable);
        }
        text(out.to_string())
    }

    /// Runs `<plugin>__<tool>` for the lease, if it was granted that plugin.
    pub(crate) async fn call(&self, lease: &LeaseContext, name: &str, args: Value) -> ToolOutput {
        let Some((plugin, tool)) = name.split_once("__") else {
            return error(format!("Unknown tool: {name}"));
        };
        let granted = self.granted(lease);
        let Some((conn, module)) = granted.into_iter().find(|(c, _)| c.plugin_id == plugin) else {
            // Connected for someone else, or not at all: never run it here.
            return if self.connections.any_of_plugin(plugin) {
                error(format!(
                    "The {plugin} plugin isn't enabled for this session"
                ))
            } else {
                error(format!("No plugin named {plugin} is connected"))
            };
        };
        let defs: Vec<ToolDef> = match module.tools(&conn).await {
            Ok(d) => d,
            Err(e) => return error(e.to_string()),
        };
        let Some(def) = defs.iter().find(|d| d.name == tool) else {
            return error(format!(
                "{} has no tool named {tool}",
                module.manifest().label
            ));
        };
        // Writes, spending and the rest ask first (F-PL-06), which arrives
        // with plugin approvals; until then only reads run.
        if def.risk_class != RiskClass::Read {
            return error(format!(
                "{name} needs the user's approval, and this version of Vibezeo can't ask for plugin approvals yet"
            ));
        }
        match module.call(&conn, tool, args).await {
            Ok(out) => out,
            Err(e) => error(e.to_string()),
        }
    }
}
