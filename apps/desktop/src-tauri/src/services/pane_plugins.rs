//! Plugins for agent terminals (F-PL-01, F-PL-05): a Claude Code or Codex
//! pane started in a workspace with plugins granted to it, or to the folder
//! a worktree came from, gets the gateway for that run, beside its own MCP
//! servers (F-EN-04). Its lease is an interactive Edits session whose
//! approvals show on the pane, and it's revoked when the pane's process
//! ends or the pane closes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use tandem_core::access::Access;
use tandem_core::ids::{ConnectionId, LeaseId, PaneId, WorkspaceId};
use tandem_gateway::{Attended, BuiltinSet, Caller, GatewayHandle, SessionDescriptor};
use tandem_store::repo::plugins::GrantSubject;
use tandem_store::Store;

/// Codex reads the lease from here (`bearer_token_env_var`), never from its
/// arguments.
pub use tandem_engines::chat::codex::LEASE_ENV;

/// What a pane's engine gets for the gateway: arguments that go right after
/// its program, and environment.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct GatewayLaunch {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// The arguments and environment that give `engine` the gateway at `url`
/// with the lease `token`: Claude Code's `--mcp-config=<file>`, written here
/// for only the user to read, and Codex's `-c mcp_servers.tandem.*` with the
/// lease in [`LEASE_ENV`]. Other engines get none.
pub fn launch(
    engine: &str,
    url: &str,
    token: &str,
    file: &Path,
) -> std::io::Result<Option<GatewayLaunch>> {
    match engine {
        "claude" => {
            let config = serde_json::json!({"mcpServers": {"tandem": {
                "type": "http",
                "url": url,
                "headers": {"Authorization": format!("Bearer {token}")}
            }}});
            tandem_core::atomic::atomic_write(file, config.to_string().as_bytes(), Some(0o600))?;
            // Its `=` form: the option takes several values, so a value
            // given apart from it would take the next argument as well.
            Ok(Some(GatewayLaunch {
                args: vec![format!("--mcp-config={}", file.to_string_lossy())],
                env: Vec::new(),
            }))
        }
        "codex" => Ok(Some(GatewayLaunch {
            args: vec![
                "-c".into(),
                format!("mcp_servers.tandem.url={}", toml_string(url)),
                "-c".into(),
                format!(
                    "mcp_servers.tandem.bearer_token_env_var={}",
                    toml_string(LEASE_ENV)
                ),
            ],
            env: vec![(LEASE_ENV.to_string(), token.to_string())],
        })),
        _ => Ok(None),
    }
}

fn toml_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// What a pane's start gets for the gateway, and the file its sandbox must
/// let it read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneStart {
    pub launch: GatewayLaunch,
    pub file: Option<PathBuf>,
}

impl PaneStart {
    /// `argv` with the gateway's arguments right after its program.
    pub fn argv(&self, mut argv: Vec<String>) -> Vec<String> {
        if !argv.is_empty() {
            argv.splice(1..1, self.launch.args.iter().cloned());
        }
        argv
    }
}

/// The gateway agent terminals get when their workspace has plugins.
pub struct PaneGateway {
    pub service: Arc<crate::services::gateway::GatewayService>,
    pub leases: Arc<PaneLeases>,
    /// `$DATA/mcp`, where Claude Code's configs go, one per pane.
    pub dir: PathBuf,
}

