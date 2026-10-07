//! `layout_trees` (F-CD-02): one split tree per tab. The UI owns the tree;
//! the store validates it (every leaf is an open pane of the tab, no
//! duplicates, ratios in range, depth ≤ 16) before saving.

use std::collections::HashSet;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use tandem_core::ids::{PaneId, TabId};
use ts_rs::TS;

use crate::repo::tabs::Tab;
use crate::{now_ms, Store, StoreError};

pub const MAX_DEPTH: usize = 16;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum LayoutNode {
    Leaf {
        #[serde(rename = "paneId")]
        #[ts(rename = "paneId")]
        pane_id: String,
    },
    Split {
        /// `row` (side by side) or `col` (stacked).
        dir: String,
        ratio: f32,
        a: Box<LayoutNode>,
        b: Box<LayoutNode>,
    },
}

impl LayoutNode {
    pub fn leaves(&self) -> Vec<&str> {
        match self {
            LayoutNode::Leaf { pane_id } => vec![pane_id.as_str()],
            LayoutNode::Split { a, b, .. } => {
                let mut v = a.leaves();
                v.extend(b.leaves());
                v
            }
        }
    }

    pub fn depth(&self) -> usize {
        match self {
            LayoutNode::Leaf { .. } => 1,
            LayoutNode::Split { a, b, .. } => 1 + a.depth().max(b.depth()),
        }
    }

