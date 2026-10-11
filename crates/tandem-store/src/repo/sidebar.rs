//! `layout_sidebar` (F-SH-10): the right sidebar's tabs, order, active tab,
//! width and full-screen, one row. The UI owns the state; the store keeps it
//! sane (unique ids, one of each singleton, width 320–900).

use std::collections::HashSet;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{now_ms, Store, StoreError};

pub const MIN_WIDTH: u32 = 320;
pub const MAX_WIDTH: u32 = 900;
pub const DEFAULT_WIDTH: u32 = 470;
const MAX_TABS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SidebarTabKind {
    Dashboard,
    Browser,
    Files,
    Tasks,
    Scratchpad,
    /// F-EX-01 (UI-140).
    Checkpoints,
    /// F-EX-03 (UI-142).
    Ports,
}

impl SidebarTabKind {
    /// Everything but browser tabs opens once (F-SH-10: "+" focuses it).
    pub fn singleton(self) -> bool {
        self != SidebarTabKind::Browser
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SidebarTab {
    pub id: String,
    pub kind: SidebarTabKind,
    /// Browser tabs: the page title and URL.
    pub title: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SidebarState {
    pub open: bool,
    pub tabs: Vec<SidebarTab>,
    pub active: Option<String>,
    pub width: u32,
    pub fullscreen: bool,
    /// Floating (narrow windows): stays open on an outside click.
    pub pinned: bool,
}

impl Default for SidebarState {
    fn default() -> Self {
        Self {
            open: false,
            tabs: vec![SidebarTab {
                id: "dashboard".into(),
                kind: SidebarTabKind::Dashboard,
                title: None,
                url: None,
            }],
            active: Some("dashboard".into()),
            width: DEFAULT_WIDTH,
            fullscreen: false,
            pinned: false,
        }
    }
}

impl SidebarState {
    /// Repairs what can be repaired (width, a stale active tab, duplicate
    /// singletons); refuses what can't.
    pub fn normalized(mut self) -> Result<Self, StoreError> {
        if self.tabs.len() > MAX_TABS {
            return Err(StoreError::Invalid("Too many sidebar tabs".into()));
        }
        let mut ids = HashSet::new();
        let mut kinds = HashSet::new();
        for tab in &self.tabs {
            if tab.id.is_empty() || tab.id.len() > 64 || !ids.insert(tab.id.clone()) {
                return Err(StoreError::Invalid("Sidebar tab ids must be unique".into()));
            }
            if tab.kind.singleton() && !kinds.insert(tab.kind) {
                return Err(StoreError::Invalid(
                    "That sidebar tab is already open".into(),
                ));
            }
        }
        self.width = self.width.clamp(MIN_WIDTH, MAX_WIDTH);
        if !self
            .active
            .as_ref()
            .is_some_and(|a| self.tabs.iter().any(|t| &t.id == a))
        {
            self.active = self.tabs.first().map(|t| t.id.clone());
        }
        if self.tabs.is_empty() {
            self.fullscreen = false;
        }
        Ok(self)
    }
}

pub async fn get(store: &Store) -> Result<SidebarState, StoreError> {
    let json: Option<String> = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT state_json FROM layout_sidebar WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .optional()?)
        })
        .await?;
    Ok(json
        .and_then(|j| serde_json::from_str::<SidebarState>(&j).ok())
        .and_then(|s| s.normalized().ok())
        .unwrap_or_default())
}

pub async fn save(store: &Store, state: SidebarState) -> Result<SidebarState, StoreError> {
    let state = state.normalized()?;
    let json = serde_json::to_string(&state).map_err(|e| StoreError::Invalid(e.to_string()))?;
    store
        .write(move |t| {
            t.execute(
                "INSERT INTO layout_sidebar (id, state_json, updated_at) VALUES (1, ?1, ?2)
                 ON CONFLICT(id) DO UPDATE SET state_json = excluded.state_json, updated_at = excluded.updated_at",
                params![json, now_ms()],
            )?;
            Ok(())
        })
        .await?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: &str, kind: SidebarTabKind) -> SidebarTab {
        SidebarTab {
            id: id.into(),
            kind,
            title: None,
            url: None,
        }
    }

    #[tokio::test]
    async fn f_sh_10_3_tab_order_and_active_tab_round_trip() {
        let (_d, store) = crate::tests::temp_store().await;
        assert_eq!(get(&store).await.expect("get"), SidebarState::default());
        let state = SidebarState {
            open: true,
            tabs: vec![
                tab("files", SidebarTabKind::Files),
                tab("dashboard", SidebarTabKind::Dashboard),
                tab("br_1", SidebarTabKind::Browser),
                tab("br_2", SidebarTabKind::Browser),
            ],
            active: Some("br_1".into()),
            width: 2000,
            fullscreen: true,
            pinned: false,
        };
        let saved = save(&store, state).await.expect("save");
        assert_eq!(saved.width, MAX_WIDTH);
        let back = get(&store).await.expect("get");
        assert_eq!(back, saved);
        assert_eq!(
            back.tabs.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["files", "dashboard", "br_1", "br_2"]
        );
        assert_eq!(back.active.as_deref(), Some("br_1"));
    }

    #[tokio::test]
    async fn f_sh_10_4_a_second_dashboard_is_refused_and_a_stale_active_tab_repaired() {
        let (_d, store) = crate::tests::temp_store().await;
        let two = SidebarState {
            tabs: vec![
                tab("dashboard", SidebarTabKind::Dashboard),
                tab("dashboard-2", SidebarTabKind::Dashboard),
            ],
            ..SidebarState::default()
        };
        assert!(save(&store, two).await.is_err());
        let stale = SidebarState {
            active: Some("gone".into()),
            width: 10,
            ..SidebarState::default()
        };
        let saved = save(&store, stale).await.expect("save");
        assert_eq!(saved.active.as_deref(), Some("dashboard"));
        assert_eq!(saved.width, MIN_WIDTH);
    }
}
