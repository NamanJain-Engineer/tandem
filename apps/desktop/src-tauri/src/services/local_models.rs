//! Local models as engines (F-EN-11, UI-164): model servers the user runs on
//! this computer, their models, and a Codex pane configured for one in its
//! own provider home. Every request goes to `127.0.0.1` (D-2517).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::ids::TabId;
use tandem_core::settings::SettingKey;
use tandem_core::{ErrorCode, IpcError};
use tandem_engines::local_models::{self, LocalModel, ServerKind};
use tandem_store::repo::panes;
use tandem_store::repo::sessions::LaunchConfig;
use tandem_store::repo::settings;

use crate::commands::terminal::create_pane_with;
use crate::services::library::LibraryOpened;
use crate::state::AppState;

/// How long one server has to answer.
pub const PROBE_WAIT: Duration = Duration::from_secs(1);

/// One server, as UI-164 lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LocalServer {
    pub kind: ServerKind,
    pub port: u16,
    /// Added by the user (it can be removed), not a default port.
    pub added: bool,
    pub running: bool,
    pub models: Vec<LocalModel>,
    /// Codex can start with its models (D-2518).
    pub startable: bool,
}

/// Asks a server on this computer for a path; the trait is how the tests
/// see every request.
#[async_trait::async_trait]
pub trait LocalHttp: Send + Sync {
    /// `None`: nothing answered, or not with JSON.
    async fn get_json(&self, url: &str) -> Option<serde_json::Value>;
}

/// The real client: no proxy (a proxy would send the request off the
/// machine), a 1 s limit, at most 4 MB.
pub struct Reqwest(reqwest::Client);

impl Reqwest {
    pub fn new() -> Option<Self> {
        reqwest::Client::builder()
            .no_proxy()
            .timeout(PROBE_WAIT)
            .connect_timeout(PROBE_WAIT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .ok()
            .map(Self)
    }
}

#[async_trait::async_trait]
impl LocalHttp for Reqwest {
    async fn get_json(&self, url: &str) -> Option<serde_json::Value> {
        if !url.starts_with("http://127.0.0.1:") {
            return None;
        }
        let res = self.0.get(url).send().await.ok()?;
        if !res.status().is_success() {
            return None;
        }
        let bytes = res.bytes().await.ok()?;
        if bytes.len() > 4 * 1024 * 1024 {
            return None;
        }
        serde_json::from_slice(&bytes).ok()
    }
}

/// The ports to ask: every kind's default, then the user's.
pub fn targets(added: &BTreeMap<u16, ServerKind>) -> Vec<(ServerKind, u16, bool)> {
    let mut out: Vec<(ServerKind, u16, bool)> = ServerKind::ALL
        .iter()
        .map(|k| (*k, k.default_port(), false))
        .collect();
    for (port, kind) in added {
        if !out.iter().any(|(_, p, _)| p == port) {
            out.push((*kind, *port, true));
        }
    }
    out
}

/// Asks each target once, all at once.
pub async fn find(http: &dyn LocalHttp, added: &BTreeMap<u16, ServerKind>) -> Vec<LocalServer> {
    let asks = targets(added)
        .into_iter()
        .map(|(kind, port, added)| async move {
            let body = http
                .get_json(&local_models::url(port, kind.models_path()))
                .await;
            let models = body.as_ref().and_then(|b| local_models::parse(kind, b));
            LocalServer {
                kind,
                port,
                added,
                running: models.is_some(),
                models: models.unwrap_or_default(),
                startable: kind.startable(),
            }
        });
    futures::future::join_all(asks).await
}

/// The user's added ports (`local_model_ports`: `{"ports": ["kind:port"]}`).
pub fn ports_from(value: &serde_json::Value) -> BTreeMap<u16, ServerKind> {
    value
        .get("ports")
        .and_then(serde_json::Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(serde_json::Value::as_str)
                .filter_map(local_models::parse_port_entry)
                .map(|(kind, port)| (port, kind))
                .take(MAX_PORTS)
                .collect()
        })
        .unwrap_or_default()
}

pub fn ports_value(ports: &BTreeMap<u16, ServerKind>) -> serde_json::Value {
    let list: Vec<serde_json::Value> = ports
        .iter()
        .map(|(port, kind)| serde_json::Value::String(local_models::port_entry(*kind, *port)))
        .collect();
    serde_json::json!({ "ports": list })
}

/// Added ports at most.
pub const MAX_PORTS: usize = 16;

