//! Local models as engines (F-EN-11, UI-164): model servers the user runs on
//! this computer, their models, and a Codex pane configured for one in its
//! own provider home. Every request goes to `127.0.0.1` (D-2517).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::ids::TabId;
use tandem_core::settings::SettingKey;
use tandem_core::{ErrorCode, IpcError};
use tandem_engines::local_models::{self, CodexVersion, LocalModel, ServerKind};
use tandem_engines::EngineSpec;
use tandem_store::repo::panes;
use tandem_store::repo::sessions::{LaunchConfig, LocalLaunch};
use tandem_store::repo::settings;

use crate::commands::terminal::create_pane_with;
use crate::services::library::LibraryOpened;
use crate::state::AppState;

/// How long one server has to answer.
pub const PROBE_WAIT: Duration = Duration::from_secs(1);

/// How long `codex --version` has at a local start.
const VERSION_WAIT: Duration = Duration::from_secs(5);

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
    let mut models = body.as_ref().and_then(|b| local_models::parse(kind, b));
    // An OpenAI-style list doesn't say which models run elsewhere. When the
    // server is Ollama under another kind, its tags do: those models aren't
    // listed. A server that answers Ollama's version but not its tags lists
    // nothing, since which of its models are remote can't be known
    // (D-2517). Both are asked at once: up to 1 s more.
    if kind != ServerKind::Ollama {
        if let Some(list) = models.as_mut() {
            let version_url = local_models::url(port, "/api/version");
            let tags_url = local_models::url(port, ServerKind::Ollama.models_path());
            let (version, tags) =
                futures::join!(http.get_json(&version_url), http.get_json(&tags_url));
            // Tags without a `models` list say nothing about which models
            // are remote: no answer.
            let tags = tags.filter(|t| t.get("models").is_some_and(serde_json::Value::is_array));
            match tags {
                Some(tags) => {
                    let remote = local_models::ollama_remote_ids(&tags);
                    list.retain(|m| !remote.contains(&m.id));
                }
                None if version.as_ref().is_some_and(local_models::ollama_version) => {
                    list.clear();
                }
                None => {}
            }
        }
    }
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
    // Read now, from the program Start would run, so Check again shows the
    // Codex there is, not the one detected when the app started (D-3069).
    let codex = match codex_program(state).await {
        Some((engine, program)) => Some(read_codex(&engine, &program).await),
        None => None,
    };
    Ok(with_codex_reason(found, codex.as_ref()))
}

