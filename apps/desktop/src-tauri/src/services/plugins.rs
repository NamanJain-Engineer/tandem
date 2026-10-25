//! Plugins in the app (F-PL-03, F-PL-05..08; ARCHITECTURE §11): the catalog's
//! cards, the connect flows with their progress events, disconnect, the
//! approval cards and held drafts the gateway asks for, and the Team
//! allowlist. Credentials only ever pass between the keychain and the
//! gateway; nothing here logs or returns one.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_core::approvals::{ApprovalKind, ApprovalOutcome, ApprovalScope};
use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::ids::{ConnectionId, OpId, ThreadId};
use tandem_core::source::SourceRef;
use tandem_core::{ErrorCode, IpcError};
use tandem_gateway::connect::{self, Connected, Progress};
use tandem_gateway::credentials::CredentialStore;
use tandem_gateway::lease::LeaseContext;
use tandem_gateway::oauth::{Loopback, Step, SIGN_IN_TIMEOUT};
use tandem_gateway::vendors::{
    self, AuthKind, Availability, ConnectMethod, ModuleStatus, PluginOptions, RiskClass,
    VendorModule,
};
use tandem_gateway::{ConnectionInfo, HeldCall, PluginAsk, PluginHooks, PluginVerdict};
use tandem_store::repo::held_drafts::{self, HeldDraftRow, HeldState, NewDraft, OriginKind};
use tandem_store::repo::notifications::NotificationKind;
use tandem_store::repo::plugins::{self as repo, ConnectionStatus, GrantSubject, PluginConnection};
use tandem_store::Store;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use crate::services::opener::UrlOpener;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PluginState {
    Connected,
    Available,
    NeedsAttention,
    BlockedByOrg,
    /// Listed as ready, but this build can't connect it yet (no OAuth client
    /// in the config, or turned off): `unavailableReason` says why (D-960).
    Unavailable,
}

/// One card of Settings › Plugins (UI-90).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginCard {
    pub plugin_id: String,
    pub label: String,
    pub category: String,
    /// The two-letter tile.
    pub logo: String,
    pub description: String,
    /// "ready" or "preview".
    pub status: String,
    pub state: PluginState,
    pub account_label: Option<String>,
    pub connection_id: Option<ConnectionId>,
    pub auth_kind: AuthKind,
    /// The ways this build can connect it, in the connect sheet's order.
    pub methods: Vec<ConnectMethod>,
    /// Why the browser sign-in isn't offered, or why it can't connect at all.
    pub unavailable_reason: Option<String>,
    /// Also takes an API key (the connect sheet offers it).
    pub takes_key: bool,
    pub key_hint: Option<String>,
    /// Teammates this connection is enabled for.
    pub teammates: u32,
    /// The limits' connections are used up (none are with no account,
    /// D-2900).
    pub limit_reached: bool,
}

/// A step of a connect flow (`plugins:connect`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConnectProgress {
    pub op_id: String,
    pub plugin_id: String,
    pub step: Step,
    pub state: tandem_gateway::connect::StepState,
    pub authorize_url: Option<String>,
    /// The provider's words, untruncated.
    pub error: Option<String>,
    /// Set on the final, successful step.
    pub card: Option<PluginCard>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConnectOp {
    pub op_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginToolView {
    pub name: String,
    pub risk_class: RiskClass,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TeammateGrant {
    pub teammate_id: String,
    pub name: String,
    pub granted: bool,
}

/// UI-92.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginDetail {
    pub card: PluginCard,
    pub tools: Vec<PluginToolView>,
    /// Why the tools couldn't be listed (vendor down, needs attention).
    pub tools_error: Option<String>,
    pub options: PluginOptions,
    pub created_at: i64,
    pub teammates: Vec<TeammateGrant>,
}

/// What an approval card for a plugin action shows (the `detail` of a
/// `plugin_action` or `spend` approval, as JSON; F-PL-07).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginApprovalDetail {
    pub plugin_id: String,
    pub plugin_label: String,
    pub logo: String,
    pub account_label: Option<String>,
    pub tool: String,
    pub action: String,
    pub target: Option<String>,
    /// "5.00 USD", "120 credits", or none.
    pub cost: Option<String>,
    pub risk_class: RiskClass,
    /// The full request, pretty JSON, secrets masked.
    pub request: String,
}

/// A held draft as the Held actions list shows it (F-PL-08).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HeldDraft {
    #[serde(flatten)]
    pub row: HeldDraftRow,
    pub plugin_label: String,
    pub logo: String,
    pub action: String,
    /// The tool's input schema (Edit & run validates against it).
    #[ts(type = "unknown")]
    pub schema: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ToolResultView {
    pub text: String,
    pub is_error: bool,
}