    fn check_splits(&self) -> Result<(), StoreError> {
        if let LayoutNode::Split { dir, ratio, a, b } = self {
            if dir != "row" && dir != "col" {
                return Err(StoreError::Invalid(
                    "Split direction must be row or col".into(),
                ));
            }
            if !(0.05..=0.95).contains(ratio) {
                return Err(StoreError::Invalid(
                    "Split ratio must be between 0.05 and 0.95".into(),
                ));
            }
            a.check_splits()?;
            b.check_splits()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TabLayout {
    pub tab: Tab,
    /// Absent until the tab's first pane opens.
    pub tree: Option<LayoutNode>,
    pub active_pane_id: Option<PaneId>,
    pub maximized_pane_id: Option<PaneId>,
}

/// Rejects trees the UI must never persist (F-CD-02, §3.2).
pub fn validate(tree: &LayoutNode, open_panes: &HashSet<String>) -> Result<(), StoreError> {
    if tree.depth() > MAX_DEPTH {
        return Err(StoreError::Invalid("The layout is too deep".into()));
    }
    tree.check_splits()?;
    let mut seen = HashSet::new();
    for id in tree.leaves() {
        if !seen.insert(id) {
            return Err(StoreError::Invalid(
                "A pane appears twice in the layout".into(),
            ));
        }
        if !open_panes.contains(id) {
            return Err(StoreError::Invalid(
                "The layout names a pane that isn't open in this tab".into(),
            ));
        }
    }
    Ok(())
}

pub async fn save(
    store: &Store,
    tab: &TabId,
    tree: LayoutNode,
    active: Option<PaneId>,
    maximized: Option<PaneId>,
) -> Result<(), StoreError> {
    let tab = tab.as_str().to_string();
    store
        .write(move |t| {
            let open: HashSet<String> = {
                let mut stmt = t.prepare("SELECT id FROM panes WHERE tab_id = ?1 AND closed_at IS NULL")?;
                let rows = stmt.query_map([&tab], |r| r.get::<_, String>(0))?.collect::<Result<_, _>>()?;
                rows
            };
            validate(&tree, &open)?;
            for p in [&active, &maximized].into_iter().flatten() {
                if !open.contains(p.as_str()) {
                    return Err(StoreError::Invalid("The focused pane isn't in this tab".into()));
                }
            }
            let json = serde_json::to_string(&tree).map_err(|e| StoreError::Invalid(e.to_string()))?;
            t.execute(
                "INSERT INTO layout_trees (tab_id, tree_json, active_pane_id, maximized_pane_id, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(tab_id) DO UPDATE SET tree_json = excluded.tree_json, active_pane_id = excluded.active_pane_id,
                   maximized_pane_id = excluded.maximized_pane_id, updated_at = excluded.updated_at",
                params![tab, json, active.as_ref().map(|p| p.as_str()), maximized.as_ref().map(|p| p.as_str()), now_ms()],
            )?;
            Ok(())
        })
        .await
}

/// Layouts for every tab of a workspace, in tab order.
pub async fn for_workspace(store: &Store, tabs: Vec<Tab>) -> Result<Vec<TabLayout>, StoreError> {
    store
        .read(move |c| {
            let mut out = Vec::new();
            for tab in tabs {
                let row: Option<(String, Option<String>, Option<String>)> = c
                    .query_row(
                        "SELECT tree_json, active_pane_id, maximized_pane_id FROM layout_trees WHERE tab_id = ?1",
                        [tab.id.as_str()],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .optional()?;
                let (tree, active, maximized) = match row {
                    Some((json, a, m)) => (serde_json::from_str(&json).ok(), a, m),
                    None => (None, None, None),
                };
                out.push(TabLayout {
                    tab,
                    tree,
                    active_pane_id: active.map(PaneId::from_db),
                    maximized_pane_id: maximized.map(PaneId::from_db),
                });
            }
            Ok(out)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{panes, tabs, workspaces};

    fn leaf(id: &str) -> LayoutNode {
        LayoutNode::Leaf { pane_id: id.into() }
    }

    fn split(a: LayoutNode, b: LayoutNode, ratio: f32) -> LayoutNode {
        LayoutNode::Split {
            dir: "row".into(),
            ratio,
            a: Box::new(a),
            b: Box::new(b),
        }
    }

    #[test]
    fn f_cd_02_validation_rules() {
        let open: HashSet<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        assert!(validate(&split(leaf("a"), leaf("b"), 0.5), &open).is_ok());
        assert!(
            validate(&split(leaf("a"), leaf("a"), 0.5), &open).is_err(),
            "duplicate"
        );
        assert!(
            validate(&split(leaf("a"), leaf("zz"), 0.5), &open).is_err(),
            "unknown pane"
        );
        assert!(
            validate(&split(leaf("a"), leaf("b"), 0.99), &open).is_err(),
            "ratio"
        );
        let mut deep = leaf("a");
        for _ in 0..16 {
            deep = split(deep, leaf("b"), 0.5);
        }
        assert!(validate(&deep, &open).is_err(), "depth");
    }

    #[test]
    fn layout_json_matches_the_typescript_shape() {
        let tree = split(leaf("pn_1"), leaf("pn_2"), 0.4);
        let json = serde_json::to_value(&tree).expect("json");
        assert_eq!(json["type"], "split");
        assert_eq!(json["a"]["type"], "leaf");
        assert_eq!(json["a"]["paneId"], "pn_1");
        let back: LayoutNode = serde_json::from_value(json).expect("parse");
        assert_eq!(back, tree);
    }

    #[tokio::test]
    async fn f_cd_02_layouts_persist_per_tab() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = tandem_core::DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let (ws, _) = workspaces::add(
            &store,
            workspaces::NewWorkspace {
                kind: "local".into(),
                host_id: "local".into(),
                root: "/l".into(),
                name: "l".into(),
                wsl_distro: None,
            },
            None,
        )
        .await
        .expect("ws");
        let tab = tabs::create(&store, &ws.id, None).await.expect("tab");
        let p1 = panes::create(&store, &tab.id, "terminal", None)
            .await
            .expect("p1");
        let p2 = panes::create(&store, &tab.id, "terminal", None)
            .await
            .expect("p2");
        let tree = split(leaf(p1.id.as_str()), leaf(p2.id.as_str()), 0.3);
        save(&store, &tab.id, tree.clone(), Some(p2.id.clone()), None)
            .await
            .expect("save");
        let got = for_workspace(&store, tabs::list(&store, &ws.id).await.expect("tabs"))
            .await
            .expect("get");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].tree.as_ref(), Some(&tree));
        assert_eq!(got[0].active_pane_id.as_ref(), Some(&p2.id));
        // A closed pane can't be saved into the layout.
        panes::close(&store, &p2.id).await.expect("close");
        assert!(matches!(
            save(&store, &tab.id, tree, None, None).await,
            Err(StoreError::Invalid(_))
        ));
    }
}
