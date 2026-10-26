//! `panes` rows (F-CD-02, F-CD-03). In M2 a pane is a plain terminal; agent,
//! browser and simulator panes arrive with their milestones.

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tandem_core::ids::{AccountId, PaneId, TabId, WorkspaceId};
use ts_rs::TS;

use crate::{now_ms, Store, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PaneRecord {
    pub id: PaneId,
    pub tab_id: TabId,
    pub workspace_id: WorkspaceId,
    /// `terminal` | `agent` | `browser` | `simulator`
    pub kind: String,
    pub cwd: Option<String>,
    pub engine_id: Option<String>,
    /// The engine account it launches with; `None` = the engine's default.
    pub account_id: Option<AccountId>,
    pub human_name: Option<String>,
    pub title: Option<String>,
    /// `none` | `live` | `paused` (an agent waiting for Resume after a
    /// restart) | `cancelled` (its restore was cancelled; F-SH-07).
    pub restore_state: String,
    pub created_at: i64,
    /// A browser pane's page: the URL it last committed (F-CD-17).
    pub browser_url: Option<String>,
    /// "{model} · {kind}" for a Codex pane started on a model server on
    /// this computer (F-EN-11): its header says "On this computer".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub local_model: Option<String>,
}

const COLUMNS: &str =
    "id, tab_id, workspace_id, kind, cwd, engine_id, human_name, title, created_at, account_id, restore_state, browser_url, local_model";

fn from_row(r: &Row<'_>) -> rusqlite::Result<PaneRecord> {
    Ok(PaneRecord {
        id: PaneId::from_db(r.get(0)?),
        tab_id: TabId::from_db(r.get(1)?),
        workspace_id: WorkspaceId::from_db(r.get(2)?),
        kind: r.get(3)?,
        cwd: r.get(4)?,
        engine_id: r.get(5)?,
        human_name: r.get(6)?,
        title: r.get(7)?,
        created_at: r.get(8)?,
        account_id: r.get::<_, Option<String>>(9)?.map(AccountId::from_db),
        restore_state: r.get(10)?,
        browser_url: r.get(11)?,
        local_model: r.get(12)?,
    })
}

pub async fn create(
    store: &Store,
    tab: &TabId,
    kind: &str,
    cwd: Option<String>,
) -> Result<PaneRecord, StoreError> {
    create_with(store, tab, kind, cwd, None).await
}

/// A pane that runs `engine_id` (agent panes).
pub async fn create_with(
    store: &Store,
    tab: &TabId,
    kind: &str,
    cwd: Option<String>,
    engine_id: Option<String>,
) -> Result<PaneRecord, StoreError> {
    if !matches!(kind, "terminal" | "agent" | "browser" | "simulator") {
        return Err(StoreError::Invalid("Unknown pane kind".into()));
    }
    let tab = tab.as_str().to_string();
    let kind = kind.to_string();
    store
        .write(move |t| {
            let ws: Option<String> =
                t.query_row("SELECT workspace_id FROM tabs WHERE id = ?1", [&tab], |r| r.get(0)).optional()?;
            let ws = ws.ok_or_else(|| StoreError::NotFound("No tab with that id".into()))?;
            let pane = PaneRecord {
                id: PaneId::new(),
                tab_id: TabId::from_db(tab.clone()),
                workspace_id: WorkspaceId::from_db(ws),
                kind,
                cwd,
                engine_id,
                account_id: None,
                human_name: None,
                title: None,
                restore_state: "none".into(),
                created_at: now_ms(),
                browser_url: None,
                local_model: None,
            };
            t.execute(
                "INSERT INTO panes (id, tab_id, workspace_id, kind, cwd, engine_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![pane.id.as_str(), tab, pane.workspace_id.as_str(), pane.kind, pane.cwd, pane.engine_id, pane.created_at],
            )?;
            Ok(pane)
        })
        .await
}

/// Gives an agent pane a first name no other open pane of its workspace
/// has (F-CD-12 AC1). `pick(n)` chooses among the `n` free names (random in
/// the app, fixed in tests); when all are taken, names get a number.
pub async fn assign_human_name(
    store: &Store,
    pane: &PaneId,
    names: &[String],
    pick: impl FnOnce(usize) -> usize + Send + 'static,
) -> Result<String, StoreError> {
    if names.is_empty() {
        return Err(StoreError::Invalid("No names to choose from".into()));
    }
    let pane = pane.as_str().to_string();
    let names = names.to_vec();
    store
        .write(move |t| {
            let ws: String = t
                .query_row(
                    "SELECT workspace_id FROM panes WHERE id = ?1 AND closed_at IS NULL",
                    [&pane],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or_else(|| StoreError::NotFound("No open pane with that id".into()))?;
            let taken = taken_names(t, &ws, &pane)?;
            let free: Vec<&String> = names
                .iter()
                .filter(|n| !taken.contains(n.as_str()))
                .collect();
            let name = if free.is_empty() {
                let base = &names[pick(names.len()) % names.len()];
                (2..)
                    .map(|n| format!("{base} {n}"))
                    .find(|c| !taken.contains(c.as_str()))
                    .unwrap_or_else(|| base.clone())
            } else {
                free[pick(free.len()) % free.len()].clone()
            };
            t.execute(
                "UPDATE panes SET human_name = ?2 WHERE id = ?1",
                params![pane, name],
            )?;
            Ok(name)
        })
        .await
}

/// Renames a pane; a name another open pane of the workspace uses is a
/// Conflict (F-CD-12).
pub async fn rename(store: &Store, pane: &PaneId, name: &str) -> Result<String, StoreError> {
    let name = name.trim().to_string();
    if name.is_empty() || name.chars().count() > 32 || name.chars().any(char::is_control) {
        return Err(StoreError::Invalid("Names are 1–32 characters".into()));
    }
    let pane = pane.as_str().to_string();
    store
        .write(move |t| {
            let ws: String = t
                .query_row(
                    "SELECT workspace_id FROM panes WHERE id = ?1 AND closed_at IS NULL",
                    [&pane],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or_else(|| StoreError::NotFound("No open pane with that id".into()))?;
            if taken_names(t, &ws, &pane)?.contains(name.as_str()) {
                return Err(StoreError::Conflict(format!(
                    "Another pane is already called {name}"
                )));
            }
            t.execute(
                "UPDATE panes SET human_name = ?2 WHERE id = ?1",
                params![pane, name],
            )?;
            Ok(name)
        })
        .await
}

fn taken_names(
    t: &rusqlite::Transaction<'_>,
    workspace: &str,
    except: &str,
) -> Result<std::collections::HashSet<String>, StoreError> {
    let mut stmt = t.prepare(
        "SELECT human_name FROM panes WHERE workspace_id = ?1 AND id <> ?2 AND closed_at IS NULL AND human_name IS NOT NULL",
    )?;
    let rows = stmt.query_map(params![workspace, except], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Pins an agent pane to one engine account (F-AC-01).
pub async fn set_account(
    store: &Store,
    pane: &PaneId,
    account: &AccountId,
) -> Result<(), StoreError> {
    let (pane, account) = (pane.as_str().to_string(), account.as_str().to_string());
    store
        .write(move |t| {
            t.execute(
                "UPDATE panes SET account_id = ?2 WHERE id = ?1",
                params![pane, account],
            )?;
            Ok(())
        })
        .await
}

pub async fn get(store: &Store, id: &PaneId) -> Result<PaneRecord, StoreError> {
    let id = id.as_str().to_string();
    store
        .read(move |c| {
            c.query_row(
                &format!("SELECT {COLUMNS} FROM panes WHERE id = ?1"),
                [&id],
                from_row,
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound("No pane with that id".into()))
        })
        .await
}

/// A pane that's still open (closed panes never get a process).
pub async fn get_open(store: &Store, id: &PaneId) -> Result<PaneRecord, StoreError> {
    let id = id.as_str().to_string();
    store
        .read(move |c| {
            c.query_row(
                &format!("SELECT {COLUMNS} FROM panes WHERE id = ?1 AND closed_at IS NULL"),
                [&id],
                from_row,
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound("This pane was closed".into()))
        })
        .await
}

/// Open panes in a tab, oldest first.
pub async fn list_open(store: &Store, tab: &TabId) -> Result<Vec<PaneRecord>, StoreError> {
    let tab = tab.as_str().to_string();
    store
        .read(move |c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM panes WHERE tab_id = ?1 AND closed_at IS NULL ORDER BY created_at, id"
            ))?;
            let rows = stmt.query_map([tab], from_row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

/// Every open pane in a workspace that isn't removed, oldest first (Find
/// work, F-CN-05).
pub async fn list_all_open(store: &Store) -> Result<Vec<PaneRecord>, StoreError> {
    store
        .read(move |c| {
            let columns = COLUMNS
                .split(", ")
                .map(|col| format!("p.{col}"))
                .collect::<Vec<_>>()
                .join(", ");
            let mut stmt = c.prepare(&format!(
                "SELECT {columns} FROM panes p JOIN workspaces w ON w.id = p.workspace_id
                 WHERE p.closed_at IS NULL AND w.removed_at IS NULL ORDER BY p.created_at, p.rowid"
            ))?;
            let rows = stmt
                .query_map([], from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

pub async fn close(store: &Store, id: &PaneId) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE panes SET closed_at = ?2 WHERE id = ?1 AND closed_at IS NULL",
                params![id, now_ms()],
            )?;
            Ok(())
        })
        .await
}

/// The shell's working directory, from its OSC 7 mark (F-CD-10): restores
/// and link resolution start there.
pub async fn set_cwd(store: &Store, id: &PaneId, cwd: String) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE panes SET cwd = ?2 WHERE id = ?1 AND closed_at IS NULL AND (cwd IS NULL OR cwd <> ?2)",
                params![id, cwd],
            )?;
            Ok(())
        })
        .await
}

/// Marks an agent pane as started on a local model and keeps how every
/// start of it launches: its own home and arguments, never an account
/// (F-EN-11, D-2519).
pub async fn set_local_model(
    store: &Store,
    id: &PaneId,
    label: &str,
    config: &crate::repo::sessions::LaunchConfig,
) -> Result<(), StoreError> {
    let (id, label) = (id.as_str().to_string(), label.to_string());
    let json = serde_json::to_string(config).map_err(|e| StoreError::Invalid(e.to_string()))?;
    store
        .write(move |t| {
            t.execute(
                "UPDATE panes SET local_model = ?2, launch_config = ?3 \
                 WHERE id = ?1 AND kind = 'agent' AND closed_at IS NULL",
                params![id, label, json],
            )?;
            Ok(())
        })
        .await
}

/// A local-model pane's saved launch (F-EN-11): `None` when none was saved,
/// `Some(Err)` when it can't be read.
pub async fn launch_config(
    store: &Store,
    id: &PaneId,
) -> Result<Option<Result<crate::repo::sessions::LaunchConfig, String>>, StoreError> {
    let id = id.as_str().to_string();
    store
        .read(move |c| {
            let raw: Option<String> = c
                .query_row(
                    "SELECT launch_config FROM panes WHERE id = ?1",
                    [&id],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            Ok(raw.map(|j| serde_json::from_str(&j).map_err(|e| e.to_string())))
        })
        .await
}

/// A browser pane's page (F-CD-17): what a relaunch opens again.
pub async fn set_browser_url(
    store: &Store,
    id: &PaneId,
    url: Option<String>,
) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE panes SET browser_url = ?2 WHERE id = ?1 AND kind = 'browser' AND closed_at IS NULL \
                 AND browser_url IS NOT ?2",
                params![id, url],
            )?;
            Ok(())
        })
        .await
}