/// What the plugin service asks of the thread service.
#[async_trait]
pub trait ThreadAsk: Send + Sync {
    /// Shows a card in the thread and waits for the answer.
    async fn ask(
        &self,
        thread: &ThreadId,
        kind: ApprovalKind,
        summary: String,
        detail: String,
        options: Vec<ApprovalScope>,
    ) -> ApprovalOutcome;
}

#[async_trait]
impl ThreadAsk for crate::services::threads::ThreadService {
    async fn ask(
        &self,
        thread: &ThreadId,
        kind: ApprovalKind,
        summary: String,
        detail: String,
        options: Vec<ApprovalScope>,
    ) -> ApprovalOutcome {
        self.ask_with_options(thread, kind, summary, detail, options)
            .await
    }
}

pub struct PluginService {
    store: Store,
    bus: EventBus,
    credentials: CredentialStore,
    opener: Arc<dyn UrlOpener>,
    notifications: Arc<crate::services::notifications::NotificationService>,
    gateway: Arc<crate::services::gateway::GatewayService>,
    entitlements: Arc<crate::services::entitlement::EntitlementService>,
    config: Arc<crate::services::config::ConfigService>,
    audit: Arc<crate::services::audit::AuditService>,
    threads: OnceLock<Weak<dyn ThreadAsk>>,
    loopback: tokio::sync::OnceCell<Loopback>,
    ops: Mutex<HashMap<String, CancellationToken>>,
    /// Test harness: a Team allowlist fixture (`Some(None)` clears it).
    allowlist_fixture: Mutex<Option<Option<Vec<String>>>>,
    sign_in_timeout: Duration,
}

fn store_err(e: tandem_store::StoreError) -> IpcError {
    IpcError::from(e)
}

