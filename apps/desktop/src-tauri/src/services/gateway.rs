//! The app's one MCP gateway (F-PL-01, ARCHITECTURE §11): started once, at
//! launch or on the first session that needs it, shared by every thread and
//! teammate chat. Its port goes to `$DATA/run/gateway.json`; it knows the
//! plugin connections (never their secrets) and "Expose all plugin tools
//! directly"; the app's services answer its built-ins.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock, Weak};

use async_trait::async_trait;
use serde_json::Value;
use tandem_core::settings::SettingKey;
use tandem_core::{ErrorCode, IpcError};
use tandem_gateway::{
    ApprovalAnswer, ApprovalAsker, Builtin, BuiltinHandler, Caller, ConnectionInfo, GatewayHandle,
    GatewayMetrics, LeaseContext, ToolApproval, ToolOutput,
};
use tandem_store::repo::plugins::{self, ConnectionStatus};
use tandem_store::Store;

use crate::services::threads::ThreadService;

/// Answers the `tandem__*` built-ins for a lease's caller. The teammate
/// services register here as they start.
#[async_trait]
pub trait BuiltinService: Send + Sync {
    async fn call(&self, lease: &LeaseContext, builtin: Builtin, args: Value) -> ToolOutput;
}

/// The gateway's view of the app: approvals go to the thread that asked,
/// built-ins to the registered service.
struct AppHooks {
    threads: OnceLock<Weak<ThreadService>>,
    builtins: OnceLock<Weak<dyn BuiltinService>>,
    plugins: OnceLock<crate::services::plugins::Hooks>,
}

struct Asker(Arc<AppHooks>);

#[async_trait]
impl ApprovalAsker for Asker {
    async fn ask(&self, caller: &Caller, ask: ToolApproval) -> ApprovalAnswer {
        let threads = self.0.threads.get().and_then(Weak::upgrade);
        match (caller.thread(), threads) {
            (Some(thread), Some(threads)) => threads.ask_approval(thread, ask).await,
            _ => ApprovalAnswer::Deny { message: None },
        }
    }
}

/// Plugin approvals, holds and re-checks go to the plugin service.
struct Plugins(Arc<AppHooks>);

#[async_trait]
impl tandem_gateway::PluginHooks for Plugins {
    async fn ask(
        &self,
        lease: &LeaseContext,
        ask: tandem_gateway::PluginAsk,
    ) -> tandem_gateway::PluginVerdict {
        match self.0.plugins.get() {
            Some(h) => h.ask(lease, ask).await,
            None => tandem_gateway::PluginVerdict::Cancelled,
        }
    }

    async fn hold(
        &self,
        lease: &LeaseContext,
        call: tandem_gateway::HeldCall,
    ) -> Result<String, String> {
        match self.0.plugins.get() {
            Some(h) => h.hold(lease, call).await,
            None => Err("Held drafts aren't available yet".into()),
        }
    }

    async fn grant_present(
        &self,
        lease: &LeaseContext,
        connection: &tandem_core::ids::ConnectionId,
    ) -> bool {
        match self.0.plugins.get() {
            Some(h) => h.grant_present(lease, connection).await,
            None => false,
        }
    }

    async fn needs_attention(&self, connection: &ConnectionInfo) {
        if let Some(h) = self.0.plugins.get() {
            h.needs_attention(connection).await;
        }
    }
}

struct Builtins(Arc<AppHooks>);

#[async_trait]
impl BuiltinHandler for Builtins {
    async fn call(&self, lease: &LeaseContext, builtin: Builtin, args: Value) -> ToolOutput {
        match self.0.builtins.get().and_then(Weak::upgrade) {
            Some(service) => service.call(lease, builtin, args).await,
            None => ToolOutput {
                text: "This tool isn't available right now".into(),
                is_error: true,
            },
        }
    }
}

pub struct GatewayService {
    store: Store,
    run_dir: std::path::PathBuf,
    hooks: Arc<AppHooks>,
    cell: tokio::sync::OnceCell<GatewayHandle>,
    /// Times a gateway was started in this process (always 1 once up).
    starts: AtomicU32,
    /// Where plugin credentials are (the keychain).
    credentials: OnceLock<tandem_gateway::CredentialStore>,
    net: OnceLock<tandem_core::net::Net>,
    /// Unattended threads → the run they belong to (`routine:<id>:<run>`).
    origins: std::sync::Mutex<std::collections::HashMap<String, String>>,
    /// Where a gateway that keeps failing is reported (§5.2).
    bus: OnceLock<tandem_core::bus::EventBus>,
}

impl GatewayService {
    pub fn new(store: Store, run_dir: std::path::PathBuf) -> Arc<Self> {
        Arc::new(Self {
            store,
            run_dir,
            hooks: Arc::new(AppHooks {
                threads: OnceLock::new(),
                builtins: OnceLock::new(),
                plugins: OnceLock::new(),
            }),
            credentials: OnceLock::new(),
            net: OnceLock::new(),
            origins: std::sync::Mutex::default(),
            cell: tokio::sync::OnceCell::new(),
            starts: AtomicU32::new(0),
            bus: OnceLock::new(),
        })
    }

