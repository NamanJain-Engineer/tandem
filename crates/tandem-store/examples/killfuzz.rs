//! Store half of `scripts/killfuzz.ts` (F-SH-07): seeds a profile, hammers
//! it with the writes the app makes (so a kill -9 lands mid-transaction),
//! and checks what a crash left behind.
//!
//! ```text
//! killfuzz seed  <data-dir> <workspace-root>   one workspace: 4 shells, 2 agents
//! killfuzz write <data-dir>                    writes until killed
//! killfuzz check <data-dir> [--launched]       prints {"ok", "problems"}
//! ```

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tandem_core::ids::PaneId;
use tandem_core::settings::SettingKey;
use tandem_core::DataDir;
use tandem_store::repo::layouts::{self, LayoutNode};
use tandem_store::repo::{panes, sessions, settings, tabs, workspaces};
use tandem_store::Store;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn leaf(id: &PaneId) -> LayoutNode {
    LayoutNode::Leaf {
        pane_id: id.as_str().to_string(),
    }
}

fn split(dir: &str, ratio: f32, a: LayoutNode, b: LayoutNode) -> LayoutNode {
    LayoutNode::Split {
        dir: dir.into(),
        ratio,
        a: Box::new(a),
        b: Box::new(b),
    }
}

/// Two rows of three: shells on top, then a shell and two agents.
fn grid(ids: &[PaneId], ratio: f32) -> LayoutNode {
    let row = |p: &[PaneId]| {
        split(
            "row",
            1.0 / 3.0,
            leaf(&p[0]),
            split("row", 0.5, leaf(&p[1]), leaf(&p[2])),
        )
    };
    split("col", ratio, row(&ids[0..3]), row(&ids[3..6]))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

async fn seed(store: &Store, root: &str) -> Result<()> {
    std::fs::create_dir_all(root)?;
    let (ws, _) = workspaces::add(
        store,
        workspaces::NewWorkspace {
            kind: "local".into(),
            host_id: "local".into(),
            root: root.into(),
            name: "killfuzz".into(),
            wsl_distro: None,
        },
        None,
    )
    .await?;
    let tab = tabs::create(store, &ws.id, None).await?;
    let mut ids = Vec::new();
    for i in 0..4 {
        let dir = PathBuf::from(root).join(format!("dir-{i}"));
        std::fs::create_dir_all(&dir)?;
        let p = panes::create(
            store,
            &tab.id,
            "terminal",
            Some(dir.to_string_lossy().into_owned()),
        )
        .await?;
        ids.push(p.id);
    }
    for i in 0..2 {
        let p = panes::create_with(store, &tab.id, "agent", None, Some("claude".into())).await?;
        let config = sessions::LaunchConfig {
            extra_args: Vec::new(),
            home: Some(format!("{root}/home-{i}")),
        };
        sessions::start_with_config(store, &p.id, &ws.id, "claude", None, &config).await?;
        sessions::set_engine_session(store, &p.id, &format!("fuzz-{i}")).await?;
        ids.push(p.id);
    }
    layouts::save(store, &tab.id, grid(&ids, 0.5), Some(ids[0].clone()), None).await?;
    // Offline mode, plain shells: the app opens straight into Code mode.
    settings::set(
        store,
        SettingKey::OfflineStartedAt,
        serde_json::json!(now_ms()),
    )
    .await?;
    settings::set(
        store,
        SettingKey::DefaultShell,
        serde_json::json!("/bin/sh"),
    )
    .await?;
    settings::set(store, SettingKey::LastMode, serde_json::json!("code")).await?;
    println!(
        "{}",
        serde_json::json!({ "workspace": ws.id, "tab": tab.id, "panes": ids })
    );
    Ok(())
}

/// The app's writes, as fast as the store takes them.
async fn write(store: &Store) -> Result<()> {
    let ws = workspaces::list(store).await?;
    let ws = ws.first().ok_or("seed first")?;
    let tab = tabs::list(store, &ws.id).await?.remove(0);
    let ids: Vec<PaneId> = panes::list_open(store, &tab.id)
        .await?
        .into_iter()
        .map(|p| p.id)
        .collect();
    let mut n: u64 = now_ms() as u64;
    loop {
        // A cheap LCG: varied values without a dependency.
        n = n
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let ratio = 0.2 + ((n >> 33) % 600) as f32 / 1000.0;
        layouts::save(
            store,
            &tab.id,
            grid(&ids, ratio),
            Some(ids[(n % 6) as usize].clone()),
            None,
        )
        .await?;
        let shell = &ids[(n % 4) as usize];
        panes::set_cwd(store, shell, format!("{}/dir-{}", ws.root, n % 4)).await?;
        let agent = &ids[4 + (n % 2) as usize];
        sessions::set_engine_session(store, agent, &format!("fuzz-{}", n % 1000)).await?;
        settings::set(store, SettingKey::LastMode, serde_json::json!("code")).await?;
    }
}

async fn check(store: &Store, launched: bool) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    let integrity: String = store
        .read(|c| Ok(c.query_row("PRAGMA integrity_check", [], |r| r.get(0))?))
        .await?;
    if integrity != "ok" {
        problems.push(format!("integrity_check: {integrity}"));
    }
    let fk: usize = store
        .read(|c| {
            let mut st = c.prepare("PRAGMA foreign_key_check")?;
            let n = st.query_map([], |_| Ok(()))?.count();
            Ok(n)
        })
        .await?;
    if fk > 0 {
        problems.push(format!("{fk} foreign key violations"));
    }
    for ws in workspaces::list(store).await? {
        let list = tabs::list(store, &ws.id).await?;
        for layout in layouts::for_workspace(store, list).await? {
            let open: HashSet<String> = panes::list_open(store, &layout.tab.id)
                .await?
                .into_iter()
                .map(|p| p.id.as_str().to_string())
                .collect();
            match &layout.tree {
                Some(tree) => {
                    if let Err(err) = layouts::validate(tree, &open) {
                        problems.push(format!(
                            "tab {}: layout doesn't match its panes: {err}",
                            layout.tab.id
                        ));
                    }
                }
                None if !open.is_empty() => {
                    problems.push(format!("tab {}: panes but no layout", layout.tab.id))
                }
                None => {}
            }
            for p in panes::list_open(store, &layout.tab.id).await? {
                let want_paused = launched && p.kind == "agent";
                if want_paused && p.restore_state != "paused" {
                    problems.push(format!(
                        "agent pane {} is {} after a launch, not paused",
                        p.id, p.restore_state
                    ));
                }
                if p.kind == "terminal" && p.restore_state != "none" {
                    problems.push(format!(
                        "shell pane {} has restore state {}",
                        p.id, p.restore_state
                    ));
                }
            }
        }
    }
    Ok(problems)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, dir) = match (args.first(), args.get(1)) {
        (Some(c), Some(d)) => (c.as_str(), PathBuf::from(d)),
        _ => return Err("usage: killfuzz seed|write|check <data-dir> …".into()),
    };
    let data = DataDir::at(dir)?;
    let store = Store::open(&data)?;
    match cmd {
        "seed" => seed(&store, args.get(2).ok_or("seed needs a workspace root")?).await,
        "write" => write(&store).await,
        "check" => {
            let problems = check(&store, args.iter().any(|a| a == "--launched")).await?;
            println!(
                "{}",
                serde_json::json!({ "ok": problems.is_empty(), "problems": problems })
            );
            // Let the writer thread finish before exit.
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok(())
        }
        other => Err(format!("unknown command {other}").into()),
    }
}