impl PluginService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store: Store,
        bus: EventBus,
        keychain: Arc<dyn tandem_core::keychain::Keychain>,
        opener: Arc<dyn UrlOpener>,
        notifications: Arc<crate::services::notifications::NotificationService>,
        gateway: Arc<crate::services::gateway::GatewayService>,
        entitlements: Arc<crate::services::entitlement::EntitlementService>,
        config: Arc<crate::services::config::ConfigService>,
        audit: Arc<crate::services::audit::AuditService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            bus,
            credentials: CredentialStore::new(keychain),
            opener,
            notifications,
            gateway,
            entitlements,
            config,
            audit,
            threads: OnceLock::new(),
            loopback: tokio::sync::OnceCell::new(),
            ops: Mutex::default(),
            allowlist_fixture: Mutex::default(),
            sign_in_timeout: SIGN_IN_TIMEOUT,
        })
    }

    pub fn credentials(&self) -> &CredentialStore {
        &self.credentials
    }

    pub fn bind_threads(&self, threads: Weak<dyn ThreadAsk>) {
        let _ = self.threads.set(threads);
    }

    /// Settles drafts a crash left running.
    pub async fn settle_at_launch(&self) {
        if let Err(e) = held_drafts::settle_interrupted(&self.store).await {
            tracing::warn!(code = "plugins.settle_failed", error = %e, "couldn't settle held drafts");
        }
    }

    fn modules(&self) -> Vec<Arc<dyn VendorModule>> {
        vendors::registry()
    }

    fn module(&self, plugin_id: &str) -> Result<Arc<dyn VendorModule>, IpcError> {
        vendors::find(&self.modules(), plugin_id).ok_or_else(|| {
            IpcError::new(ErrorCode::NotFound, format!("No plugin named {plugin_id}"))
        })
    }

    /// The Team allowlist (F-PL-05 AC2): the org's `allowed_plugins`, read
    /// from the entitlement's policy claims. `None` allows every plugin.
    pub fn allowlist(&self) -> Option<Vec<String>> {
        if let Ok(f) = self.allowlist_fixture.lock() {
            if let Some(fixture) = f.as_ref() {
                return fixture.clone();
            }
        }
        self.entitlements.policy().and_then(|p| p.allowed_plugins)
    }

    /// Test harness only: an allowlist as if the org's policy said it.
    pub fn set_allowlist_fixture(&self, list: Option<Vec<String>>) {
        if let Ok(mut f) = self.allowlist_fixture.lock() {
            *f = Some(list);
        }
        self.publish_all();
    }

    /// The module's entry in the config's `plugins` map (D-960).
    fn client(&self, plugin_id: &str) -> Option<tandem_core::remote_config::PluginClient> {
        tandem_core::remote_config::plugin_clients(&self.config.value()).remove(plugin_id)
    }

    /// The ways this build can connect `module` (D-960).
    fn availability(&self, module: &dyn VendorModule) -> Availability {
        let m = module.manifest();
        vendors::availability(m, self.client(m.id).as_ref())
    }

    /// Refuses a way this build doesn't offer, with the card's reason.
    fn offers(&self, module: &dyn VendorModule, method: ConnectMethod) -> Result<(), IpcError> {
        let can = self.availability(module);
        if can.offers(method) {
            return Ok(());
        }
        Err(IpcError::new(
            ErrorCode::Invalid,
            can.reason.unwrap_or_else(|| {
                format!("{} can't be connected that way", module.manifest().label)
            }),
        ))
    }

    fn blocked(&self, plugin_id: &str) -> bool {
        self.allowlist()
            .is_some_and(|list| !list.iter().any(|p| p == plugin_id))
    }

    fn publish_all(&self) {
        self.bus
            .publish(BusEvent::new(Topic::Plugins, "plugins:changed", json!({})));
    }

    fn publish(&self, card: &PluginCard) {
        self.bus
            .publish(BusEvent::new(Topic::Plugins, "plugins:changed", card));
    }

    async fn teammate_counts(&self) -> HashMap<ConnectionId, u32> {
        let mut out = HashMap::new();
        if let Ok(conns) = repo::connections(&self.store).await {
            for c in conns {
                let n = repo::subjects_of(&self.store, &c.id)
                    .await
                    .map(|s| {
                        s.iter()
                            .filter(|s| matches!(s, GrantSubject::Teammate { .. }))
                            .count()
                    })
                    .unwrap_or(0);
                out.insert(c.id, n as u32);
            }
        }
        out
    }

    fn card(
        &self,
        module: &dyn VendorModule,
        conn: Option<&PluginConnection>,
        teammates: u32,
        limit_reached: bool,
    ) -> PluginCard {
        let m = module.manifest();
        let can = self.availability(module);
        let category = serde_json::to_value(m.category)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        let state = match conn.map(|c| c.status) {
            Some(ConnectionStatus::Connected) => PluginState::Connected,
            Some(ConnectionStatus::NeedsAttention) => PluginState::NeedsAttention,
            _ if self.blocked(m.id) => PluginState::BlockedByOrg,
            _ if m.status == ModuleStatus::Ready && can.methods.is_empty() => {
                PluginState::Unavailable
            }
            _ => PluginState::Available,
        };
        PluginCard {
            plugin_id: m.id.to_string(),
            label: m.label.to_string(),
            category,
            logo: m.logo.to_string(),
            description: m.description.to_string(),
            status: match m.status {
                ModuleStatus::Ready => "ready",
                ModuleStatus::Preview => "preview",
            }
            .to_string(),
            state,
            account_label: conn.and_then(|c| c.account_label.clone()),
            connection_id: conn.map(|c| c.id.clone()),
            auth_kind: m.auth,
            methods: can.methods.clone(),
            unavailable_reason: can.reason.clone(),
            takes_key: m.auth == AuthKind::ApiKey || m.alt_auth == Some(AuthKind::ApiKey),
            key_hint: m.key_hint.map(str::to_string),
            teammates,
            limit_reached,
        }
    }

    /// The limits' connection cap; `None` (no account, D-2900) is unlimited.
    async fn limit_reached(&self) -> bool {
        let Some(max) = self.entitlements.limits().plugin_connections else {
            return false;
        };
        let n = repo::connections(&self.store)
            .await
            .map(|c| {
                c.iter()
                    .filter(|c| c.status != ConnectionStatus::Disconnected)
                    .count()
            })
            .unwrap_or(0);
        n as u32 >= max
    }

    /// UI-90: a card per connection and one per module without any. Test
    /// modules (debug E2E builds) list too.
    pub async fn catalog(&self) -> Result<Vec<PluginCard>, IpcError> {
        let connections = repo::connections(&self.store).await.map_err(store_err)?;
        let counts = self.teammate_counts().await;
        let limit = self.limit_reached().await;
        let mut cards = Vec::new();
        for module in self.modules() {
            let id = module.manifest().id;
            let mine: Vec<&PluginConnection> = connections
                .iter()
                .filter(|c| c.plugin_id == id && c.status != ConnectionStatus::Disconnected)
                .collect();
            if mine.is_empty() {
                cards.push(self.card(module.as_ref(), None, 0, limit));
            }
            for c in mine {
                cards.push(self.card(
                    module.as_ref(),
                    Some(c),
                    counts.get(&c.id).copied().unwrap_or(0),
                    limit,
                ));
            }
        }
        Ok(cards)
    }

    async fn card_for(&self, connection: &ConnectionId) -> Result<PluginCard, IpcError> {
        let conn = self.connection(connection).await?;
        let module = self.module(&conn.plugin_id)?;
        let n = self
            .teammate_counts()
            .await
            .get(connection)
            .copied()
            .unwrap_or(0);
        Ok(self.card(module.as_ref(), Some(&conn), n, self.limit_reached().await))
    }

    async fn connection(&self, id: &ConnectionId) -> Result<PluginConnection, IpcError> {
        repo::connections(&self.store)
            .await
            .map_err(store_err)?
            .into_iter()
            .find(|c| &c.id == id)
            .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "That plugin isn't connected"))
    }

    /// Whether a new connection of `module` may be made (allowlist, preview,
    /// limits' cap); a reconnect of an existing one skips the limit.
    async fn may_connect(
        &self,
        module: &dyn VendorModule,
        reconnect: bool,
    ) -> Result<(), IpcError> {
        let m = module.manifest();
        if m.status == ModuleStatus::Preview {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                format!("{} isn't ready yet", m.label),
            ));
        }
        if self.blocked(m.id) {
            return Err(IpcError::new(
                ErrorCode::ForbiddenPlan,
                format!("Your organization doesn't allow {}", m.label),
            ));
        }
        if !reconnect && self.limit_reached().await {
            return Err(IpcError::new(
                ErrorCode::ForbiddenPlan,
                "This account's plugin connections are used up.",
            ));
        }
        Ok(())
    }

    async fn loopback(&self) -> Result<&Loopback, IpcError> {
        self.loopback
            .get_or_try_init(|| async {
                Loopback::start()
                    .await
                    .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))
            })
            .await
    }

    /// `tandem://plugin/callback?…` (F-SH-13): handed to the same listener.
    pub async fn deep_link_callback(&self, query: &str) -> bool {
        match self.loopback().await {
            Ok(lb) => lb.deliver(query),
            Err(_) => false,
        }
    }

    /// Records a working connection (new or reconnected) and tells
    /// everyone: gateway, leases, cards.
    async fn record(
        &self,
        module: &dyn VendorModule,
        connection: &ConnectionId,
        reconnect: bool,
        done: &Connected,
    ) -> Result<PluginCard, IpcError> {
        let m = module.manifest();
        if reconnect {
            repo::reconnected(
                &self.store,
                connection,
                done.account.label.as_deref(),
                done.auth_kind.as_str(),
                done.meta.clone(),
            )
            .await
            .map_err(store_err)?;
        } else {
            repo::insert_connection_with(
                &self.store,
                connection,
                m.id,
                done.account.label.as_deref(),
                done.auth_kind.as_str(),
                done.meta.clone(),
            )
            .await
            .map_err(store_err)?;
        }
        // A reconnect clears remembered approvals (F-PL-06).
        self.gateway.forget_approvals(Some(m.id));
        self.gateway.refresh_connections().await;
        self.audit.record(
            crate::services::audit::PLUGIN_CONNECTED,
            &[("plugin", m.id)],
        );
        self.notifications
            .resolve_key(&format!("plugin_attention:{}", connection.as_str()))
            .await;
        let card = self.card_for(connection).await?;
        self.publish(&card);
        Ok(card)
    }

    /// The browser sign-in, in the background; progress on
    /// `plugins:connect`. `connection` set means a reconnect.
    pub async fn connect_oauth(
        self: &Arc<Self>,
        plugin_id: &str,
        connection: Option<ConnectionId>,
    ) -> Result<ConnectOp, IpcError> {
        let module = self.module(plugin_id)?;
        let reconnect = connection.is_some();
        self.may_connect(module.as_ref(), reconnect).await?;
        self.offers(module.as_ref(), ConnectMethod::Oauth)?;
        let loopback = self.loopback().await?.clone();
        let op = OpId::new().as_str().to_string();
        let cancel = CancellationToken::new();
        if let Ok(mut ops) = self.ops.lock() {
            ops.insert(op.clone(), cancel.clone());
        }
        let conn = connection.unwrap_or_default();
        let static_client = self.client(plugin_id).and_then(|c| c.client_id);
        let me = self.clone();
        let (op_id, pid) = (op.clone(), plugin_id.to_string());
        let progress_bus = self.bus.clone();
        let progress = {
            let (op_id, pid) = (op_id.clone(), pid.clone());
            Arc::new(move |p: Progress| {
                progress_bus.publish(BusEvent::new(
                    Topic::Plugins,
                    "plugins:connect",
                    ConnectProgress {
                        op_id: op_id.clone(),
                        plugin_id: pid.clone(),
                        step: p.step,
                        state: p.state,
                        authorize_url: p.authorize_url,
                        error: p.error,
                        card: None,
                    },
                ));
            })
        };
        let opener = self.opener.clone();
        let open = Arc::new(move |url: &url::Url| opener.open(url.as_str()));
        tauri::async_runtime::spawn(async move {
            let req = connect::OAuthRequest {
                module: module.clone(),
                connection: conn.clone(),
                static_client_id: static_client,
                open,
                progress,
                cancel,
                timeout: me.sign_in_timeout,
            };
            let result = connect::oauth(&me.credentials, &loopback, req).await;
            if let Ok(mut ops) = me.ops.lock() {
                ops.remove(&op_id);
            }
            let finish = |step, state, error: Option<String>, card| {
                me.bus.publish(BusEvent::new(
                    Topic::Plugins,
                    "plugins:connect",
                    ConnectProgress {
                        op_id: op_id.clone(),
                        plugin_id: pid.clone(),
                        step,
                        state,
                        authorize_url: None,
                        error,
                        card,
                    },
                ));
            };
            match result {
                Ok(done) => match me.record(module.as_ref(), &conn, reconnect, &done).await {
                    Ok(card) => finish(
                        Step::Verification,
                        tandem_gateway::connect::StepState::Done,
                        None,
                        Some(card),
                    ),
                    Err(e) => finish(
                        Step::Verification,
                        tandem_gateway::connect::StepState::Failed,
                        Some(e.message),
                        None,
                    ),
                },
                Err(e) if e.message == "Cancelled" => {
                    tracing::info!(
                        code = "plugins.connect_cancelled",
                        "a plugin sign-in was cancelled"
                    );
                }
                Err(e) => {
                    tracing::info!(
                        code = "plugins.connect_failed",
                        step = e.step.as_str(),
                        "a plugin sign-in failed"
                    );
                }
            }
        });
        Ok(ConnectOp { op_id: op })
    }

    /// Cancel on the connect sheet: the attempt stops waiting, and its
    /// callback, if it still comes, is ignored.
    pub fn cancel(&self, op_id: &str) {
        if let Some(token) = self.ops.lock().ok().and_then(|mut o| o.remove(op_id)) {
            token.cancel();
        }
    }

    /// An API key, verified before it's saved. An empty key cancels and
    /// changes nothing (UI-91).
    pub async fn connect_api_key(
        &self,
        plugin_id: &str,
        key: &str,
        connection: Option<ConnectionId>,
    ) -> Result<Option<PluginCard>, IpcError> {
        if key.trim().is_empty() {
            return Ok(None);
        }
        let module = self.module(plugin_id)?;
        let reconnect = connection.is_some();
        self.may_connect(module.as_ref(), reconnect).await?;
        self.offers(module.as_ref(), ConnectMethod::Key)?;
        let conn = connection.unwrap_or_default();
        let done = connect::api_key(&self.credentials, module.as_ref(), &conn, key)
            .await
            .map_err(|e| {
                IpcError::new(ErrorCode::Invalid, e.message).with_stage(e.step.as_str())
            })?;
        self.record(module.as_ref(), &conn, reconnect, &done)
            .await
            .map(Some)
    }

    /// A built-in with no sign-in (Web fetch), or a local port.
    pub async fn connect_local(
        &self,
        plugin_id: &str,
        port: Option<u16>,
    ) -> Result<PluginCard, IpcError> {
        let module = self.module(plugin_id)?;
        self.may_connect(module.as_ref(), false).await?;
        let method = if port.is_some() {
            ConnectMethod::Local
        } else {
            ConnectMethod::None
        };
        self.offers(module.as_ref(), method)?;
        let done = match (module.manifest().auth, port) {
            (AuthKind::None, _) => Connected {
                account: Default::default(),
                auth_kind: AuthKind::None,
                meta: json!({}),
            },
            (AuthKind::LocalPort, Some(p)) => connect::probe_local(module.as_ref(), p)
                .await
                .map_err(|e| IpcError::new(ErrorCode::Unavailable, e.message))?,
            _ => {
                return Err(IpcError::new(
                    ErrorCode::Invalid,
                    format!("{} needs a sign-in", module.manifest().label),
                ))
            }
        };
        let conn = ConnectionId::new();
        self.record(module.as_ref(), &conn, false, &done).await
    }

    /// Disconnect (F-PL-03): revoke upstream where possible, delete the
    /// credential, drop the connection and its grants; leases lose it at
    /// once and remembered approvals go.
    pub async fn disconnect(&self, connection: &ConnectionId) -> Result<(), IpcError> {
        let conn = self.connection(connection).await?;
        let module = self.module(&conn.plugin_id)?;
        let info = ConnectionInfo {
            id: conn.id.clone(),
            plugin_id: conn.plugin_id.clone(),
            account_label: conn.account_label.clone(),
            connected: false,
            meta: conn.meta.clone(),
            options: PluginOptions::default(),
        };
        if let Err(e) = connect::disconnect(&self.credentials, module.as_ref(), &info).await {
            return Err(IpcError::new(ErrorCode::Io, e));
        }
        self.gateway.invalidate_connection(connection);
        repo::delete_connection(&self.store, connection)
            .await
            .map_err(store_err)?;
        self.gateway.forget_approvals(Some(&conn.plugin_id));
        self.gateway.refresh_connections().await;
        self.publish_all();
        Ok(())
    }

    pub async fn detail(&self, connection: &ConnectionId) -> Result<PluginDetail, IpcError> {
        let conn = self.connection(connection).await?;
        let card = self.card_for(connection).await?;
        let (tools, tools_error) = match self.gateway.connection_tools(connection).await {
            Ok(defs) => (
                defs.into_iter()
                    .map(|d| PluginToolView {
                        name: d.name,
                        risk_class: d.risk_class,
                        description: d.description,
                    })
                    .collect(),
                None,
            ),
            Err(e) => (Vec::new(), Some(e.to_string())),
        };
        let granted = repo::subjects_of(&self.store, connection)
            .await
            .map_err(store_err)?;
        let teammates = tandem_store::repo::teammates::list(&self.store)
            .await
            .map_err(store_err)?
            .into_iter()
            .map(|t| TeammateGrant {
                granted: granted.contains(&GrantSubject::Teammate {
                    teammate_id: t.id.as_str().to_string(),
                }),
                teammate_id: t.id.as_str().to_string(),
                name: t.name,
            })
            .collect();
        Ok(PluginDetail {
            card,
            tools,
            tools_error,
            options: serde_json::from_value(conn.options).unwrap_or_default(),
            created_at: conn.created_at,
            teammates,
        })
    }

    /// Grants changed: cards' teammate counts follow.
    pub fn grants_changed(&self) {
        self.publish_all();
    }

    pub async fn set_options(
        &self,
        connection: &ConnectionId,
        options: PluginOptions,
    ) -> Result<(), IpcError> {
        self.connection(connection).await?;
        repo::set_options(
            &self.store,
            connection,
            serde_json::to_value(options).unwrap_or_default(),
        )
        .await
        .map_err(store_err)?;
        self.gateway.refresh_connections().await;
        Ok(())
    }

    // ── held drafts (F-PL-08) ─────────────────────────────────────────

    async fn view(&self, row: HeldDraftRow) -> HeldDraft {
        let module = vendors::find(&self.modules(), &row.plugin_id);
        let (label, logo, action) = match &module {
            Some(m) => (
                m.manifest().label.to_string(),
                m.manifest().logo.to_string(),
                m.describe(&row.tool, &row.args).action,
            ),
            None => (
                row.plugin_id.clone(),
                "??".into(),
                vendors::humanize(&row.tool),
            ),
        };
        let schema = self
            .gateway
            .connection_tools(&row.connection_id)
            .await
            .ok()
            .and_then(|defs| defs.into_iter().find(|d| d.name == row.tool))
            .map(|d| d.input_schema)
            .unwrap_or_else(|| json!({"type": "object"}));
        HeldDraft {
            row,
            plugin_label: label,
            logo,
            action,
            schema,
        }
    }

    pub async fn held_list(
        &self,
        state: Option<HeldState>,
        origin_id: Option<String>,
    ) -> Result<Vec<HeldDraft>, IpcError> {
        let rows = held_drafts::list(&self.store, state, origin_id)
            .await
            .map_err(store_err)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(self.view(r).await);
        }
        Ok(out)
    }

    /// Approve & run, or Edit & run with `edited` (checked against the
    /// tool's schema first). Runs exactly once.
    pub async fn held_run(
        &self,
        id: &str,
        edited: Option<Value>,
    ) -> Result<ToolResultView, IpcError> {
        let draft = held_drafts::get(&self.store, id)
            .await
            .map_err(store_err)?
            .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "That draft is gone"))?;
        if draft.state != HeldState::Held {
            return Err(IpcError::new(
                ErrorCode::Conflict,
                "That draft was already run or discarded",
            ));
        }
        if let Some(args) = &edited {
            let schema = self
                .gateway
                .connection_tools(&draft.connection_id)
                .await
                .map_err(|e| IpcError::new(ErrorCode::Unavailable, e.to_string()))?
                .into_iter()
                .find(|d| d.name == draft.tool)
                .map(|d| d.input_schema)
                .ok_or_else(|| {
                    IpcError::new(ErrorCode::Invalid, "The plugin no longer has this tool")
                })?;
            tandem_gateway::schema::validate(&schema, args)
                .map_err(|e| IpcError::new(ErrorCode::Invalid, e))?;
        }
        if !held_drafts::claim(&self.store, id, edited.clone())
            .await
            .map_err(store_err)?
        {
            return Err(IpcError::new(
                ErrorCode::Conflict,
                "That draft was already run or discarded",
            ));
        }
        let args = edited.unwrap_or(draft.args.clone());
        let handle = self.gateway.handle().await?;
        let out = handle
            .run_held(&draft.connection_id, &draft.tool, args)
            .await;
        held_drafts::finish(&self.store, id, !out.is_error, &out.text)
            .await
            .map_err(store_err)?;
        if !out.is_error {
            self.audit.record(
                crate::services::audit::PLUGIN_WRITE_APPROVED,
                &[("plugin", &draft.plugin_id)],
            );
        }
        self.after_held_change(&draft.origin_id).await;
        Ok(ToolResultView {
            text: out.text,
            is_error: out.is_error,
        })
    }

    pub async fn held_discard(&self, ids: Vec<String>) -> Result<(), IpcError> {
        let rows: Vec<_> = {
            let mut v = Vec::new();
            for id in &ids {
                if let Ok(Some(r)) = held_drafts::get(&self.store, id).await {
                    v.push(r.origin_id);
                }
            }
            v
        };
        held_drafts::discard(&self.store, ids)
            .await
            .map_err(store_err)?;
        for origin in rows {
            self.after_held_change(&origin).await;
        }
        Ok(())
    }

    /// Updates the run's "N actions held for review" notice (one per run).
    async fn after_held_change(&self, origin_id: &str) {
        let held = held_drafts::list(
            &self.store,
            Some(HeldState::Held),
            Some(origin_id.to_string()),
        )
        .await
        .map(|v| v.len())
        .unwrap_or(0);
        let key = format!("held_actions:{origin_id}");
        if held == 0 {
            self.notifications.resolve_key(&key).await;
        } else {
            self.notifications
                .raise_in_app(
                    NotificationKind::HeldActions,
                    held_title(held),
                    "Approve, edit or discard them in Held actions.".into(),
                    SourceRef::Routine {
                        routine_id: origin_id.to_string(),
                        run_id: None,
                    },
                    key,
                )
                .await;
        }
        self.bus.publish(BusEvent::new(
            Topic::Plugins,
            "held:changed",
            json!({ "originId": origin_id, "held": held }),
        ));
    }
}

