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
use tandem_store::repo::sessions::{LaunchConfig, LocalLaunch};
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
    /// Why not, when it can't: llama.cpp, or Windows for now (D-2517).
    pub start_reason: Option<String>,
}

/// Why Codex can't start on a server's models here, if it can't.
pub fn start_reason(kind: ServerKind) -> Option<String> {
    if cfg!(windows) {
        Some(local_models::NOT_ON_WINDOWS.to_string())
    } else if !kind.startable() {
        Some(format!("Codex can't use {} servers yet", kind.label()))
    } else {
        None
    }
}

/// Asks a server on this computer for a path; the trait is how the tests
/// see every request.
#[async_trait::async_trait]
pub trait LocalHttp: Send + Sync {
    /// `None`: nothing answered, or not with JSON.
    async fn get_json(&self, url: &str) -> Option<serde_json::Value>;
}

/// Most a server's answer may be.
pub const MAX_BODY: usize = 4 * 1024 * 1024;

/// A URL the finder may ask: plain HTTP to `127.0.0.1` with a port and no
/// user information (D-2517).
pub fn loopback_url(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "http"
            && u.host_str() == Some("127.0.0.1")
            && u.username().is_empty()
            && u.password().is_none()
            // `port()` is None for http's own 80, which can be added.
            && u.port_or_known_default().is_some()
    })
}

/// The real client: no proxy (a proxy would send the request off the
/// machine), a 1 s limit, no redirects, at most [`MAX_BODY`] read.
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
        if !loopback_url(url) {
            return None;
        }
        let mut res = self.0.get(url).send().await.ok()?;
        if !res.status().is_success() {
            return None;
        }
        if res.content_length().is_some_and(|n| n > MAX_BODY as u64) {
            return None;
        }
        // Read in chunks and stop past the cap: a server that never ends
        // its answer can't fill memory.
        let mut body = Vec::new();
        while let Some(chunk) = res.chunk().await.ok()? {
            if body.len() + chunk.len() > MAX_BODY {
                return None;
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).ok()
    }
}

/// The ports to ask: every kind's default, with a port the user added in
/// place of the default that has its number (D-2517).
pub fn targets(added: &BTreeMap<u16, ServerKind>) -> Vec<(ServerKind, u16, bool)> {
    let mut out: Vec<(ServerKind, u16, bool)> = ServerKind::ALL
        .iter()
        .filter(|k| !added.contains_key(&k.default_port()))
        .map(|k| (*k, k.default_port(), false))
        .collect();
    out.extend(added.iter().map(|(port, kind)| (*kind, *port, true)));
    out
}

/// Asks one server for its models.
pub async fn ask(http: &dyn LocalHttp, kind: ServerKind, port: u16, added: bool) -> LocalServer {
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
        startable: start_reason(kind).is_none(),
        start_reason: start_reason(kind),
    }
}

/// Asks each target once, all at once.
pub async fn find(http: &dyn LocalHttp, added: &BTreeMap<u16, ServerKind>) -> Vec<LocalServer> {
    let asks = targets(added)
        .into_iter()
        .map(|(kind, port, added)| ask(http, kind, port, added));
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

/// Held across each Add and Remove's read, change and write.
static PORTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
    let _held = PORTS.lock().await;
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
    let _held = PORTS.lock().await;
    let mut ports = added_ports(state).await;
    ports.remove(&port);
    save_ports(state, &ports).await
}

/// The provider home a server's model runs Codex in, with its `config.toml`
/// written for that model. Only this folder is written, kept private
/// (F-EN-11 AC2); a `config.toml` that can't be read is refused, never
/// replaced.
pub fn prepare_home(
    providers: &std::path::Path,
    kind: ServerKind,
    port: u16,
    model: &str,
) -> Result<PathBuf, IpcError> {
    let io =
        |e: std::io::Error| IpcError::new(ErrorCode::Io, format!("Couldn't make the folder: {e}"));
    let codex = providers.join("codex");
    let home = codex.join(local_models::home_name(kind, port, model));
    for dir in [providers, codex.as_path(), home.as_path()] {
        tandem_engines::homes::private_dir(dir).map_err(io)?;
    }
    write_config(&home, kind, port, model)?;
    Ok(home)
}

