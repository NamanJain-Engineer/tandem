//! F-TL-12: the updater's `on_before_exit` hook (`before_update_exit`) ends
//! agent runs before Windows' installer exits the process. Its own process,
//! since `shutdown_all` reaches every `AgentRuns` in it.
#![cfg(unix)]

use std::sync::Arc;
use std::time::Duration;

use tandem_app::services::agent_run::{before_update_exit, AgentRuns};
use tandem_app::services::terminal::TerminalService;
use tandem_app::services::{secrets, workspaces};
use tandem_core::approvals::{ApprovalBroker, ApprovalDecision, ApprovalScope, ApprovalVia};
use tandem_core::keychain::{Keychain, MemoryKeychain};
use tandem_core::run_request::RunRequest;
use tandem_store::repo::{panes, tabs};
use tandem_store::Store;

/// Gone, or a zombie waiting for its new parent to reap it, within 2 s.
fn ends(pid: &str) -> bool {
    (0..100).any(|_| {
        std::thread::sleep(Duration::from_millis(20));
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Err(_) => true,
            Ok(stat) => stat
                .rsplit(')')
                .next()
                .and_then(|rest| rest.split_whitespace().next())
                .is_some_and(|state| state == "Z" || state == "X"),
        }
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn f_tl_12_8_the_updaters_exit_hook_ends_agent_runs_and_terminals() {
    let dir = tempfile::tempdir().expect("dir");
    let data = tandem_core::DataDir::at(dir.path().join("data")).expect("data");
    let store = Store::open(&data).expect("store");
    let root = dir.path().join("shop");
    std::fs::create_dir_all(&root).expect("mkdir");
    std::fs::write(root.join(".env"), "API_KEY=vibezeo-secret://stripe-test\n").expect(".env");
    let root = std::fs::canonicalize(&root).expect("canon");
    let ws = workspaces::add(&store, root.to_str().expect("utf8"), Some(5))
        .await
        .expect("ws")
        .workspace;
    let tab = tabs::list(&store, &ws.id).await.expect("tabs").remove(0);
    let agent = panes::create_with(&store, &tab.id, "agent", None, Some("claude".into()))
        .await
        .expect("agent")
        .id;
    let kc: Arc<dyn Keychain> = Arc::new(MemoryKeychain::new());
    secrets::set(&store, kc.as_ref(), "stripe-test", "sk_test_4242xyz", 1)
        .await
        .expect("secret");
    let broker = ApprovalBroker::new(Arc::new(tandem_core::clock::SystemClock));
    let terminals = TerminalService::new(tandem_pty::PtyManager::new(), store.clone());
    let runs = AgentRuns::new(
        store.clone(),
        kc,
        broker.clone(),
        terminals.clone(),
        data.root().to_path_buf(),
    );
    let pid_file = root.join("bg.pid");
    let req = RunRequest {
        pane_id: agent.as_str().to_string(),
        cwd: root.to_string_lossy().into_owned(),
        argv: vec![format!(
            "sleep 30 & echo $! > {}; sleep 30",
            pid_file.display()
        )],
        ts: 0,
        nonce: "0123456789abcdef".into(),
    };
    let task = tokio::spawn({
        let runs = runs.clone();
        async move { runs.handle(req).await }
    });
    let mut view = None;
    for _ in 0..300 {
        view = broker.pending().into_iter().next();
        if view.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    broker
        .resolve(
            &view.expect("asked").id,
            ApprovalDecision::Allow {
                scope: ApprovalScope::Once,
            },
            ApprovalVia::Inline,
        )
        .expect("allow");
    let mut pid = String::new();
    for _ in 0..300 {
        pid = std::fs::read_to_string(&pid_file)
            .unwrap_or_default()
            .trim()
            .to_string();
        if !pid.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!pid.is_empty(), "the run started");

    // What the updater calls before its installer exits the process.
    let snapshots = dir.path().join("scrollback");
    let hook_terminals = terminals.clone();
    tokio::task::spawn_blocking(move || before_update_exit(&hook_terminals, &snapshots))
        .await
        .expect("hook");

    assert!(ends(&pid), "the run's group outlived the update");
    assert_eq!(task.await.expect("task").exit_code, None);
    let run = terminals
        .activity
        .list("pane", agent.as_str(), None, 0)
        .await
        .expect("ledger")
        .events
        .into_iter()
        .find(|e| e.kind == "run")
        .expect("its row is written before the installer exits");
    assert_eq!(run.ended.as_deref(), Some("app_quit"));
}