impl PaneGateway {
    /// Configs a run before left behind, when the app stopped without
    /// ending its panes: their leases died with it, and the files go too.
    pub fn clear_stale(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("pn_") && name.ends_with(".json") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    /// The gateway for this run of `pane`, an agent terminal of `engine` in
    /// `workspace`: none when the engine takes none or nothing is granted.
    /// The run before's lease is revoked first.
    pub async fn start(
        &self,
        store: &Store,
        pane: &PaneId,
        workspace: &WorkspaceId,
        engine: &str,
    ) -> Option<PaneStart> {
        self.end(pane.as_str());
        if !matches!(engine, "claude" | "codex") {
            return None;
        }
        let granted = grants(store, workspace).await;
        if granted.is_empty() {
            return None;
        }
        let gateway = match self.service.handle().await {
            Ok(g) => g.clone(),
            Err(e) => {
                tracing::warn!(code = "gateway.pane_unavailable", error = %e.message, "an agent terminal starts without its plugins");
                return None;
            }
        };
        let mut session = SessionDescriptor::new(
            Caller::Pane {
                pane_id: pane.clone(),
            },
            Attended::Interactive,
            Access::Edits,
            BuiltinSet::default(),
        );
        session.granted = granted;
        let lease = gateway.prepare(session);
        let file = self.dir.join(format!("{}.json", pane.as_str()));
        match launch(engine, &gateway.url, &lease.token, &file) {
            Ok(Some(launch)) => {
                let file = (engine == "claude").then_some(file);
                self.leases
                    .hold(pane.as_str(), gateway, lease.id, file.clone());
                Some(PaneStart { launch, file })
            }
            Ok(None) => {
                gateway.revoke(&lease.id);
                None
            }
            Err(e) => {
                gateway.revoke(&lease.id);
                tracing::warn!(code = "gateway.pane_config_failed", error = %e, "an agent terminal starts without its plugins");
                None
            }
        }
    }

    /// The pane's process ended or the pane closed.
    pub fn end(&self, pane: &str) {
        self.leases.end(pane);
    }
}

/// Whose grants a session in `workspace` has (F-PL-05): the workspace's,
/// then, for a worktree, the folder's it came from.
pub async fn workspace_subjects(store: &Store, workspace: &WorkspaceId) -> Vec<GrantSubject> {
    let subject = |id: &WorkspaceId| GrantSubject::Workspace {
        workspace_id: id.as_str().to_string(),
    };
    let mut out = vec![subject(workspace)];
    if let Ok(ws) = tandem_store::repo::workspaces::get(store, workspace).await {
        if let Some(folder) = ws.parent_id {
            out.push(subject(&folder));
        }
    }
    out
}

/// The plugins an agent terminal in `workspace` may use: its workspace's
/// grants, and for a worktree its folder's too.
pub async fn grants(store: &Store, workspace: &WorkspaceId) -> Vec<ConnectionId> {
    let mut out: Vec<ConnectionId> = Vec::new();
    for subject in workspace_subjects(store, workspace).await {
        for c in tandem_store::repo::plugins::grants(store, &subject)
            .await
            .unwrap_or_default()
        {
            if !out.contains(&c) {
                out.push(c);
            }
        }
    }
    out
}

struct Held {
    gateway: GatewayHandle,
    lease: LeaseId,
    file: Option<PathBuf>,
}

impl Held {
    /// Revokes the lease; the config goes too unless `keep` is it.
    fn end(self, keep: Option<&Path>) {
        self.gateway.revoke(&self.lease);
        if let Some(file) = self.file.filter(|f| Some(f.as_path()) != keep) {
            let _ = std::fs::remove_file(file);
        }
    }
}

/// Each agent pane's gateway lease, while its process runs.
#[derive(Default)]
pub struct PaneLeases {
    held: Mutex<HashMap<String, Held>>,
    /// Where the panes' plugin questions are: they go with the lease.
    broker: OnceLock<Arc<tandem_core::approvals::ApprovalBroker>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl PaneLeases {
    /// Keeps the pane's lease for this run. One from a run before is
    /// revoked, its config kept when this run's is the same file.
    pub fn hold(&self, pane: &str, gateway: GatewayHandle, lease: LeaseId, file: Option<PathBuf>) {
        let keep = file.clone();
        let before = lock(&self.held).insert(
            pane.to_string(),
            Held {
                gateway,
                lease,
                file,
            },
        );
        if let Some(before) = before {
            before.end(keep.as_deref());
        }
    }

    /// The pane's process ended or the pane closed: its lease is revoked,
    /// its config file deleted, and the plugin questions it left asking
    /// are cancelled (an Allow there would be refused anyway, the session
    /// being over).
    pub fn end(&self, pane: &str) {
        let held = lock(&self.held).remove(pane);
        if let Some(held) = held {
            held.end(None);
        }
        let Some(broker) = self.broker.get() else {
            return;
        };
        let source = tandem_core::source::SourceRef::Pane {
            pane_id: pane.to_string(),
        };
        for view in broker.pending() {
            let plugin = matches!(
                view.kind,
                tandem_core::approvals::ApprovalKind::PluginAction
                    | tandem_core::approvals::ApprovalKind::Spend
            );
            if plugin && view.source == source {
                broker.cancel(&view.id);
            }
        }
    }

    /// Where its plugin questions are, so they go with a pane's lease.
    pub fn bind_broker(&self, broker: Arc<tandem_core::approvals::ApprovalBroker>) {
        let _ = self.broker.set(broker);
    }

    /// Whether the pane holds a lease.
    pub fn holds(&self, pane: &str) -> bool {
        lock(&self.held).contains_key(pane)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const URL: &str = "http://127.0.0.1:4100/mcp";
    const TOKEN: &str = "lease-token-0123456789abcdef0123456789abcdef";

    #[test]
    fn f_pl_01_5_claude_gets_the_gateway_in_a_config_only_the_user_reads() {
        let dir = tempfile::tempdir().expect("dir");
        let file = dir.path().join("mcp").join("pn_1.json");
        let got = launch("claude", URL, TOKEN, &file)
            .expect("written")
            .expect("claude gets it");
        // One argument, its `=` form: a value Claude's option can't take
        // more of.
        assert_eq!(got.args, vec![format!("--mcp-config={}", file.display())]);
        assert!(got.env.is_empty());
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).expect("file")).expect("json");
        assert_eq!(config["mcpServers"]["tandem"]["type"], "http");
        assert_eq!(config["mcpServers"]["tandem"]["url"], URL);
        assert_eq!(
            config["mcpServers"]["tandem"]["headers"]["Authorization"],
            format!("Bearer {TOKEN}")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn f_pl_01_5_codex_gets_the_lease_only_in_its_environment() {
        let dir = tempfile::tempdir().expect("dir");
        let file = dir.path().join("pn_2.json");
        let got = launch("codex", URL, TOKEN, &file)
            .expect("ok")
            .expect("codex gets it");
        assert_eq!(
            got.args,
            vec![
                "-c".to_string(),
                format!("mcp_servers.tandem.url=\"{URL}\""),
                "-c".to_string(),
                "mcp_servers.tandem.bearer_token_env_var=\"TANDEM_GATEWAY_LEASE\"".to_string(),
            ]
        );
        assert!(got.args.iter().all(|a| !a.contains(TOKEN)));
        assert_eq!(got.env, vec![(LEASE_ENV.to_string(), TOKEN.to_string())]);
        assert!(!file.exists(), "nothing written for Codex");
    }

    #[test]
    fn f_pl_01_5_other_engines_get_no_gateway() {
        let dir = tempfile::tempdir().expect("dir");
        let file = dir.path().join("pn_3.json");
        for engine in ["gemini", "aider", "opencode", "shell"] {
            assert_eq!(
                launch(engine, URL, TOKEN, &file).expect("ok"),
                None,
                "{engine}"
            );
        }
        assert!(!file.exists());
    }

    async fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("dir");
        let data = tandem_core::DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        (dir, store)
    }

    /// A folder, a worktree of it and another folder, each with a plugin
    /// granted, and one granted to all threads.
    pub(crate) async fn granted_folders(
        store: &Store,
        dir: &Path,
    ) -> (HashMap<&'static str, ConnectionId>, [WorkspaceId; 3]) {
        use tandem_store::repo::plugins::{insert_connection, set_grant};
        let add = |name: &str| {
            let root = dir.join(name);
            std::fs::create_dir_all(&root).expect("mkdir");
            root.to_string_lossy().into_owned()
        };
        let repo = crate::services::workspaces::add(store, &add("shop"), None)
            .await
            .expect("repo")
            .workspace
            .id;
        let other = crate::services::workspaces::add(store, &add("blog"), None)
            .await
            .expect("other")
            .workspace
            .id;
        let lane = tandem_store::repo::workspaces::add_lane(
            store,
            tandem_store::repo::workspaces::NewLane {
                parent_id: repo.clone(),
                root: add("shop-wt"),
                name: "wt".into(),
                branch: "tandem/wt".into(),
                base: "main".into(),
            },
        )
        .await
        .expect("lane")
        .id;
        let mut ids = HashMap::new();
        for plugin in ["test_echo", "test_upper", "test_down", "test_all"] {
            let c = insert_connection(store, plugin, Some("me"), "none")
                .await
                .expect("connection");
            ids.insert(plugin, c.id);
        }
        let ws = |id: &WorkspaceId| GrantSubject::Workspace {
            workspace_id: id.as_str().to_string(),
        };
        for (subject, plugin) in [
            (ws(&repo), "test_echo"),
            (ws(&lane), "test_upper"),
            (ws(&other), "test_down"),
            // Threads' "all threads" grants aren't a terminal's.
            (GrantSubject::AllThreads, "test_all"),
        ] {
            set_grant(store, &subject, &ids[plugin], true)
                .await
                .expect("grant");
        }
        (ids, [repo, lane, other])
    }

    #[tokio::test]
    async fn f_pl_05_3_a_terminal_uses_its_workspaces_grants_and_a_worktree_its_folders_too() {
        let (dir, store) = store().await;
        let (ids, [repo, lane, other]) = granted_folders(&store, dir.path()).await;
        let names = |v: Vec<ConnectionId>| {
            v.iter()
                .filter_map(|c| ids.iter().find(|(_, id)| *id == c).map(|(p, _)| *p))
                .collect::<Vec<_>>()
        };
        assert_eq!(names(grants(&store, &repo).await), vec!["test_echo"]);
        // Its own first, then its folder's.
        assert_eq!(
            names(grants(&store, &lane).await),
            vec!["test_upper", "test_echo"]
        );
        assert_eq!(names(grants(&store, &other).await), vec!["test_down"]);
        let gone = WorkspaceId::from_db("ws_gone".into());
        assert!(grants(&store, &gone).await.is_empty());
        assert_eq!(
            workspace_subjects(&store, &lane).await,
            vec![
                GrantSubject::Workspace {
                    workspace_id: lane.as_str().to_string()
                },
                GrantSubject::Workspace {
                    workspace_id: repo.as_str().to_string()
                },
            ]
        );
    }

    #[test]
    fn f_pl_01_6_a_run_befores_pane_configs_go_at_launch_and_nothing_else_does() {
        let dir = tempfile::tempdir().expect("dir");
        let mcp = dir.path().join("mcp");
        std::fs::create_dir_all(&mcp).expect("mkdir");
        for name in ["pn_1.json", "pn_2.json", "th_1.json", "notes.txt"] {
            std::fs::write(mcp.join(name), "{}").expect("write");
        }
        let gateway = PaneGateway {
            service: crate::services::gateway::GatewayService::new(
                Store::open(&tandem_core::DataDir::at(dir.path().join("data")).expect("data"))
                    .expect("store"),
                dir.path().join("run"),
            ),
            leases: Arc::default(),
            dir: mcp.clone(),
        };
        gateway.clear_stale();
        let mut left: Vec<String> = std::fs::read_dir(&mcp)
            .expect("dir")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, vec!["notes.txt", "th_1.json"]);
    }

