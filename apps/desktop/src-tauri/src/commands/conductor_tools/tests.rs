//! Conductor's tools on the app's own services (F-CN-02's table), with the
//! real store and services under Tauri's mock runtime; the window's side
//! (UI actions, focusing a thread) is recorded.

use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};
use tauri::Manager;

use tandem_core::access::{Access, ThreadMode};
use tandem_core::approvals::{ApprovalKind, ApprovalRequest, ApprovalScope};
use tandem_core::ids::{ThreadId, TurnId};
use tandem_core::source::SourceRef;
use tandem_store::repo::threads::{self, NewThread, ThreadKind};
use tandem_store::Store;
use tandem_voice::conductor::view::UiAction;

use super::{run, ToolWindow};
use crate::state::AppState;
use tandem_core::DataDir;

#[derive(Default)]
struct Window {
    actions: Mutex<Vec<UiAction>>,
    focused: Mutex<Vec<SourceRef>>,
}

#[async_trait::async_trait]
impl ToolWindow for Window {
    fn ui_action(&self, action: UiAction) {
        self.actions.lock().push(action);
    }
    async fn focus(&self, source: SourceRef) -> Result<(), tandem_core::IpcError> {
        self.focused.lock().push(source);
        Ok(())
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    app: tauri::App<tauri::test::MockRuntime>,
    window: Arc<Window>,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("dir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let app = tauri::test::mock_app();
        app.manage(AppState::for_tests(data, store));
        Self {
            _dir: dir,
            app,
            window: Arc::new(Window::default()),
        }
    }

    fn state(&self) -> tauri::State<'_, AppState> {
        self.app.state::<AppState>()
    }

    fn store(&self) -> Store {
        self.state().store.clone()
    }

    async fn call(&self, name: &str, args: Value) -> Result<String, String> {
        run(self.state(), self.window.as_ref(), name, &args).await
    }

    /// A thread, as the rail lists it.
    async fn thread(&self, title: &str) -> ThreadId {
        let id = ThreadId::new();
        let t = threads::insert(
            &self.store(),
            NewThread {
                id: id.clone(),
                kind: ThreadKind::Thread,
                workspace_id: None,
                cwd: self._dir.path().to_string_lossy().into_owned(),
                engine_id: "claude".into(),
                account_id: None,
                model: None,
                effort: None,
                access: Access::Edits,
                mode: ThreadMode::Act,
                attended: true,
            },
        )
        .await
        .expect("thread");
        threads::update(
            &self.store(),
            &t.id,
            threads::ThreadPatch {
                title: Some(title.into()),
                ..Default::default()
            },
        )
        .await
        .expect("title");
        id
    }

    async fn get(&self, id: &ThreadId) -> threads::ThreadRecord {
        threads::get(&self.store(), id)
            .await
            .expect("get")
            .expect("the thread")
    }
}

#[tokio::test]
async fn f_cn_02_thread_tools_rename_switch_mode_archive_and_open() {
    let f = Fixture::new();
    let id = f.thread("Billing bug").await;

    // By title, any case.
    assert_eq!(
        f.call(
            "rename_thread",
            json!({"thread": "billing BUG", "title": "Invoice rounding"})
        )
        .await,
        Ok("Renamed it Invoice rounding.".into())
    );
    assert_eq!(f.get(&id).await.title, "Invoice rounding");

    // By id.
    f.call(
        "set_thread_mode",
        json!({"thread": id.as_str(), "mode": "plan"}),
    )
    .await
    .expect("mode");
    assert_eq!(f.get(&id).await.mode, ThreadMode::Plan);

    f.call("open_thread", json!({"thread": "Invoice rounding"}))
        .await
        .expect("open");
    assert_eq!(
        f.window.focused.lock().clone(),
        vec![SourceRef::Thread {
            thread_id: id.as_str().to_string()
        }]
    );

    f.call("archive_thread", json!({"thread": "Invoice rounding"}))
        .await
        .expect("archive");
    assert!(f.get(&id).await.archived);

    // An unknown thread changes nothing and says so.
    assert_eq!(
        f.call("rename_thread", json!({"thread": "Nope", "title": "x"}))
            .await,
        Err("There's no thread Nope.".into())
    );
    assert_eq!(
        f.call("stop_thread", json!({"thread": ""})).await,
        Err("Say which thread.".into())
    );
}