/// Merges the local provider into the home's `config.toml`: the model, the
/// `vibezeo-local` provider at 127.0.0.1 and no update check. Start writes
/// it, and so does every later start of the pane, so a deleted or edited
/// file never leaves Codex on its default provider (D-2519). A file that
/// can't be read is refused, never replaced.
pub fn write_config(
    home: &std::path::Path,
    kind: ServerKind,
    port: u16,
    model: &str,
) -> Result<(), IpcError> {
    let file = home.join("config.toml");
    let existing = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(IpcError::new(
                ErrorCode::Io,
                format!("Couldn't read Codex's settings in this model's home: {e}"),
            ))
        }
    };
    let config = local_models::codex_config(&existing, kind, port, model)
        .map_err(|e| IpcError::new(ErrorCode::Invalid, e.to_string()))?;
    tandem_core::atomic::atomic_write(&file, config.as_bytes(), Some(0o600)).map_err(|e| {
        IpcError::new(
            ErrorCode::Io,
            format!("Couldn't write Codex's settings: {e}"),
        )
    })
}

/// [`write_config`] for a pane's saved launch, before each start: only into
/// that model's own home under `$PROVIDERS/codex/`, never a folder a saved
/// row points elsewhere.
pub fn refresh_config(
    providers: &std::path::Path,
    home: &std::path::Path,
    local: &LocalLaunch,
) -> Result<(), IpcError> {
    let kind = ServerKind::ALL
        .into_iter()
        .find(|k| k.slug() == local.kind)
        .filter(|k| k.startable())
        .ok_or_else(|| IpcError::new(ErrorCode::Invalid, "This pane's model server isn't known"))?;
    let own = providers
        .join("codex")
        .join(local_models::home_name(kind, local.port, &local.model));
    if home != own {
        return Err(IpcError::new(
            ErrorCode::Unavailable,
            crate::services::engines::LAUNCH_CONFIG_UNAVAILABLE,
        )
        .with_stage("launch_config"));
    }
    write_config(home, kind, local.port, &local.model)
}