/// Moves an open pane into another tab (drag to the tab strip, "Move to new
/// tab"). The process keeps running; only the pane's place changes.
pub async fn move_to_tab(
    store: &Store,
    id: &PaneId,
    tab: &TabId,
) -> Result<PaneRecord, StoreError> {
    let id = id.as_str().to_string();
    let tab = tab.as_str().to_string();
    store
        .write(move |t| {
            let ws: Option<String> =
                t.query_row("SELECT workspace_id FROM tabs WHERE id = ?1", [&tab], |r| r.get(0)).optional()?;
            let ws = ws.ok_or_else(|| StoreError::NotFound("No tab with that id".into()))?;
            let moved = t.execute(
                "UPDATE panes SET tab_id = ?2, workspace_id = ?3 WHERE id = ?1 AND closed_at IS NULL",
                params![id, tab, ws],
            )?;
            if moved == 0 {
                return Err(StoreError::NotFound("No open pane with that id".into()));
            }
            Ok(t.query_row(&format!("SELECT {COLUMNS} FROM panes WHERE id = ?1"), [&id], from_row)?)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{tabs, workspaces};

    #[tokio::test]
    async fn panes_belong_to_a_tab_and_close() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = tandem_core::DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let (ws, _) = workspaces::add(
            &store,
            workspaces::NewWorkspace {
                kind: "local".into(),
                host_id: "local".into(),
                root: "/p".into(),
                name: "p".into(),
                wsl_distro: None,
            },
            None,
        )
        .await
        .expect("ws");
        let tab = tabs::create(&store, &ws.id, None).await.expect("tab");
        let pane = create(&store, &tab.id, "terminal", Some("/p".into()))
            .await
            .expect("pane");
        assert_eq!(pane.workspace_id, ws.id);
        assert_eq!(list_open(&store, &tab.id).await.expect("list").len(), 1);
        assert_eq!(
            get(&store, &pane.id).await.expect("get").cwd.as_deref(),
            Some("/p")
        );
        assert!(get_open(&store, &pane.id).await.is_ok());
        set_cwd(&store, &pane.id, "/tmp".into()).await.expect("cwd");
        assert_eq!(
            get(&store, &pane.id).await.expect("get").cwd.as_deref(),
            Some("/tmp")
        );
        close(&store, &pane.id).await.expect("close");
        assert!(list_open(&store, &tab.id).await.expect("list").is_empty());
        assert!(matches!(
            get_open(&store, &pane.id).await,
            Err(StoreError::NotFound(_))
        ));
        assert!(matches!(
            create(&store, &tab.id, "toaster", None).await,
            Err(StoreError::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn f_cn_05_every_open_pane_of_every_workspace_is_listed_but_not_removed_ones() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = tandem_core::DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let mut tabs_by_ws = Vec::new();
        for root in ["/a", "/b"] {
            let (ws, _) = workspaces::add(
                &store,
                workspaces::NewWorkspace {
                    kind: "local".into(),
                    host_id: "local".into(),
                    root: root.into(),
                    name: root.trim_start_matches('/').into(),
                    wsl_distro: None,
                },
                None,
            )
            .await
            .expect("ws");
            let tab = tabs::create(&store, &ws.id, None).await.expect("tab");
            tabs_by_ws.push((ws, tab));
        }
        let a = create(&store, &tabs_by_ws[0].1.id, "terminal", None)
            .await
            .expect("a");
        let closed = create(&store, &tabs_by_ws[0].1.id, "terminal", None)
            .await
            .expect("closed");
        close(&store, &closed.id).await.expect("close");
        let b = create(&store, &tabs_by_ws[1].1.id, "terminal", None)
            .await
            .expect("b");
        let ids = |rows: Vec<PaneRecord>| rows.into_iter().map(|p| p.id).collect::<Vec<_>>();
        assert_eq!(
            ids(list_all_open(&store).await.expect("all")),
            [a.id.clone(), b.id]
        );
        workspaces::remove(&store, &tabs_by_ws[1].0.id)
            .await
            .expect("remove");
        assert_eq!(ids(list_all_open(&store).await.expect("all")), [a.id]);
    }

    /// F-EN-11: an agent pane keeps the local model it was started on;
    /// other panes never get one.
    #[tokio::test]
    async fn f_en_11_2_an_agent_pane_keeps_its_local_model() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = tandem_core::DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let (ws, _) = workspaces::add(
            &store,
            workspaces::NewWorkspace {
                kind: "local".into(),
                host_id: "local".into(),
                root: "/p".into(),
                name: "p".into(),
                wsl_distro: None,
            },
            None,
        )
        .await
        .expect("ws");
        let tab = tabs::create(&store, &ws.id, None).await.expect("tab");
        let agent = create_with(&store, &tab.id, "agent", None, Some("codex".into()))
            .await
            .expect("agent");
        let shell = create(&store, &tab.id, "terminal", None)
            .await
            .expect("terminal");
        assert_eq!(agent.local_model, None);
        let config = crate::repo::sessions::LaunchConfig {
            extra_args: vec!["--sandbox".into(), "read-only".into()],
            home: Some("/data/providers/codex/local-ollama-11434-qwen3".into()),
            own_home: false,
        };
        assert_eq!(launch_config(&store, &agent.id).await.expect("read"), None);
        set_local_model(&store, &agent.id, "qwen3-coder:30b · Ollama", &config)
            .await
            .expect("set");
        set_local_model(&store, &shell.id, "x", &config)
            .await
            .expect("ignored");
        // Kept in the database, so every later start (and a new run of the
        // app) launches the same way.
        assert_eq!(
            launch_config(&store, &agent.id).await.expect("read"),
            Some(Ok(config))
        );
        assert_eq!(launch_config(&store, &shell.id).await.expect("read"), None);
        let listed = list_open(&store, &tab.id).await.expect("list");
        let model = |id: &PaneId| {
            listed
                .iter()
                .find(|p| &p.id == id)
                .and_then(|p| p.local_model.clone())
        };
        assert_eq!(
            model(&agent.id).as_deref(),
            Some("qwen3-coder:30b · Ollama")
        );
        assert_eq!(model(&shell.id), None);
        // Not serialized when unset, so older payloads stay the same.
        let json = serde_json::to_value(&shell).expect("json");
        assert!(json.get("localModel").is_none());
    }

    #[tokio::test]
    async fn f_cd_17_a_browser_pane_keeps_the_page_it_was_on() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = tandem_core::DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let (ws, _) = workspaces::add(
            &store,
            workspaces::NewWorkspace {
                kind: "local".into(),
                host_id: "local".into(),
                root: "/p".into(),
                name: "p".into(),
                wsl_distro: None,
            },
            None,
        )
        .await
        .expect("ws");
        let tab = tabs::create(&store, &ws.id, None).await.expect("tab");
        let page = create(&store, &tab.id, "browser", None)
            .await
            .expect("browser");
        let shell = create(&store, &tab.id, "terminal", None)
            .await
            .expect("terminal");
        assert_eq!(page.browser_url, None);
        set_browser_url(&store, &page.id, Some("http://localhost:5173/".into()))
            .await
            .expect("url");
        // Only browser panes have a page.
        set_browser_url(&store, &shell.id, Some("http://x/".into()))
            .await
            .expect("ignored");
        let listed = list_open(&store, &tab.id).await.expect("list");
        let url = |id: &PaneId| {
            listed
                .iter()
                .find(|p| &p.id == id)
                .and_then(|p| p.browser_url.clone())
        };
        assert_eq!(url(&page.id).as_deref(), Some("http://localhost:5173/"));
        assert_eq!(url(&shell.id), None);
        // A closed pane keeps the last page it had.
        close(&store, &page.id).await.expect("close");
        set_browser_url(&store, &page.id, Some("http://later/".into()))
            .await
            .expect("no-op");
        assert_eq!(
            get(&store, &page.id)
                .await
                .expect("get")
                .browser_url
                .as_deref(),
            Some("http://localhost:5173/")
        );
    }

    #[tokio::test]
    async fn f_cd_02_move_to_new_tab_keeps_the_pane() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = tandem_core::DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let (ws, _) = workspaces::add(
            &store,
            workspaces::NewWorkspace {
                kind: "local".into(),
                host_id: "local".into(),
                root: "/p".into(),
                name: "p".into(),
                wsl_distro: None,
            },
            None,
        )
        .await
        .expect("ws");
        let from = tabs::create(&store, &ws.id, None).await.expect("tab");
        let to = tabs::create(&store, &ws.id, None).await.expect("tab");
        let pane = create(&store, &from.id, "terminal", None)
            .await
            .expect("pane");
        let moved = move_to_tab(&store, &pane.id, &to.id).await.expect("move");
        assert_eq!(moved.id, pane.id);
        assert_eq!(moved.tab_id, to.id);
        assert!(list_open(&store, &from.id).await.expect("list").is_empty());
        assert_eq!(list_open(&store, &to.id).await.expect("list").len(), 1);
        close(&store, &pane.id).await.expect("close");
        assert!(matches!(
            move_to_tab(&store, &pane.id, &from.id).await,
            Err(StoreError::NotFound(_))
        ));
    }

    async fn workspace_tab(store: &Store, root: &str) -> TabId {
        let (ws, _) = workspaces::add(
            store,
            workspaces::NewWorkspace {
                kind: "local".into(),
                host_id: "local".into(),
                root: root.into(),
                name: root.trim_start_matches('/').into(),
                wsl_distro: None,
            },
            None,
        )
        .await
        .expect("ws");
        tabs::create(store, &ws.id, None).await.expect("tab").id
    }

    #[tokio::test]
    async fn f_cd_12_1_agent_names_are_unique_per_workspace_and_persist() {
        let (_d, store) = crate::tests::temp_store().await;
        let names: Vec<String> = ["Ada", "Bea"].map(String::from).to_vec();
        let tab = workspace_tab(&store, "/a").await;
        let other = workspace_tab(&store, "/b").await;
        let mut got = Vec::new();
        for _ in 0..3 {
            let p = create_with(&store, &tab, "agent", None, Some("claude".into()))
                .await
                .expect("pane");
            // Always "pick the first free one": the uniqueness is the store's.
            got.push(
                assign_human_name(&store, &p.id, &names, |_| 0)
                    .await
                    .expect("name"),
            );
        }
        assert_eq!(got, ["Ada", "Bea", "Ada 2"]);
        // Another workspace starts over.
        let p = create_with(&store, &other, "agent", None, Some("claude".into()))
            .await
            .expect("pane");
        assert_eq!(
            assign_human_name(&store, &p.id, &names, |_| 0)
                .await
                .expect("name"),
            "Ada"
        );
        // Stored on the pane, so a relaunch shows the same name.
        assert_eq!(
            get(&store, &p.id).await.expect("get").human_name.as_deref(),
            Some("Ada")
        );
        // A closed pane frees its name.
        let open = list_open(&store, &tab).await.expect("list");
        let first = open
            .iter()
            .find(|x| x.human_name.as_deref() == Some("Ada"))
            .expect("ada");
        close(&store, &first.id).await.expect("close");
        let p = create_with(&store, &tab, "agent", None, Some("claude".into()))
            .await
            .expect("pane");
        assert_eq!(
            assign_human_name(&store, &p.id, &names, |_| 0)
                .await
                .expect("name"),
            "Ada"
        );
    }

    #[tokio::test]
    async fn f_cd_12_renaming_to_a_name_in_use_is_a_conflict() {
        let (_d, store) = crate::tests::temp_store().await;
        let names: Vec<String> = ["Ada", "Bea"].map(String::from).to_vec();
        let tab = workspace_tab(&store, "/a").await;
        let a = create_with(&store, &tab, "agent", None, Some("claude".into()))
            .await
            .expect("a");
        let b = create_with(&store, &tab, "agent", None, Some("claude".into()))
            .await
            .expect("b");
        assign_human_name(&store, &a.id, &names, |_| 0)
            .await
            .expect("a");
        assign_human_name(&store, &b.id, &names, |_| 0)
            .await
            .expect("b");
        assert!(matches!(
            rename(&store, &b.id, " Ada ").await,
            Err(StoreError::Conflict(_))
        ));
        assert!(matches!(
            rename(&store, &b.id, "").await,
            Err(StoreError::Invalid(_))
        ));
        assert_eq!(
            rename(&store, &b.id, " Rhea ").await.expect("rename"),
            "Rhea"
        );
        assert_eq!(
            rename(&store, &a.id, "Ada").await.expect("same name"),
            "Ada"
        );
    }
}