#[tokio::test]
async fn f_cn_02_read_thread_says_the_last_messages() {
    let f = Fixture::new();
    let id = f.thread("Docs").await;
    assert_eq!(
        f.call("read_thread", json!({"thread": "Docs"})).await,
        Ok("Docs has no messages yet.".into())
    );
    let turn = TurnId::new();
    let store = f.store();
    threads::start_turn(&store, &id, &turn, "claude", None)
        .await
        .expect("turn");
    for (seq, (kind, payload)) in [
        ("user", json!({"text": "Update the README"})),
        ("tool", json!({"name": "edit"})),
        (
            "assistant",
            json!({"markdown": "Updated the install steps."}),
        ),
        ("user", json!({"text": "And the changelog"})),
    ]
    .into_iter()
    .enumerate()
    {
        threads::append(&store, &id, &turn, seq as u64, kind, payload.to_string())
            .await
            .expect("append");
    }
    assert_eq!(
        f.call("read_thread", json!({"thread": "Docs", "last": 2}))
            .await,
        Ok("Updated the install steps.\nAnd the changelog".into())
    );
}

#[tokio::test]
async fn f_cn_03_1_send_to_thread_refuses_a_thread_waiting_on_an_approval() {
    let f = Fixture::new();
    let id = f.thread("Release").await;
    let source = SourceRef::Thread {
        thread_id: id.as_str().to_string(),
    };

    // Not waiting: the text goes to the composer, controls stripped.
    assert_eq!(
        f.call(
            "send_to_thread",
            json!({"thread": "Release", "text": "bump\u{1b}[2J the version", "send": false})
        )
        .await,
        Ok("Left it in Release's composer.".into())
    );
    let draft: Value =
        serde_json::from_str(&threads::draft(&f.store(), &id).await.expect("draft")).expect("json");
    assert_eq!(draft["text"], "bump[2J the version");

    // Waiting on an approval: nothing is sent or typed, and it says why.
    let (_view, _ticket) = f.state().approvals.broker.request(ApprovalRequest {
        source: source.clone(),
        kind: ApprovalKind::Command,
        summary: "Run `npm publish`".into(),
        detail: String::new(),
        options: vec![ApprovalScope::Once],
    });
    for send in [false, true] {
        let out = f
            .call(
                "send_to_thread",
                json!({"thread": "Release", "text": "ship it", "send": send}),
            )
            .await;
        assert_eq!(
            out,
            Err("Release is waiting for approval, so nothing was sent. Tell the user to answer it first.".into())
        );
    }
    let draft: Value =
        serde_json::from_str(&threads::draft(&f.store(), &id).await.expect("draft")).expect("json");
    assert_eq!(draft["text"], "bump[2J the version");
    assert!(threads::all_messages(&f.store(), &id)
        .await
        .expect("messages")
        .is_empty());
}

async fn pinned(f: &Fixture, id: &tandem_core::ids::TeammateId) -> bool {
    f.state()
        .teammates
        .list()
        .await
        .expect("list")
        .into_iter()
        .find(|t| &t.teammate.id == id)
        .expect("the teammate")
        .teammate
        .pinned
}