/// "1 action held for review" / "5 actions held for review".
pub fn held_title(n: usize) -> String {
    if n == 1 {
        "1 action held for review".into()
    } else {
        format!("{n} actions held for review")
    }
}

/// `routine:<id>[:<run>]`, `task:<id>`, `mail:<id>`, `auto:<id>`.
pub fn parse_origin(origin: &str) -> Option<(OriginKind, String, Option<String>)> {
    let mut parts = origin.splitn(3, ':');
    let kind = OriginKind::parse(parts.next()?)?;
    let id = parts.next().filter(|s| !s.is_empty())?.to_string();
    let run = parts.next().filter(|s| !s.is_empty()).map(str::to_string);
    Some((kind, id, run))
}

/// The gateway's view of the app for plugin calls.
pub struct Hooks(pub Weak<PluginService>);

#[async_trait]
impl PluginHooks for Hooks {
    async fn ask(&self, lease: &LeaseContext, ask: PluginAsk) -> PluginVerdict {
        let Some(me) = self.0.upgrade() else {
            return PluginVerdict::Cancelled;
        };
        let Some(thread) = lease.session.caller.thread().cloned() else {
            return PluginVerdict::Deny {
                reason: Some("Only threads and teammate chats can ask for plugin approvals".into()),
            };
        };
        let Some(threads) = me.threads.get().and_then(Weak::upgrade) else {
            return PluginVerdict::Cancelled;
        };
        let kind = if ask.risk_class == RiskClass::Spend {
            ApprovalKind::Spend
        } else {
            ApprovalKind::PluginAction
        };
        let detail = PluginApprovalDetail {
            plugin_id: ask.plugin_id.clone(),
            plugin_label: ask.plugin_label.clone(),
            logo: ask.logo.clone(),
            account_label: ask.account_label.clone(),
            tool: ask.tool.clone(),
            action: ask.action.clone(),
            target: ask.target.clone(),
            cost: ask.cost.as_ref().map(vendors::Cost::display),
            risk_class: ask.risk_class,
            request: serde_json::to_string_pretty(&ask.request).unwrap_or_default(),
        };
        let mut options = vec![ApprovalScope::Once];
        if ask.offer_conversation {
            options.push(ApprovalScope::Conversation);
        }
        let summary = format!("Use {}: {}", ask.plugin_label, ask.action);
        let detail = serde_json::to_string(&detail).unwrap_or_default();
        match threads.ask(&thread, kind, summary, detail, options).await {
            ApprovalOutcome::Allowed { scope, .. } => {
                if matches!(
                    ask.risk_class,
                    RiskClass::Write | RiskClass::ExternalPublish
                ) {
                    me.audit.record(
                        crate::services::audit::PLUGIN_WRITE_APPROVED,
                        &[("plugin", &ask.plugin_id)],
                    );
                }
                PluginVerdict::Allow {
                    conversation: scope == ApprovalScope::Conversation,
                }
            }
            ApprovalOutcome::Denied { reason, .. } => PluginVerdict::Deny { reason },
            ApprovalOutcome::Expired => PluginVerdict::Expired,
            ApprovalOutcome::Cancelled => PluginVerdict::Cancelled,
        }
    }