    #[test]
    fn f_pl_01_5_the_gateways_arguments_go_right_after_the_program() {
        let start = PaneStart {
            launch: GatewayLaunch {
                args: vec!["--mcp-config=/d/mcp/pn_1.json".into()],
                env: Vec::new(),
            },
            file: Some(PathBuf::from("/d/mcp/pn_1.json")),
        };
        assert_eq!(
            start.argv(vec!["/bin/claude".into(), "--resume".into(), "s1".into()]),
            vec![
                "/bin/claude",
                "--mcp-config=/d/mcp/pn_1.json",
                "--resume",
                "s1"
            ]
        );
        assert!(start.argv(Vec::new()).is_empty());
    }

    #[tokio::test]
    async fn f_pl_01_6_ending_a_panes_lease_takes_its_plugin_questions_with_it() {
        use tandem_core::approvals::{
            ApprovalBroker, ApprovalKind, ApprovalRequest, ApprovalScope,
        };
        use tandem_core::source::SourceRef;
        let broker = ApprovalBroker::new(Arc::new(tandem_core::clock::SystemClock));
        let ask = |pane: &str, kind: ApprovalKind| {
            broker.request(ApprovalRequest {
                source: SourceRef::Pane {
                    pane_id: pane.into(),
                },
                kind,
                summary: "s".into(),
                detail: "{}".into(),
                options: vec![ApprovalScope::Once],
            })
        };
        let (plugin, _t1) = ask("pn_1", ApprovalKind::PluginAction);
        let (spend, _t2) = ask("pn_1", ApprovalKind::Spend);
        let (run, _t3) = ask("pn_1", ApprovalKind::Command);
        let (other, _t4) = ask("pn_2", ApprovalKind::PluginAction);
        let leases = PaneLeases::default();
        leases.bind_broker(broker.clone());
        leases.end("pn_1");
        let left: Vec<_> = broker.pending().into_iter().map(|v| v.id).collect();
        assert!(!left.contains(&plugin.id) && !left.contains(&spend.id));
        // A `vibezeo run` question isn't the gateway's, and another pane's stays.
        assert!(left.contains(&run.id) && left.contains(&other.id));
    }