#[tokio::test]
async fn f_cn_02_teammate_tools_open_pin_and_read_memory() {
    let f = Fixture::new();
    // Straight into the store: creating one through the service checks the
    // engine is installed, which isn't what this test is about.
    let home = f._dir.path().join("teammates/scout");
    std::fs::create_dir_all(&home).expect("home");
    let scout = tandem_store::repo::teammates::insert(
        &f.store(),
        tandem_store::repo::teammates::NewTeammate {
            id: tandem_core::ids::TeammateId::new(),
            name: "Scout".into(),
            avatar_hue: None,
            brief: "Keeps the changelog.".into(),
            engine_id: "claude".into(),
            model: None,
            effort: None,
            preferred_account_id: None,
            home: home.to_string_lossy().into_owned(),
            places: Vec::new(),
            settings: Default::default(),
            is_lead: false,
            pinned: false,
        },
    )
    .await
    .expect("teammate");

    f.call("open_teammate", json!({"teammate": "scout"}))
        .await
        .expect("open");
    let opened = f.window.actions.lock().last().cloned().expect("an action");
    assert_eq!(opened.action, "open_teammate");
    assert_eq!(opened.args["teammate"], scout.id.as_str());

    assert_eq!(
        f.call("pin_teammate", json!({"teammate": "Scout"})).await,
        Ok("Pinned.".into())
    );
    assert!(pinned(&f, &scout.id).await);
    f.call(
        "pin_teammate",
        json!({"teammate": "Scout", "pinned": false}),
    )
    .await
    .expect("unpin");
    assert!(!pinned(&f, &scout.id).await);

    assert_eq!(
        f.call("read_teammate_memory", json!({"teammate": "Scout"}))
            .await,
        Ok("Its memory is empty.".into())
    );
    let notes = std::path::PathBuf::from(&scout.home).join("memory/NOTES.md");
    std::fs::create_dir_all(notes.parent().expect("dir")).expect("mkdir");
    std::fs::write(&notes, "- uses pnpm\n- deploys on Fridays\n").expect("notes");
    assert_eq!(
        f.call("read_teammate_memory", json!({"teammate": "Scout"}))
            .await,
        Ok("- uses pnpm\n- deploys on Fridays".into())
    );

    assert_eq!(
        f.call("pin_teammate", json!({"teammate": "Ghost"})).await,
        Err("There's no teammate Ghost.".into())
    );
}

#[tokio::test]
async fn f_cn_02_create_teammate_only_opens_the_sheet() {
    let f = Fixture::new();
    let before = f.state().teammates.list().await.expect("list").len();
    f.call(
        "create_teammate",
        json!({"description": "Reviews pull requests every morning"}),
    )
    .await
    .expect("sheet");
    let a = f.window.actions.lock().last().cloned().expect("an action");
    assert_eq!(a.action, "new_teammate");
    assert_eq!(a.args["description"], "Reviews pull requests every morning");
    // Nothing is saved without the user's own review in the sheet.
    assert_eq!(
        f.state().teammates.list().await.expect("list").len(),
        before
    );
}

#[tokio::test]
async fn f_cn_02_task_tools_create_and_list() {
    let f = Fixture::new();
    let created = f
        .call(
            "create_task",
            json!({"title": "Fix the flaky upload test", "description": "It times out on CI"}),
        )
        .await
        .expect("create");
    assert!(created.starts_with("Created "), "{created}");
    let listed: Value =
        serde_json::from_str(&f.call("list_tasks", json!({})).await.expect("list")).expect("json");
    let items = listed.as_array().expect("tasks");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["title"], "Fix the flaky upload test");
    // Executing one needs a workspace to run it in.
    let id = created.trim_start_matches("Created ").trim_end_matches('.');
    assert_eq!(
        f.call("execute_task", json!({"task": id})).await,
        Err("There's no workspace yet: open a folder first.".into())
    );
}

#[tokio::test]
async fn f_cn_02_routine_and_handoff_tools_name_what_is_missing() {
    let f = Fixture::new();
    assert_eq!(f.call("list_routines", json!({})).await, Ok("[]".into()));
    assert_eq!(
        f.call("run_routine", json!({"routine": "Morning triage"}))
            .await,
        Err("There's no routine Morning triage.".into())
    );
    let handoff = f
        .call("handoff", json!({"pane": "pn_missing", "target": "codex"}))
        .await;
    assert!(handoff.is_err(), "{handoff:?}");
    assert!(f.window.actions.lock().is_empty());
}