    async fn hold(&self, lease: &LeaseContext, call: HeldCall) -> Result<String, String> {
        let me = self.0.upgrade().ok_or("Vibezeo is shutting down")?;
        let (kind, origin_id, run_id) = parse_origin(&call.origin).ok_or_else(|| {
            "this run doesn't say where it came from, so nothing can be held for it".to_string()
        })?;
        let row = held_drafts::insert(
            &me.store,
            NewDraft {
                connection_id: call.connection_id,
                plugin_id: call.plugin_id,
                tool: call.tool,
                args: call.args,
                risk_class: call.risk_class.as_str().to_string(),
                origin_kind: kind,
                origin_id: origin_id.clone(),
                run_id,
                thread_id: lease
                    .session
                    .caller
                    .thread()
                    .map(|t| t.as_str().to_string()),
            },
        )
        .await
        .map_err(|e| e.to_string())?;
        me.after_held_change(&origin_id).await;
        Ok(format!("D-{}", row.display_no))
    }

    async fn grant_present(&self, lease: &LeaseContext, connection: &ConnectionId) -> bool {
        let Some(me) = self.0.upgrade() else {
            return false;
        };
        let store = &me.store;
        let caller = &lease.session.caller;
        let subjects: Vec<GrantSubject> = match (caller.teammate(), caller.thread()) {
            (Some(tm), _) => vec![GrantSubject::Teammate {
                teammate_id: tm.as_str().to_string(),
            }],
            (None, Some(th)) => {
                let mut v = vec![
                    GrantSubject::Thread {
                        thread_id: th.as_str().to_string(),
                    },
                    GrantSubject::AllThreads,
                ];
                if let Ok(Some(t)) = tandem_store::repo::threads::get(store, th).await {
                    if let Some(ws) = t.workspace_id {
                        v.push(GrantSubject::Workspace {
                            workspace_id: ws.as_str().to_string(),
                        });
                    }
                }
                v
            }
            _ => Vec::new(),
        };
        for s in subjects {
            if repo::grants(store, &s)
                .await
                .is_ok_and(|g| g.contains(connection))
            {
                return true;
            }
        }
        false
    }