async fn added_ports(state: &AppState) -> BTreeMap<u16, ServerKind> {
    let value = settings::get(&state.store, SettingKey::LocalModelPorts)
        .await
        .unwrap_or_default();
    ports_from(&value)
}

async fn save_ports(state: &AppState, ports: &BTreeMap<u16, ServerKind>) -> Result<(), IpcError> {
    settings::set(
        &state.store,
        SettingKey::LocalModelPorts,
        ports_value(ports),
    )
    .await?;
    Ok(())
}

fn http() -> Result<Arc<dyn LocalHttp>, IpcError> {
    Reqwest::new()
        .map(|c| Arc::new(c) as Arc<dyn LocalHttp>)
        .ok_or_else(|| IpcError::new(ErrorCode::Internal, "Couldn't set up the local client"))
}

/// `local_models_list`.
pub async fn list(state: &AppState) -> Result<Vec<LocalServer>, IpcError> {
    let added = added_ports(state).await;
    let found = find(http()?.as_ref(), &added).await;
    tracing::info!(
        code = "local_models.found",
        running = found.iter().filter(|s| s.running).count(),
        "local model servers checked"
    );
    Ok(found)
}

/// `local_models_add_port`: loopback hosts only.
pub async fn add_port(
    state: &AppState,
    host: &str,
    port: u16,
    kind: ServerKind,
) -> Result<(), IpcError> {
    if !local_models::loopback_host(host) {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "Vibezeo only looks for model servers on this computer (127.0.0.1 or localhost)",
        ));
    }
    if port == 0 {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "Ports run from 1 to 65535",
        ));
    }
    let mut ports = added_ports(state).await;
    if ports.len() >= MAX_PORTS && !ports.contains_key(&port) {
        return Err(IpcError::new(
            ErrorCode::LimitReached,
            format!("Vibezeo keeps up to {MAX_PORTS} added ports"),
        ));
    }
    ports.insert(port, kind);
    save_ports(state, &ports).await
}

/// `local_models_remove_port`.
pub async fn remove_port(state: &AppState, port: u16) -> Result<(), IpcError> {
    let mut ports = added_ports(state).await;
    ports.remove(&port);
    save_ports(state, &ports).await
}

