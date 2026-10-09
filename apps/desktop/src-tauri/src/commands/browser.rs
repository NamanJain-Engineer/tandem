//! The browser (ARCHITECTURE §3.2 "Files, editor, browser, scratchpad";
//! F-CD-17). Opening and navigating read the workspaces (for `file:` pages)
//! and the search engine setting; the rest are main-thread commands, so a
//! page's calls reach it in the order the UI made them.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tandem_core::ids::{has_prefix, PaneId};
use tandem_core::settings::SettingKey;
use tandem_core::{ErrorCode, IpcError};
use tandem_store::repo::{panes, settings, workspaces};
use tandem_store::Store;
use tauri::State;

use crate::browser::{BrowserCmd, BrowserService, BrowserState, Device, Navigated, Rect};
use crate::state::AppState;

fn service(app: &AppState) -> Result<Arc<BrowserService>, IpcError> {
    app.browser.get().cloned().ok_or_else(|| {
        IpcError::new(
            ErrorCode::Unavailable,
            crate::browser::wry_host::UNAVAILABLE,
        )
        .with_stage("browser")
    })
}

/// The workspaces' folders, for the `file:` rule.
async fn refresh_roots(app: &AppState, browser: &BrowserService) {
    if let Ok(list) = workspaces::list(&app.store).await {
        browser.set_roots(
            list.into_iter()
                .filter(|w| w.kind == "local")
                .map(|w| PathBuf::from(w.root))
                .collect(),
        );
    }
}

/// The search engine setting (DuckDuckGo by default).
async fn search_engine(app: &AppState) -> String {
    settings::get(&app.store, SettingKey::SearchEngine)
        .await
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "duckduckgo".into())
}