    /// The app's bus, for the watchdog's report (§5.2).
    pub fn bind_bus(&self, bus: tandem_core::bus::EventBus) {
        let _ = self.bus.set(bus);
    }

    /// The threads the approvals tool reports to.
    pub fn bind_threads(&self, threads: &Arc<ThreadService>) {
        let _ = self.hooks.threads.set(Arc::downgrade(threads));
    }

    /// The service that answers the built-ins.
    pub fn bind_builtins(&self, service: Weak<dyn BuiltinService>) {
        let _ = self.hooks.builtins.set(service);
    }

    /// Plugins: credentials from the keychain, approvals and holds through
    /// the plugin service. Set before the gateway starts.
    pub fn configure_plugins(
        &self,
        credentials: tandem_gateway::CredentialStore,
        net: tandem_core::net::Net,
        hooks: crate::services::plugins::Hooks,
    ) {
        let _ = self.credentials.set(credentials);
        let _ = self.net.set(net);
        let _ = self.hooks.plugins.set(hooks);
    }

    /// The run an unattended thread belongs to; what it holds for review
    /// is filed under it (F-PL-08). Set by whoever starts the run.
    pub fn set_unattended_origin(&self, thread: &tandem_core::ids::ThreadId, origin: &str) {
        if let Ok(mut o) = self.origins.lock() {
            o.insert(thread.as_str().to_string(), origin.to_string());
        }
    }

    pub fn unattended_origin(&self, thread: &tandem_core::ids::ThreadId) -> Option<String> {
        self.origins
            .lock()
            .ok()
            .and_then(|o| o.get(thread.as_str()).cloned())
    }

    /// A connection went away: its tools stop for every lease at once.
    pub fn invalidate_connection(&self, connection: &tandem_core::ids::ConnectionId) {
        if let Some(handle) = self.cell.get() {
            handle.invalidate_connection(connection);
        }
    }

    /// Forgets "Allow for this conversation" (a plugin's, or all of them on
    /// sign-out).
    pub fn forget_approvals(&self, plugin: Option<&str>) {
        if let Some(handle) = self.cell.get() {
            handle.forget_approvals(plugin);
        }
    }

    /// A connection's tools (detail page, held-draft schemas).
    pub async fn connection_tools(
        &self,
        connection: &tandem_core::ids::ConnectionId,
    ) -> Result<Vec<tandem_gateway::ToolDef>, IpcError> {
        let handle = self.handle().await?;
        handle
            .connection_tools(connection)
            .await
            .map_err(|e| IpcError::new(ErrorCode::Unavailable, e.to_string()))
    }

    /// The gateway, started on first use.
    pub async fn handle(&self) -> Result<&GatewayHandle, IpcError> {
        self.cell
            .get_or_try_init(|| async {
                let mut deps =
                    tandem_gateway::GatewayDeps::new(Arc::new(Asker(self.hooks.clone())));
                deps.builtins = Arc::new(Builtins(self.hooks.clone()));
                deps.hooks = Arc::new(Plugins(self.hooks.clone()));
                deps.credentials = self.credentials.get().cloned();
                deps.net = self.net.get().cloned();
                deps.run_dir = Some(self.run_dir.clone());
                deps.bus = self.bus.get().cloned();
                let handle = tandem_gateway::Gateway::start(deps).await.map_err(|e| {
                    IpcError::new(ErrorCode::Unavailable, e.to_string()).with_stage("gateway")
                })?;
                self.starts.fetch_add(1, Ordering::SeqCst);
                let expose = tandem_store::repo::settings::get(
                    &self.store,
                    SettingKey::ExposeAllPluginTools,
                )
                .await
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
                handle.set_expose_all(expose);
                handle.set_connections(load_connections(&self.store).await);
                Ok(handle)
            })
            .await
    }

    /// The connections changed (connect, disconnect, needs attention).
    pub async fn refresh_connections(&self) {
        if let Some(handle) = self.cell.get() {
            handle.set_connections(load_connections(&self.store).await);
        }
    }

    pub fn set_expose_all(&self, on: bool) {
        if let Some(handle) = self.cell.get() {
            handle.set_expose_all(on);
        }
    }

    pub fn revoke(&self, lease: &tandem_core::ids::LeaseId) {
        if let Some(handle) = self.cell.get() {
            handle.revoke(lease);
        }
    }

    /// The gateway's numbers, plus how many were ever started here.
    pub fn metrics(&self) -> Option<(GatewayMetrics, u32)> {
        self.cell
            .get()
            .map(|h| (h.metrics(), self.starts.load(Ordering::SeqCst)))
    }
}

async fn load_connections(store: &Store) -> Vec<ConnectionInfo> {
    match plugins::connections(store).await {
        Ok(all) => all
            .into_iter()
            .map(|c| ConnectionInfo {
                id: c.id,
                plugin_id: c.plugin_id,
                account_label: c.account_label,
                connected: c.status == ConnectionStatus::Connected,
                meta: c.meta,
                options: serde_json::from_value(c.options).unwrap_or_default(),
            })
            .collect(),
        Err(e) => {
            tracing::warn!(code = "gateway.connections_failed", error = %e, "couldn't read the plugin connections");
            Vec::new()
        }
    }
}