    async fn needs_attention(&self, connection: &ConnectionInfo) {
        let Some(me) = self.0.upgrade() else {
            return;
        };
        let changed = repo::set_status(&me.store, &connection.id, ConnectionStatus::NeedsAttention)
            .await
            .unwrap_or(false);
        if !changed {
            return;
        }
        let label = vendors::find(&me.modules(), &connection.plugin_id)
            .map(|m| m.manifest().label.to_string())
            .unwrap_or_else(|| connection.plugin_id.clone());
        me.notifications
            .raise_in_app(
                NotificationKind::PluginAttention,
                format!("Reconnect {label}"),
                format!("{label}'s sign-in expired or was revoked. Teammates can't use it until you reconnect."),
                SourceRef::Plugin {
                    connection_id: connection.id.as_str().to_string(),
                },
                format!("plugin_attention:{}", connection.id.as_str()),
            )
            .await;
        if let Ok(card) = me.card_for(&connection.id).await {
            me.publish(&card);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_pl_08_origins_name_the_run() {
        assert_eq!(
            parse_origin("routine:rt_1:run_9"),
            Some((OriginKind::Routine, "rt_1".into(), Some("run_9".into())))
        );
        assert_eq!(
            parse_origin("task:iss_4"),
            Some((OriginKind::Task, "iss_4".into(), None))
        );
        assert_eq!(parse_origin("thread"), None);
        assert_eq!(parse_origin("routine:"), None);
        assert_eq!(held_title(1), "1 action held for review");
        assert_eq!(held_title(5), "5 actions held for review");
    }
}