/// `local_model_start`: a Codex pane in `tab` for `model` on that server.
/// Every start of it runs Codex in the model's own home with no account
/// (D-2519).
pub async fn start(
    state: &AppState,
    tab: &TabId,
    kind: ServerKind,
    port: u16,
    model: &str,
) -> Result<LibraryOpened, IpcError> {
    if let Some(reason) = start_reason(kind) {
        return Err(IpcError::new(ErrorCode::EngineUnsupported, reason));
    }
    if !local_models::model_ok(model) {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "That model's name can't be used",
        ));
    }
    let tab_row = tandem_store::repo::tabs::get(&state.store, tab).await?;
    let ws = tandem_store::repo::workspaces::get(&state.store, &tab_row.workspace_id).await?;
    if ws.kind != "local" {
        return Err(IpcError::new(
            ErrorCode::EngineUnsupported,
            "A local model runs in a folder on this computer",
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
    let added = added_ports(state).await.get(&port) == Some(&kind);
    let server = ask(http()?.as_ref(), kind, port, added).await;
    if !server.models.iter().any(|m| m.id == model) {
        return Err(IpcError::new(
            ErrorCode::NotFound,
            format!(
                "{} on port {port} doesn't offer {model} right now",
                kind.label()
            ),
        ));
    }
    state
        .terminals
        .check_live_limit(state.entitlements.limits().panes)?;
    let home = prepare_home(engines.homes.root(), kind, port, model)?;
    let config = LaunchConfig {
        extra_args: local_models::local_args(&engines.extra_args("codex").await),
        home: Some(home.to_string_lossy().into_owned()),
        own_home: false,
        local: Some(LocalLaunch {
            kind: kind.slug().to_string(),
            port,
            model: model.to_string(),
        }),
    };
    let mut pane =
        create_pane_with(state, tab, "agent", None, Some("codex".into()), None, false).await?;
    let label = format!("{model} · {}", kind.label());
    // Never left behind as an ordinary Codex pane: one that couldn't be
    // marked local is closed again.
    if let Err(e) = panes::set_local_model(&state.store, &pane.id, &label, &config).await {
        let _ = panes::close(&state.store, &pane.id).await;
        return Err(e.into());
    }
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
        assert!(ours.running && ours.added && ours.startable && ours.start_reason.is_none());
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
        assert_eq!(
            home,
            providers.join(format!(
                "codex/{}",
                local_models::home_name(ServerKind::LmStudio, 1234, "openai/gpt-oss-20b")
            ))
        );
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
            for d in [providers.clone(), providers.join("codex"), home.clone()] {
                let mode = std::fs::metadata(&d).expect("meta").permissions().mode();
                assert_eq!(mode & 0o777, 0o700, "{}", d.display());
            }
        }
        // Another model of the same server gets a home of its own.
        let other =
            prepare_home(&providers, ServerKind::LmStudio, 1234, "qwen/qwen3-8b").expect("other");
        assert_ne!(other, home);
        assert!(std::fs::read_to_string(home.join("config.toml"))
            .expect("config")
            .contains("openai/gpt-oss-20b"));
        // A config.toml that can't be read is refused, never replaced.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file = other.join("config.toml");
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).expect("chmod");
            if std::fs::read_to_string(&file).is_err() {
                assert!(
                    prepare_home(&providers, ServerKind::LmStudio, 1234, "qwen/qwen3-8b").is_err()
                );
            }
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))
                .expect("chmod back");
        }
    }

    #[test]
    fn f_en_11_3_only_plain_http_to_127_0_0_1_with_a_port_is_asked() {
        assert!(loopback_url("http://127.0.0.1:11434/api/tags"));
        assert!(loopback_url(&local_models::url(80, "/v1/models")));
        for url in [
            "http://127.0.0.1:80@example.com/",
            "http://user@127.0.0.1:11434/api/tags",
            "https://127.0.0.1:11434/api/tags",
            "http://localhost:11434/api/tags",
            "http://192.168.1.5:11434/api/tags",
            "not a url",
        ] {
            assert!(!loopback_url(url), "{url}");
        }
    }

    /// F-EN-11 review: an answer that never ends is cut off at 4 MB.
    #[tokio::test]
    async fn f_en_11_3_an_endless_answer_is_not_read_past_the_cap() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let sent = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = sent.clone();
        tokio::spawn(async move {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf).await;
            let _ = sock
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ntransfer-encoding: chunked\r\n\r\n")
                .await;
            let chunk = vec![b' '; 64 * 1024];
            let framed = [
                format!("{:x}\r\n", chunk.len()).into_bytes(),
                chunk,
                b"\r\n".to_vec(),
            ]
            .concat();
            while sock.write_all(&framed).await.is_ok() {
                if counter.fetch_add(64 * 1024, std::sync::atomic::Ordering::SeqCst)
                    > 64 * 1024 * 1024
                {
                    break;
                }
            }
        });
        let client = Reqwest::new().expect("client");
        let url = local_models::url(port, "/api/tags");
        assert!(client.get_json(&url).await.is_none());
        // Stopped soon after the cap, not after reading everything sent.
        assert!(sent.load(std::sync::atomic::Ordering::SeqCst) < 32 * 1024 * 1024);
    }

    #[test]
    fn f_en_11_3_an_added_port_takes_the_place_of_a_default_with_its_number() {
        let t = targets(&BTreeMap::from([(8080, ServerKind::Ollama)]));
        assert!(t.contains(&(ServerKind::Ollama, 8080, true)));
        assert!(!t.contains(&(ServerKind::LlamaCpp, 8080, false)));
        assert_eq!(t.len(), 3);
    }

    #[test]
    fn f_en_11_3_added_ports_round_trip_and_bad_entries_are_dropped() {
        let ports = BTreeMap::from([(11500, ServerKind::Ollama), (1235, ServerKind::LmStudio)]);
        let value = ports_value(&ports);
        assert_eq!(value, json!({"ports": ["lm_studio:1235", "ollama:11500"]}));
        assert_eq!(ports_from(&value), ports);
        let messy = json!({"ports": ["ollama:0", "vllm:9", 7, "llama_cpp:8081"]});
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