    /// Posts `tools/list` with `token`: the response's status.
    async fn status_with(url: &str, token: &str) -> u16 {
        reqwest::Client::new()
            .post(url)
            .bearer_auth(token)
            .header("accept", "application/json, text/event-stream")
            .json(&serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                "protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "claude-code", "version": "2"}
            }}))
            .send()
            .await
            .expect("send")
            .status()
            .as_u16()
    }

    #[tokio::test]
    async fn f_pl_01_6_when_the_process_ends_its_lease_is_revoked_and_its_config_deleted() {
        let (dir, store) = store().await;
        let service = crate::services::gateway::GatewayService::new(store, dir.path().join("run"));
        let gateway = service.handle().await.expect("gateway").clone();
        let lease = gateway.prepare(SessionDescriptor::new(
            Caller::Pane {
                pane_id: tandem_core::ids::PaneId::from_db("pn_1".into()),
            },
            Attended::Interactive,
            Access::Edits,
            BuiltinSet::default(),
        ));
        let file = dir.path().join("mcp").join("pn_1.json");
        launch("claude", &gateway.url, &lease.token, &file).expect("written");
        let leases = PaneLeases::default();
        leases.hold(
            "pn_1",
            gateway.clone(),
            lease.id.clone(),
            Some(file.clone()),
        );
        assert!(leases.holds("pn_1"));
        assert_eq!(status_with(&gateway.url, &lease.token).await, 200);
        leases.end("pn_1");
        assert!(!leases.holds("pn_1"));
        assert_eq!(status_with(&gateway.url, &lease.token).await, 401);
        assert!(!file.exists());
        // Again, or for a pane with none: nothing.
        leases.end("pn_1");
        leases.end("pn_9");
    }

    #[tokio::test]
    async fn f_pl_01_6_a_relaunch_revokes_the_runs_lease_before_and_keeps_the_new_config() {
        let (dir, store) = store().await;
        let service = crate::services::gateway::GatewayService::new(store, dir.path().join("run"));
        let gateway = service.handle().await.expect("gateway").clone();
        let session = || {
            SessionDescriptor::new(
                Caller::Pane {
                    pane_id: tandem_core::ids::PaneId::from_db("pn_1".into()),
                },
                Attended::Interactive,
                Access::Edits,
                BuiltinSet::default(),
            )
        };
        let file = dir.path().join("pn_1.json");
        let first = gateway.prepare(session());
        launch("claude", &gateway.url, &first.token, &file).expect("written");
        let leases = PaneLeases::default();
        leases.hold(
            "pn_1",
            gateway.clone(),
            first.id.clone(),
            Some(file.clone()),
        );
        let second = gateway.prepare(session());
        launch("claude", &gateway.url, &second.token, &file).expect("written");
        leases.hold(
            "pn_1",
            gateway.clone(),
            second.id.clone(),
            Some(file.clone()),
        );
        assert_eq!(status_with(&gateway.url, &first.token).await, 401);
        assert_eq!(status_with(&gateway.url, &second.token).await, 200);
        assert!(file.exists(), "the new run's config stays");
    }
}