/// Start is off, saying why, where Codex was found at a version a local
/// pane can't run (D-3062). Not found, the page says to install it.
pub fn with_codex_reason(
    mut servers: Vec<LocalServer>,
    codex: Option<&Result<CodexVersion, String>>,
) -> Vec<LocalServer> {
    if let Some(Err(reason)) = codex {
        for s in servers.iter_mut().filter(|s| s.startable) {
            s.startable = false;
            s.start_reason = Some(reason.clone());
        }
    }
    servers
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
/// written for that model, `untrusted` listed as untrusted projects. Only
/// this folder is written, kept private (F-EN-11 AC2); a `config.toml`
/// that can't be read is refused, never replaced.
pub fn prepare_home(
    providers: &std::path::Path,
    kind: ServerKind,
    port: u16,
    model: &str,
    untrusted: &[String],
) -> Result<PathBuf, IpcError> {
    let io =
        |e: std::io::Error| IpcError::new(ErrorCode::Io, format!("Couldn't make the folder: {e}"));
    let codex = providers.join("codex");
    let home = codex.join(local_models::home_name(kind, port, model));
    for dir in [providers, codex.as_path(), home.as_path()] {
        tandem_engines::homes::private_dir(dir).map_err(io)?;
    }
    write_config(&home, kind, port, model, untrusted)?;
    Ok(home)
}

/// Merges the local provider into the home's `config.toml`: the model, the
/// `vibezeo-local` provider at 127.0.0.1, no update check, and the pane's
/// folders as untrusted projects (D-3065). Start writes it, and so does
/// every later start of the pane, so a deleted or edited file never leaves
/// Codex on its default provider or a folder trusted (D-2519). A file that
/// can't be read is refused, never replaced.
pub fn write_config(
    home: &std::path::Path,
    kind: ServerKind,
    port: u16,
    model: &str,
    untrusted: &[String],
) -> Result<(), IpcError> {
    // The status hook and the tasks server are written under the same
    // lock, so a start keeps what another wrote (D-3070, D-3073).
    let lock = tandem_engines::homes::config_lock(home);
    let _held = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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
    let config = local_models::codex_config(&existing, kind, port, model, untrusted)
        .map_err(|e| IpcError::new(ErrorCode::Invalid, e.to_string()))?;
    tandem_core::atomic::atomic_write(&file, config.as_bytes(), Some(0o600)).map_err(|e| {
        IpcError::new(
            ErrorCode::Io,
            format!("Couldn't write Codex's settings: {e}"),
        )
    })
}

/// A pane's saved home is that model's own under `$PROVIDERS/codex/`, never
/// a folder a saved row points elsewhere.
pub fn own_home(
    providers: &std::path::Path,
    home: &std::path::Path,
    local: &LocalLaunch,
) -> Result<ServerKind, IpcError> {
    let kind = kind_of(local)?;
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
    Ok(kind)
}

/// [`write_config`] for a pane's saved launch, before each start, with the
/// folders it starts in untrusted: only into that model's [`own_home`].
pub fn refresh_config(
    providers: &std::path::Path,
    home: &std::path::Path,
    local: &LocalLaunch,
    untrusted: &[String],
) -> Result<(), IpcError> {
    let kind = own_home(providers, home, local)?;
    write_config(home, kind, local.port, &local.model, untrusted)
}

/// A saved launch's server kind, one Codex can start on.
fn kind_of(local: &LocalLaunch) -> Result<ServerKind, IpcError> {
    ServerKind::ALL
        .into_iter()
        .find(|k| k.slug() == local.kind)
        .filter(|k| k.startable())
        .ok_or_else(|| IpcError::new(ErrorCode::Invalid, "This pane's model server isn't known"))
}

/// The command-line settings every launch of a local pane carries
/// (D-2519, D-3063, D-3065): the model and the local provider, the pane's
/// folders untrusted and its folder to start in, above any project's
/// configuration.
pub fn launch_overrides(
    local: &LocalLaunch,
    folder: &Path,
    untrusted: &[String],
) -> Result<Vec<String>, IpcError> {
    let kind = kind_of(local)?;
    local_models::codex_overrides(
        kind,
        local.port,
        &local.model,
        &folder.to_string_lossy(),
        untrusted,
    )
    .ok_or_else(|| {
        IpcError::new(
            ErrorCode::Unavailable,
            crate::services::engines::LAUNCH_CONFIG_UNAVAILABLE,
        )
        .with_stage("launch_config")
    })
}

/// The Codex program Start would run: the registry's entry and where it's
/// installed. `None` when it isn't.
async fn codex_program(state: &AppState) -> Option<(EngineSpec, PathBuf)> {
    let codex = state.terminals.engines.get("codex")?;
    let program = state.terminals.resolve_engine(&codex).await?;
    Some((codex, program))
}

/// `program --version`, read now, as a local pane may run it (D-3062).
async fn read_codex(engine: &EngineSpec, program: &Path) -> Result<CodexVersion, String> {
    let output = tandem_engines::detect::version_output(engine, program, VERSION_WAIT).await;
    local_models::check_codex(output.as_deref())
}

/// The Codex a local pane would run, read from that program at this start
/// (D-3062, D-3066): 0.149.0 or newer, or refused saying what's needed.
pub async fn codex_for_local(engine: &EngineSpec, path: &Path) -> Result<CodexVersion, IpcError> {
    read_codex(engine, path).await.map_err(|reason| {
        IpcError::new(ErrorCode::EngineUnsupported, reason).with_stage("local_model")
    })
}

/// Before every start of a local pane: the server still offers the model,
/// and as one that runs on this computer (D-2519). A name re-pointed since
/// Start (`ollama cp` of a cloud model) or a server that's gone refuses,
/// rather than sending prompts elsewhere.
pub async fn still_offered(local: &LocalLaunch) -> Result<(), IpcError> {
    let kind = kind_of(local)?;
    let server = ask(http()?.as_ref(), kind, local.port, false).await;
    check_offered(&server, &local.model)
}

/// [`still_offered`]'s answer for a server already asked.
pub fn check_offered(server: &LocalServer, model: &str) -> Result<(), IpcError> {
    if !server.running {
        return Err(IpcError::new(
            ErrorCode::Unavailable,
            format!(
                "{} isn't running on port {}. Start it, then try again.",
                server.kind.label(),
                server.port
            ),
        )
        .with_stage("local_model"));
    }
    if !server.models.iter().any(|m| m.id == model) {
        return Err(IpcError::new(
            ErrorCode::NotFound,
            format!(
                "{} on port {} no longer offers {model} as a model on this computer.",
                server.kind.label(),
                server.port
            ),
        )
        .with_stage("local_model"));
    }
    Ok(())
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
    let Some(program) = state.terminals.resolve_engine(&codex).await else {
        return Err(IpcError::new(
            ErrorCode::EngineMissing,
            "Install Codex to use a local model",
        ));
    };
    codex_for_local(&codex, &program).await?;
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
    let home = prepare_home(
        engines.homes.root(),
        kind,
        port,
        model,
        &local_models::untrusted_folders(Path::new(&ws.root)),
    )?;
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

    /// Answers the URLs it knows and records every request.
    struct Fake {
        answers: Vec<(String, serde_json::Value)>,
        asked: std::sync::Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl LocalHttp for Fake {
        async fn get_json(&self, url: &str) -> Option<serde_json::Value> {
            self.asked.lock().expect("lock").push(url.to_string());
            self.answers
                .iter()
                .find(|(u, _)| u == url)
                .map(|(_, v)| v.clone())
        }
    }

    #[tokio::test]
    async fn f_en_11_1_a_server_answering_ollamas_tags_is_found_with_its_models() {
        let fake = Fake {
            answers: vec![(
                "http://127.0.0.1:11500/api/tags".into(),
                json!({"models": [{"name": "qwen3-coder:30b", "size": 18_556_701_696u64, "details": {"parameter_size": "30.5B"}}]}),
            )],
            asked: Default::default(),
        };
        let found = find(&fake, &BTreeMap::from([(11500, ServerKind::Ollama)])).await;
        let ours = found.iter().find(|s| s.port == 11500).expect("listed");
        assert!(ours.running && ours.added);
        // Start is off on Windows for now (D-2517).
        assert_eq!(ours.startable, !cfg!(windows));
        assert_eq!(ours.start_reason.is_none(), !cfg!(windows));
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
        let here = vec!["/".to_string(), "/home/u/repo".to_string()];
        let home = prepare_home(
            &providers,
            ServerKind::LmStudio,
            1234,
            "openai/gpt-oss-20b",
            &here,
        )
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
        assert!(
            config.contains("[projects.\"/home/u/repo\"]\ntrust_level = \"untrusted\""),
            "{config}"
        );
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
        let other = prepare_home(
            &providers,
            ServerKind::LmStudio,
            1234,
            "qwen/qwen3-8b",
            &here,
        )
        .expect("other");
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
                assert!(prepare_home(
                    &providers,
                    ServerKind::LmStudio,
                    1234,
                    "qwen/qwen3-8b",
                    &here
                )
                .is_err());
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

    /// F-EN-11 review: Ollama added as another kind lists its cloud models
    /// in `/v1/models` with nothing to mark them; its tags say which are
    /// remote, and those aren't listed.
    #[tokio::test]
    async fn f_en_11_1_ollama_added_as_another_kind_still_hides_its_cloud_models() {
        let fake = Fake {
            answers: vec![
                (
                    "http://127.0.0.1:11600/v1/models".into(),
                    json!({"object": "list", "data": [
                        {"id": "my-alias:latest", "object": "model"},
                        {"id": "gemma3:4b", "object": "model"}
                    ]}),
                ),
                (
                    "http://127.0.0.1:11600/api/tags".into(),
                    json!({"models": [
                        {"name": "my-alias:latest", "remote_host": "https://ollama.com:443", "remote_model": "gpt-oss:120b"},
                        {"name": "gemma3:4b", "size": 3_338_801_804u64}
                    ]}),
                ),
            ],
            asked: Default::default(),
        };
        let server = ask(&fake, ServerKind::LmStudio, 11600, true).await;
        let ids: Vec<&str> = server.models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["gemma3:4b"]);
        // A server that isn't Ollama doesn't answer the tags: nothing hidden.
        let lm = Fake {
            answers: vec![(
                "http://127.0.0.1:1234/v1/models".into(),
                json!({"data": [{"id": "openai/gpt-oss-20b"}]}),
            )],
            asked: Default::default(),
        };
        let server = ask(&lm, ServerKind::LmStudio, 1234, false).await;
        assert_eq!(server.models.len(), 1);
        assert!(lm
            .asked
            .lock()
            .expect("lock")
            .iter()
            .all(|u| u.starts_with("http://127.0.0.1:1234/")));
    }

    /// F-EN-11 review: a server that answers Ollama's version but not its
    /// tags can't say which of its models run elsewhere, so none are listed.
    #[tokio::test]
    async fn f_en_11_1_ollama_without_its_tags_lists_no_models_under_another_kind() {
        let fake = Fake {
            answers: vec![
                (
                    "http://127.0.0.1:11700/v1/models".into(),
                    json!({"data": [{"id": "my-alias:latest"}, {"id": "gemma3:4b"}]}),
                ),
                (
                    "http://127.0.0.1:11700/api/version".into(),
                    json!({"version": "0.12.6"}),
                ),
            ],
            asked: Default::default(),
        };
        let server = ask(&fake, ServerKind::LlamaCpp, 11700, true).await;
        assert!(server.running);
        assert!(server.models.is_empty(), "{:?}", server.models);
        // Something else that happens to answer a version-shaped body
        // without one isn't taken for Ollama.
        let other = Fake {
            answers: vec![
                (
                    "http://127.0.0.1:11701/v1/models".into(),
                    json!({"data": [{"id": "gemma3:4b"}]}),
                ),
                (
                    "http://127.0.0.1:11701/api/version".into(),
                    json!({"version": ""}),
                ),
            ],
            asked: Default::default(),
        };
        let server = ask(&other, ServerKind::LlamaCpp, 11701, true).await;
        assert_eq!(server.models.len(), 1);
        // Review N8: Ollama's version with a tags answer that has no list
        // of models is no answer either.
        let listless = Fake {
            answers: vec![
                (
                    "http://127.0.0.1:11702/v1/models".into(),
                    json!({"data": [{"id": "my-alias:latest"}]}),
                ),
                (
                    "http://127.0.0.1:11702/api/version".into(),
                    json!({"version": "0.12.6"}),
                ),
                (
                    "http://127.0.0.1:11702/api/tags".into(),
                    json!({"error": "busy"}),
                ),
            ],
            asked: Default::default(),
        };
        let server = ask(&listless, ServerKind::LmStudio, 11702, true).await;
        assert!(server.models.is_empty(), "{:?}", server.models);
        // Not Ollama (no version) with the same tags answer: nothing hidden.
        let lm = Fake {
            answers: vec![
                (
                    "http://127.0.0.1:11703/v1/models".into(),
                    json!({"data": [{"id": "gemma3:4b"}]}),
                ),
                (
                    "http://127.0.0.1:11703/api/tags".into(),
                    json!({"error": "Unexpected endpoint"}),
                ),
            ],
            asked: Default::default(),
        };
        assert_eq!(
            ask(&lm, ServerKind::LmStudio, 11703, true)
                .await
                .models
                .len(),
            1
        );
        // Only this computer was asked, and only the three paths.
        let asked = fake.asked.lock().expect("lock").clone();
        assert_eq!(asked.len(), 3, "{asked:?}");
        assert!(asked
            .iter()
            .all(|u| u.starts_with("http://127.0.0.1:11700/")));
    }

    fn server(running: bool, models: &[&str]) -> LocalServer {
        LocalServer {
            kind: ServerKind::Ollama,
            port: 11434,
            added: false,
            running,
            models: models
                .iter()
                .map(|id| local_models::LocalModel {
                    id: (*id).to_string(),
                    size: None,
                    parameters: None,
                })
                .collect(),
            startable: true,
            start_reason: None,
        }
    }

    /// F-EN-11 review (N3): with a Codex too old to keep a local pane's
    /// model, Start is off on every server that had it, saying why; servers
    /// already off keep their own reason, and no Codex leaves the page's.
    #[test]
    fn f_en_11_2_start_is_off_with_the_reason_for_an_old_codex() {
        let codex = |output: &str| local_models::check_codex(Some(output));
        let llama = LocalServer {
            kind: ServerKind::LlamaCpp,
            startable: false,
            start_reason: Some("Codex can't use llama.cpp servers yet".into()),
            ..server(true, &["gpt-oss-20b"])
        };
        let both = || vec![server(true, &["qwen3:8b"]), llama.clone()];
        let old = with_codex_reason(both(), Some(&codex("codex-cli 0.148.0\n")));
        assert!(!old[0].startable);
        assert_eq!(
            old[0].start_reason.as_deref(),
            Some("Local models need Codex 0.149.0 or newer, and this computer has Codex 0.148.0. Update Codex, then try again.")
        );
        assert_eq!(old[1].start_reason, llama.start_reason);
        let unknown = with_codex_reason(both(), Some(&codex("")));
        assert!(unknown[0]
            .start_reason
            .as_deref()
            .is_some_and(|r| r.contains("couldn't tell")));
        // Recent enough, or not installed: as asked.
        assert_eq!(
            with_codex_reason(both(), Some(&codex("codex-cli 0.165.0\n"))),
            both()
        );
        assert_eq!(with_codex_reason(both(), None), both());
    }

    /// F-EN-11 follow-up (D-3070, D-3073): panes on one model starting at
    /// once each keep their folders in the home's `config.toml`, untrusted,
    /// and the status hook and the tasks server stay in it: each of
    /// Vibezeo's writers of the file (a start's local settings, then the
    /// hook, then the tasks server) reads, changes and writes it one at a
    /// time.
    #[test]
    fn f_en_11_2_starts_at_once_keep_every_folder_in_the_home() {
        const PANES: usize = 24;
        let registry = Arc::new(tandem_engines::registry::EngineRegistry::bundled());
        let server = Arc::new(tandem_engines::chat::StdioServer {
            name: "tandem_tasks".into(),
            command: "/x/tandem-tasks".into(),
            args: vec!["--stdio".into()],
            env_vars: Vec::new(),
        });
        // Several rounds: with any writer outside the lock, most lose some.
        for round in 0..4 {
            let dir = tempfile::tempdir().expect("dir");
            let providers = dir.path().join("providers");
            let homes = Arc::new(tandem_engines::homes::ProviderHomes::new(&providers));
            let home = providers.join("codex").join(local_models::home_name(
                ServerKind::Ollama,
                11434,
                "qwen3:8b",
            ));
            std::fs::create_dir_all(&home).expect("home");
            let barrier = Arc::new(std::sync::Barrier::new(PANES));
            let starts: Vec<_> = (0..PANES)
                .map(|i| {
                    let (home, barrier) = (home.clone(), Arc::clone(&barrier));
                    let (homes, registry, server) = (
                        Arc::clone(&homes),
                        Arc::clone(&registry),
                        Arc::clone(&server),
                    );
                    std::thread::spawn(move || {
                        let codex = registry.get("codex").expect("codex");
                        barrier.wait();
                        write_config(
                            &home,
                            ServerKind::Ollama,
                            11434,
                            "qwen3:8b",
                            &[format!("/work/pane-{i}")],
                        )
                        .expect("written");
                        homes
                            .install_hooks(codex, &home, Path::new("/x/tandem-hook"))
                            .expect("hook");
                        homes
                            .set_stdio_server(codex, &home, "tandem_tasks", Some(&server))
                            .expect("tasks");
                    })
                })
                .collect();
            for start in starts {
                start.join().expect("thread");
            }
            let config = std::fs::read_to_string(home.join("config.toml")).expect("config");
            for i in 0..PANES {
                assert!(
                    config.contains(&format!(
                        "[projects.\"/work/pane-{i}\"]\ntrust_level = \"untrusted\""
                    )),
                    "round {round}: pane-{i} lost:\n{config}"
                );
            }
            assert!(
                config.contains(r#"notify = ["/x/tandem-hook", "codex"]"#),
                "round {round}: the hook lost:\n{config}"
            );
            assert!(
                config.contains("[mcp_servers.tandem_tasks]"),
                "round {round}: the tasks server lost:\n{config}"
            );
        }
    }

    /// F-EN-11 review (S3, D-3069): the page's Check again
    /// (`local_models_list`) reads the version of the Codex Start would run
    /// each time, so a Codex updated or rolled back after the app started
    /// turns Start off or on again without a restart.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn f_en_11_2_check_again_reads_codexs_version_now() {
        use std::os::unix::fs::PermissionsExt;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = tempfile::tempdir().expect("dir");
        let data = tandem_core::DataDir::at(dir.path().join("data")).expect("data");
        let store = tandem_store::Store::open(&data).expect("store");
        let state = AppState::for_tests(data, store);
        // A stand-in Ollama with one model, at an added port.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let answer = if String::from_utf8_lossy(&buf[..n]).starts_with("GET /api/tags ")
                    {
                        let body = r#"{"models":[{"name":"qwen3:8b","size":1}]}"#;
                        format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        )
                    } else {
                        "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                            .to_string()
                    };
                    let _ = sock.write_all(answer.as_bytes()).await;
                });
            }
        });
        save_ports(&state, &BTreeMap::from([(port, ServerKind::Ollama)]))
            .await
            .expect("ports");
        // A Codex on the login PATH whose `--version` is the file beside it.
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        let program = bin.join("codex");
        std::fs::write(&program, "#!/bin/sh\ncat \"$(dirname \"$0\")/version\"\n").expect("fake");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        state.terminals.set_login_env(tandem_pty::env::LoginEnv {
            vars: BTreeMap::from([(
                "PATH".to_string(),
                format!("{}:/usr/bin:/bin", bin.display()),
            )]),
            shell: "/bin/sh".into(),
            degraded: false,
        });
        for (printed, reason) in [
            ("codex-cli 0.165.0\n", None),
            (
                "codex-cli 0.148.0\n",
                Some("Local models need Codex 0.149.0 or newer, and this computer has Codex 0.148.0. Update Codex, then try again."),
            ),
            ("codex-cli 0.149.0\n", None),
        ] {
            std::fs::write(bin.join("version"), printed).expect("version");
            let ours = list(&state)
                .await
                .expect("list")
                .into_iter()
                .find(|s| s.port == port)
                .expect("ours");
            assert!(ours.running, "{ours:?}");
            assert_eq!(ours.startable, reason.is_none(), "{printed}");
            assert_eq!(ours.start_reason.as_deref(), reason, "{printed}");
        }
    }

    /// F-EN-11 review (S2): every start asks again; a model gone, re-pointed
    /// to one that runs elsewhere, or a server that's stopped refuses.
    #[test]
    fn f_en_11_2_a_start_needs_the_model_still_offered_here() {
        assert!(check_offered(&server(true, &["qwen3:8b"]), "qwen3:8b").is_ok());
        let err = check_offered(&server(true, &["gemma3:4b"]), "qwen3:8b").expect_err("gone");
        assert_eq!(err.code, ErrorCode::NotFound);
        assert_eq!(
            err.message,
            "Ollama on port 11434 no longer offers qwen3:8b as a model on this computer."
        );
        let err = check_offered(&server(false, &[]), "qwen3:8b").expect_err("stopped");
        assert_eq!(err.code, ErrorCode::Unavailable);
        assert_eq!(
            err.message,
            "Ollama isn't running on port 11434. Start it, then try again."
        );
        // A name that differs only in case is another model.
        assert!(check_offered(&server(true, &["Qwen3:8b"]), "qwen3:8b").is_err());
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
