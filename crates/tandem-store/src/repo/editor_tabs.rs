//! `editor_tabs` (F-CD-19): the Files tab's state per context (a Code
//! workspace or a thread's folder): open files, the active one, expanded
//! folders, "Show ignored", the divider and the Files/Changes view. The UI
//! owns it; the store keeps it bounded, so it reopens after a relaunch
//! (AC4).

use std::collections::HashSet;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{now_ms, Store, StoreError};

const MAX_TABS: usize = 50;
const MAX_EXPANDED: usize = 500;
const MAX_PATH: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EditorTab {
    /// Relative to the context's folder, or absolute outside it.
    pub path: String,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub col: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FilesView {
    Files,
    Changes,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EditorTabs {
    pub tabs: Vec<EditorTab>,
    pub active: Option<String>,
    /// Expanded folders, relative to the context's folder.
    pub expanded: Vec<String>,
    #[serde(default)]
    pub show_ignored: bool,
    /// The tree's width in the wide layout, in px.
    #[serde(default)]
    pub divider: Option<u32>,
    #[serde(default)]
    pub view: Option<FilesView>,
}

/// `ws:<workspaceId>` or `th:<threadId>` (`tm:` joins with teammates).
pub fn valid_key(key: &str) -> bool {
    let id = key
        .strip_prefix("ws:")
        .or_else(|| key.strip_prefix("th:"))
        .unwrap_or("");
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

impl EditorTabs {
    /// Drops duplicates and empty paths, keeps the newest within the caps
    /// and repairs a stale active tab.
    pub fn normalized(mut self) -> Self {
        let mut seen = HashSet::new();
        self.tabs.retain(|t| {
            !t.path.is_empty() && t.path.len() <= MAX_PATH && seen.insert(t.path.clone())
        });
        if self.tabs.len() > MAX_TABS {
            let extra = self.tabs.len() - MAX_TABS;
            self.tabs.drain(..extra);
        }
        let mut seen = HashSet::new();
        self.expanded
            .retain(|p| !p.is_empty() && p.len() <= MAX_PATH && seen.insert(p.clone()));
        self.expanded.truncate(MAX_EXPANDED);
        if !self
            .active
            .as_ref()
            .is_some_and(|a| self.tabs.iter().any(|t| &t.path == a))
        {
            self.active = self.tabs.last().map(|t| t.path.clone());
        }
        self
    }
}

pub async fn get(store: &Store, key: &str) -> Result<EditorTabs, StoreError> {
    if !valid_key(key) {
        return Err(StoreError::Invalid("Unknown files context".into()));
    }
    let key = key.to_string();
    let json: Option<String> = store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT state_json FROM editor_tabs WHERE context_key = ?1",
                [&key],
                |r| r.get(0),
            )
            .optional()?)
        })
        .await?;
    Ok(json
        .and_then(|j| serde_json::from_str::<EditorTabs>(&j).ok())
        .map(EditorTabs::normalized)
        .unwrap_or_default())
}

pub async fn save(store: &Store, key: &str, state: EditorTabs) -> Result<(), StoreError> {
    if !valid_key(key) {
        return Err(StoreError::Invalid("Unknown files context".into()));
    }
    let json = serde_json::to_string(&state.normalized())
        .map_err(|e| StoreError::Invalid(e.to_string()))?;
    let key = key.to_string();
    store
        .write(move |t| {
            t.execute(
                "INSERT INTO editor_tabs (context_key, state_json, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(context_key) DO UPDATE SET state_json = excluded.state_json, updated_at = excluded.updated_at",
                params![key, json, now_ms()],
            )?;
            Ok(())
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(path: &str) -> EditorTab {
        EditorTab {
            path: path.into(),
            line: None,
            col: None,
        }
    }

    #[tokio::test]
    async fn f_cd_19_4_open_tabs_round_trip_per_workspace() {
        let (_d, store) = crate::tests::temp_store().await;
        assert_eq!(
            get(&store, "ws:ws_1").await.expect("get"),
            EditorTabs::default()
        );
        let state = EditorTabs {
            tabs: vec![
                tab("src/main.rs"),
                EditorTab {
                    path: "/etc/hosts".into(),
                    line: Some(3),
                    col: Some(1),
                },
            ],
            active: Some("src/main.rs".into()),
            expanded: vec!["src".into(), "src/deep".into()],
            show_ignored: true,
            divider: Some(220),
            view: Some(FilesView::Changes),
        };
        save(&store, "ws:ws_1", state.clone()).await.expect("save");
        assert_eq!(get(&store, "ws:ws_1").await.expect("get"), state);
        // Another context keeps its own.
        save(&store, "th:th_9", EditorTabs::default())
            .await
            .expect("save");
        assert_eq!(get(&store, "ws:ws_1").await.expect("get"), state);
        assert_eq!(
            get(&store, "th:th_9").await.expect("get").tabs,
            Vec::<EditorTab>::new()
        );
    }

    #[tokio::test]
    async fn f_cd_19_tab_state_is_kept_sane() {
        let (_d, store) = crate::tests::temp_store().await;
        assert!(save(&store, "nope", EditorTabs::default()).await.is_err());
        assert!(get(&store, "ws:../x").await.is_err());
        let mut tabs: Vec<EditorTab> = (0..60).map(|i| tab(&format!("f{i}.rs"))).collect();
        tabs.push(tab("f59.rs"));
        tabs.push(tab(""));
        let state = EditorTabs {
            tabs,
            active: Some("gone.rs".into()),
            expanded: vec!["a".into(), "a".into(), String::new()],
            ..EditorTabs::default()
        };
        save(&store, "ws:ws_2", state).await.expect("save");
        let back = get(&store, "ws:ws_2").await.expect("get");
        assert_eq!(back.tabs.len(), 50);
        assert_eq!(back.tabs[0].path, "f10.rs", "the newest are kept");
        assert_eq!(back.active.as_deref(), Some("f59.rs"));
        assert_eq!(back.expanded, vec!["a".to_string()]);
    }
}