/// What an address-bar `input` opens (a browser pane's or seat's first
/// page); blocked schemes are refused.
pub async fn resolve(app: &AppState, input: &str) -> Result<String, IpcError> {
    let roots = workspaces::list(&app.store)
        .await
        .map(|list| {
            list.into_iter()
                .filter(|w| w.kind == "local")
                .map(|w| PathBuf::from(w.root))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    crate::browser::resolve_input(input, &search_engine(app).await, &roots)
}

#[tauri::command]
pub async fn browser_open(
    app: State<'_, AppState>,
    browser_id: String,
    url: Option<String>,
    bounds: Rect,
    visible: bool,
) -> Result<BrowserState, IpcError> {
    let browser = service(&app)?;
    refresh_roots(&app, &browser).await;
    browser.open(&browser_id, url.as_deref(), bounds, visible)
}

#[tauri::command]
pub async fn browser_navigate(
    app: State<'_, AppState>,
    browser_id: String,
    input: String,
) -> Result<Navigated, IpcError> {
    let browser = service(&app)?;
    refresh_roots(&app, &browser).await;
    let engine = search_engine(&app).await;
    browser.navigate(&browser_id, &input, &engine)
}

#[tauri::command]
pub fn browser_command(
    app: State<'_, AppState>,
    browser_id: String,
    cmd: BrowserCmd,
) -> Result<(), IpcError> {
    service(&app)?.command(&browser_id, cmd)
}

#[tauri::command]
pub fn browser_set_bounds(
    app: State<'_, AppState>,
    browser_id: String,
    bounds: Rect,
    visible: bool,
) -> Result<(), IpcError> {
    service(&app)?.set_bounds(&browser_id, bounds, visible)
}

#[tauri::command]
pub fn browser_set_device(
    app: State<'_, AppState>,
    browser_id: String,
    device: Device,
    rotated: bool,
) -> Result<BrowserState, IpcError> {
    service(&app)?.set_device(&browser_id, device, rotated)
}

#[tauri::command]
pub fn browser_move(
    app: State<'_, AppState>,
    from: String,
    to: String,
) -> Result<BrowserState, IpcError> {
    service(&app)?.rename(&from, &to)
}

#[tauri::command]
pub fn browser_close(app: State<'_, AppState>, browser_id: String) -> Result<(), IpcError> {
    service(&app)?.close(&browser_id);
    Ok(())
}

#[tauri::command]
pub fn browser_clear_data(app: State<'_, AppState>) -> Result<(), IpcError> {
    service(&app)?.clear_data();
    Ok(())
}

/// Keeps each browser pane's committed URL in its record, so a relaunch
/// opens the same page (sidebar tabs keep theirs with the sidebar state).
pub fn keep_pane_pages(store: Store) -> impl Fn(&BrowserState) + Send + Sync {
    let kept: Mutex<HashMap<String, String>> = Mutex::default();
    move |state: &BrowserState| {
        if state.url.is_empty() || !has_prefix(&state.browser_id, "pn") {
            return;
        }
        {
            let mut kept = kept.lock().unwrap_or_else(|e| e.into_inner());
            if kept.get(&state.browser_id) == Some(&state.url) {
                return;
            }
            kept.insert(state.browser_id.clone(), state.url.clone());
        }
        let (store, pane, url) = (
            store.clone(),
            PaneId::from_db(state.browser_id.clone()),
            state.url.clone(),
        );
        tauri::async_runtime::spawn(async move {
            if let Err(err) = panes::set_browser_url(&store, &pane, Some(url)).await {
                tracing::warn!(code = "browser.keep_url", error = %err, "browser pane page not kept");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::testing::{service, Call};
    use crate::browser::Rect;
    use crate::commands::terminal::{close_pane, create_browser_pane};
    use crate::services::launch;
    use tandem_core::DataDir;
    use tandem_store::repo::presets::{Isolation, Seat, SeatKind};
    use tandem_store::repo::tabs;

    const PLACE: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
    };

    async fn setup() -> (
        tempfile::TempDir,
        AppState,
        tandem_core::ids::WorkspaceId,
        tandem_core::ids::TabId,
    ) {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let state = AppState::for_tests(data, store);
        let root = dir.path().join("shop");
        std::fs::create_dir_all(&root).expect("root");
        let (ws, _) = workspaces::add(
            &state.store,
            workspaces::NewWorkspace {
                kind: "local".into(),
                host_id: "local".into(),
                root: root.to_string_lossy().into_owned(),
                name: "shop".into(),
                wsl_distro: None,
            },
            None,
        )
        .await
        .expect("workspace");
        let tab = tabs::create(&state.store, &ws.id, None).await.expect("tab");
        (dir, state, ws.id, tab.id)
    }

    #[tokio::test]
    async fn f_cd_17_a_browser_pane_opens_a_bare_host_over_http_and_refuses_blocked_ones() {
        let (_dir, state, _, tab) = setup().await;
        let pane = create_browser_pane(&state, &tab, Some("localhost:5173".into()))
            .await
            .expect("pane");
        assert_eq!(pane.kind, "browser");
        assert_eq!(pane.engine_id, None);
        assert_eq!(pane.browser_url.as_deref(), Some("http://localhost:5173/"));
        let empty = create_browser_pane(&state, &tab, None)
            .await
            .expect("empty pane");
        assert_eq!(empty.browser_url, None);
        for blocked in ["javascript:alert(1)", "tandem://open"] {
            let err = create_browser_pane(&state, &tab, Some(blocked.into()))
                .await
                .expect_err(blocked);
            assert_eq!(err.code, ErrorCode::Invalid, "{blocked}");
        }
        let open = panes::list_open(&state.store, &tab).await.expect("list");
        assert_eq!(open.len(), 2);
    }

    #[tokio::test]
    async fn f_cd_17_a_browser_panes_page_closes_with_it_and_its_address_is_kept() {
        let (_dir, state, _, tab) = setup().await;
        let (browser, host, _) = service();
        assert!(state.browser.set(browser.clone()).is_ok());
        let pane = create_browser_pane(&state, &tab, None).await.expect("pane");
        browser
            .open(
                pane.id.as_str(),
                Some("http://localhost:5173/"),
                PLACE,
                true,
            )
            .expect("page");
        // A committed page is what a relaunch reopens.
        let keep = keep_pane_pages(state.store.clone());
        let mut page = browser.state(pane.id.as_str()).expect("state");
        page.url = "http://localhost:5173/docs".into();
        keep(&page);
        let mut kept = None;
        for _ in 0..100 {
            kept = panes::get(&state.store, &pane.id)
                .await
                .expect("get")
                .browser_url;
            if kept.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(kept.as_deref(), Some("http://localhost:5173/docs"));
        close_pane(&state, &pane.id).await.expect("close");
        assert_eq!(
            host.calls().last(),
            Some(&Call::Close {
                id: pane.id.as_str().into()
            })
        );
        assert!(browser.state(pane.id.as_str()).is_none());
    }

    #[tokio::test]
    async fn f_cd_14_a_browser_seat_opens_its_page_tiled_with_the_others() {
        let (_dir, state, ws, _) = setup().await;
        let seat = Seat {
            kind: SeatKind::Browser,
            engine_id: None,
            account_id: None,
            role: None,
            task: None,
            command: None,
            url: Some("localhost:3000".into()),
            count: 2,
        };
        let report = launch::launch(&state, &ws, &[seat], None, Isolation::Shared)
            .await
            .expect("launch");
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert_eq!(report.launched.len(), 2);
        for launched in &report.launched {
            assert_eq!(launched.pane.kind, "browser");
            assert_eq!(
                launched.pane.browser_url.as_deref(),
                Some("http://localhost:3000/")
            );
            assert_eq!(Some(&launched.tab_id), report.shared_tab_id.as_ref());
        }
    }
}