/// The provider home a server's Codex panes use, with its `config.toml`
/// written for `model`. Only this folder is written (F-EN-11 AC2).
pub fn prepare_home(
    providers: &std::path::Path,
    kind: ServerKind,
    port: u16,
    model: &str,
) -> Result<PathBuf, IpcError> {
    let home = providers
        .join("codex")
        .join(local_models::home_name(kind, port));
    std::fs::create_dir_all(&home)
        .map_err(|e| IpcError::new(ErrorCode::Io, format!("Couldn't make the folder: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700));
    }
    let file = home.join("config.toml");
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    let config = local_models::codex_config(&existing, kind, port, model)
        .map_err(|e| IpcError::new(ErrorCode::Invalid, e))?;
    tandem_core::atomic::atomic_write(&file, config.as_bytes(), Some(0o600)).map_err(|e| {
        IpcError::new(
            ErrorCode::Io,
            format!("Couldn't write Codex's settings: {e}"),
        )
    })?;
    Ok(home)
}

/// `local_model_start`: a Codex pane in `tab` for `model` on that server,
/// whose first start runs Codex in the server's own provider home with no
/// account (D-2519).
pub async fn start(
    state: &AppState,
    tab: &TabId,
    kind: ServerKind,
    port: u16,
    model: &str,
) -> Result<LibraryOpened, IpcError> {
    if !kind.startable() {
        return Err(IpcError::new(
            ErrorCode::EngineUnsupported,
            format!("Codex can't use {} servers yet", kind.label()),
        ));
    }
    if !local_models::model_ok(model) {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "That model's name can't be used",
        ));
    }
    let engines = &state.terminals.engines;
    let codex = engines
        .get("codex")
        .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "Codex isn't known to Vibezeo"))?;
    if state.terminals.resolve_engine(&codex).await.is_none() {
        return Err(IpcError::new(
            ErrorCode::EngineMissing,
            "Install Codex to use a local model",
        ));
    }
    let offered = find(http()?.as_ref(), &BTreeMap::from([(port, kind)]))
        .await
        .into_iter()
        .find(|s| s.port == port)
        .is_some_and(|s| s.models.iter().any(|m| m.id == model));
    if !offered {
        return Err(IpcError::new(
            ErrorCode::NotFound,
            format!(
                "{} on port {port} doesn't offer {model} right now",
                kind.label()
            ),
        ));
    }
    let home = prepare_home(engines.homes.root(), kind, port, model)?;
    let mut pane =
        create_pane_with(state, tab, "agent", None, Some("codex".into()), None, false).await?;
    let label = format!("{model} · {}", kind.label());
    panes::set_local_model(&state.store, &pane.id, &label).await?;
    state.terminals.launch_on_first_start(
        &pane.id,
        LaunchConfig {
            extra_args: Vec::new(),
            home: Some(home.to_string_lossy().into_owned()),
            own_home: false,
        },
    );
    tracing::info!(
        code = "local_models.started",
        "a Codex pane on a local model"
    );
    pane.local_model = Some(label);
    Ok(LibraryOpened {
        workspace_id: pane.workspace_id.clone(),
        tab_id: pane.tab_id.clone(),
        pane,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Answers one URL and records every request.
    struct Fake {
        answer: (String, serde_json::Value),
        asked: std::sync::Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl LocalHttp for Fake {
        async fn get_json(&self, url: &str) -> Option<serde_json::Value> {
            self.asked.lock().expect("lock").push(url.to_string());
            (url == self.answer.0).then(|| self.answer.1.clone())
        }
    }

    #[tokio::test]
    async fn f_en_11_1_a_server_answering_ollamas_tags_is_found_with_its_models() {
        let fake = Fake {
            answer: (
                "http://127.0.0.1:11500/api/tags".into(),
                json!({"models": [{"name": "qwen3-coder:30b", "size": 18_556_701_696u64, "details": {"parameter_size": "30.5B"}}]}),
            ),
            asked: Default::default(),
        };
        let found = find(&fake, &BTreeMap::from([(11500, ServerKind::Ollama)])).await;
        let ours = found.iter().find(|s| s.port == 11500).expect("listed");
        assert!(ours.running && ours.added && ours.startable);
        assert_eq!(ours.models[0].parameters.as_deref(), Some("30.5B"));
        // The defaults were asked too, and aren't running.
        assert_eq!(found.iter().filter(|s| !s.running).count(), 3);
        // F-EN-11 AC3: every request went to this computer.
        let asked = fake.asked.lock().expect("lock").clone();
        assert_eq!(asked.len(), 4);
        assert!(
            asked.iter().all(|u| u.starts_with("http://127.0.0.1:")),
            "{asked:?}"
        );
    }

    #[tokio::test]
    async fn f_en_11_3_the_real_client_never_asks_off_this_computer() {
        let client = Reqwest::new().expect("client");
        assert!(client
            .get_json("http://192.168.1.5:11434/api/tags")
            .await
            .is_none());
        assert!(client
            .get_json("http://localhost.example.com:11434/api/tags")
            .await
            .is_none());
    }

    #[test]
    fn f_en_11_2_the_home_is_one_folder_under_codex_with_the_config() {
        let dir = tempfile::tempdir().expect("dir");
        let providers = dir.path().join("providers");
        let home = prepare_home(&providers, ServerKind::LmStudio, 1234, "openai/gpt-oss-20b")
            .expect("home");
        assert_eq!(home, providers.join("codex/local-lmstudio-1234"));
        let config = std::fs::read_to_string(home.join("config.toml")).expect("config");
        assert!(
            config.contains("base_url = \"http://127.0.0.1:1234/v1\""),
            "{config}"
        );
        assert!(config.contains("model = \"openai/gpt-oss-20b\""));
        // Nothing else was written.
        let written: Vec<_> = walk(dir.path());
        assert_eq!(written, vec![home.join("config.toml")]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(home.join("config.toml"))
                .expect("meta")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn f_en_11_3_added_ports_round_trip_and_bad_entries_are_dropped() {
        let ports = BTreeMap::from([(11500, ServerKind::Ollama), (1235, ServerKind::LmStudio)]);
        let value = ports_value(&ports);
        assert_eq!(value, json!({"ports": ["lmstudio:1235", "ollama:11500"]}));
        assert_eq!(ports_from(&value), ports);
        let messy = json!({"ports": ["ollama:0", "vllm:9", 7, "llamacpp:8081"]});
        assert_eq!(
            ports_from(&messy),
            BTreeMap::from([(8081, ServerKind::LlamaCpp)])
        );
        assert!(ports_from(&json!({})).is_empty());
    }

    fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).expect("dir").flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
        out
    }
}
