//! Intent radar (F-WT-10): each local worktree's changed symbols, kept from
//! its last parse, refreshed after each checkpoint in its folder and when
//! its overlap is checked. A pair of sibling worktrees that newly share a
//! symbol after a checkpoint raises `radar:overlap` once; Warn both agents
//! types `lane_radar_note`'s text, only on a click.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::ids::WorkspaceId;
use tandem_core::{untrusted, DataDir, ErrorCode, IpcError};
use tandem_git::symbols::{self, SharedSymbol, SymbolChange, SymbolIndex};
use tandem_git::Git;
use tandem_store::repo::workspaces::{self, Workspace};
use tandem_store::Store;

use crate::services::lanes::git_for;
use crate::services::terminal::TerminalService;

/// A worktree's last index.
#[derive(Debug, Clone)]
struct Entry {
    parent: String,
    index: SymbolIndex,
    /// Parses by file: a refresh parses only what changed since.
    cache: symbols::ParseCache,
}

static INDEXES: LazyLock<Mutex<HashMap<String, Entry>>> = LazyLock::new(Mutex::default);
/// Pairs and symbols already announced, so a notice isn't repeated.
static WARNED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Mutex::default);

/// `lane_radar`: what a worktree shares with its siblings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RadarMark {
    pub lane_id: String,
    /// The shared symbols' qualified names, sorted.
    pub symbols: Vec<String>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether the radar covers this workspace: a local worktree with a base.
fn covered(ws: &Workspace) -> bool {
    ws.kind == "local"
        && ws.parent_id.is_some()
        && ws.lane_branch.is_some()
        // A multi-repo worktree (F-CD-22) has no single base: not covered yet.
        && ws.lane_base.as_deref().is_some_and(|b| !b.is_empty())
        && Path::new(&ws.root).is_dir()
}

/// Parses the worktree again and keeps the result.
pub async fn refresh(git: &Git, lane: &Workspace) -> Result<SymbolIndex, IpcError> {
    let (Some(parent), Some(base)) = (&lane.parent_id, &lane.lane_base) else {
        return Ok(SymbolIndex::default());
    };
    if !covered(lane) {
        return Ok(SymbolIndex::default());
    }
    let mut cache = lock(&INDEXES)
        .get(lane.id.as_str())
        .map(|e| e.cache.clone())
        .unwrap_or_default();
    let done = symbols::index_cached(git, Path::new(&lane.root), base, &mut cache).await?;
    tracing::debug!(
        code = "radar.refreshed",
        parsed = done.parsed,
        "radar index refreshed"
    );
    lock(&INDEXES).insert(
        lane.id.as_str().to_string(),
        Entry {
            parent: parent.as_str().to_string(),
            index: done.index.clone(),
            cache,
        },
    );
    Ok(done.index)
}

/// The last index, parsing it first when there's none.
pub async fn index_of(git: &Git, lane: &Workspace) -> Result<SymbolIndex, IpcError> {
    if let Some(e) = lock(&INDEXES).get(lane.id.as_str()) {
        return Ok(e.index.clone());
    }
    refresh(git, lane).await
}

/// What `here` shares with `there` (F-WT-10).
pub fn shared(here: &SymbolIndex, there: &SymbolIndex) -> Vec<SharedSymbol> {
    symbols::shared_symbols(here, there)
}

/// `lane_radar`: from the kept indexes only; nothing is parsed. Only the
/// given worktrees are compared, so one since discarded doesn't count.
pub fn marks(lane_ids: &[String]) -> Vec<RadarMark> {
    let wanted: HashSet<&String> = lane_ids.iter().collect();
    let all: HashMap<String, Entry> = lock(&INDEXES)
        .iter()
        .filter(|(id, _)| wanted.contains(id))
        .map(|(id, e)| (id.clone(), e.clone()))
        .collect();
    let mut out = Vec::new();
    for id in lane_ids {
        let Some(me) = all.get(id) else { continue };
        let mut names = BTreeSet::new();
        for (other_id, other) in &all {
            if other_id != id && other.parent == me.parent {
                names.extend(
                    shared(&me.index, &other.index)
                        .into_iter()
                        .map(|s| s.qualified),
                );
            }
        }
        if !names.is_empty() {
            out.push(RadarMark {
                lane_id: id.clone(),
                symbols: names.into_iter().collect(),
            });
        }
    }
    out
}

/// Forgets a worktree's index (it was discarded or merged).
pub fn forget(lane_id: &str) {
    lock(&INDEXES).remove(lane_id);
}

fn pair_key(a: &str, b: &str, symbols: &[String]) -> String {
    let (x, y) = if a < b { (a, b) } else { (b, a) };
    format!("{x}|{y}|{}", symbols.join(","))
}

/// After a checkpoint in `folder`: refresh the worktree there, parse the
/// siblings not parsed yet, and announce pairs newly sharing symbols.
pub async fn on_checkpoint(store: &Store, git: &Git, bus: &EventBus, folder: &str) {
    let folder = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.into());
    let Ok(all) = workspaces::list(store).await else {
        return;
    };
    let same = |ws: &Workspace| std::fs::canonicalize(&ws.root).is_ok_and(|r| r == folder);
    let Some(lane) = all.iter().find(|w| covered(w) && same(w)) else {
        return;
    };
    // F-CD-22: no radar for multi-repo worktrees yet.
    if crate::services::lanes::is_multi(store, lane)
        .await
        .unwrap_or(false)
    {
        return;
    }
    let Ok(mine) = refresh(git, lane).await else {
        tracing::warn!(code = "radar.parse_failed", "radar index skipped");
        return;
    };
    let parent = lane.parent_id.clone();
    for sib in all
        .iter()
        .filter(|w| w.id != lane.id && w.parent_id == parent && covered(w))
    {
        let Ok(theirs) = index_of(git, sib).await else {
            continue;
        };
        let found = shared(&mine, &theirs);
        if found.is_empty() {
            continue;
        }
        let names: Vec<String> = found.iter().map(|s| s.qualified.clone()).collect();
        if !lock(&WARNED).insert(pair_key(lane.id.as_str(), sib.id.as_str(), &names)) {
            continue;
        }
        tracing::info!(
            code = "radar.overlap",
            symbols = names.len(),
            "worktrees share symbols"
        );
        bus.publish(BusEvent::new(
            Topic::App,
            "radar:overlap",
            serde_json::json!({
                "repoId": parent.as_ref().map(|p| p.as_str()),
                "a": { "laneId": lane.id.as_str(), "branch": lane.lane_branch },
                "b": { "laneId": sib.id.as_str(), "branch": sib.lane_branch },
                "symbols": names,
            }),
        ));
    }
}

/// Listens for checkpoints for as long as the app runs.
pub async fn run(store: Store, data: DataDir, bus: EventBus, terminals: Arc<TerminalService>) {
    let mut rx = bus.subscribe(Topic::App);
    loop {
        match rx.recv().await {
            Ok(ev) if ev.name == "checkpoint:created" => {
                let Some(folder) = ev.payload.get("folder").and_then(|f| f.as_str()) else {
                    continue;
                };
                let login = terminals.login_env().await;
                if let Ok(git) = git_for(&data, &login) {
                    on_checkpoint(&store, &git, &bus, folder).await;
                }
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(_) => return,
        }
    }
}

fn what(c: SymbolChange) -> &'static str {
    match c {
        SymbolChange::Added => "added",
        SymbolChange::Changed => "changed",
        SymbolChange::Removed => "renamed or removed",
        SymbolChange::Used => "used",
    }
}

/// The note Warn both agents types into `lane`'s agent: what the sibling
/// changed that this worktree also touches. Names and branches come from
/// code and agents, so they're data behind the fence, and no `@` there names
/// a file for the agent to attach: TypeScript names a module or a method
/// with any string (D-3022).
pub fn note_text(other_branch: &str, found: &[SharedSymbol]) -> String {
    let mut data = format!("Other worktree: {other_branch}\n");
    for s in found {
        data.push_str(&format!(
            "- {}: {} here, {} there\n",
            s.qualified,
            what(s.here),
            what(s.there)
        ));
    }
    format!(
        "Heads-up from Vibezeo: another agent, working in a parallel worktree of this \
         repository, is changing code that your changes also touch. Check that your work \
         still fits theirs (a renamed or changed symbol you use, or the same one changed \
         twice), and adjust before you finish.\n\n{}\n{}",
        untrusted::label("the overlapping symbols"),
        untrusted::fence_defanged("text", data.trim_end())
    )
}

/// `lane_radar_note`.
pub async fn note(
    store: &Store,
    git: &Git,
    lane_id: &WorkspaceId,
    other_id: &WorkspaceId,
) -> Result<String, IpcError> {
    let lane = workspaces::get(store, lane_id).await?;
    let other = workspaces::get(store, other_id).await?;
    if lane.parent_id.is_none() || lane.parent_id != other.parent_id || lane.id == other.id {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "Those aren't two worktrees of one repository",
        ));
    }
    let found = shared(&index_of(git, &lane).await?, &index_of(git, &other).await?);
    if found.is_empty() {
        return Err(IpcError::new(
            ErrorCode::NotFound,
            "These worktrees don't share symbols any more",
        ));
    }
    Ok(note_text(
        other.lane_branch.as_deref().unwrap_or(&other.name),
        &found,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    struct Fx {
        _tmp: tempfile::TempDir,
        store: Store,
        git: Git,
        lanes: Vec<Workspace>,
    }

    /// `shop` with `src/session.ts`, and one worktree per name.
    async fn fixture(names: &[&str]) -> Fx {
        let tmp = tempfile::tempdir().expect("tmp");
        let data = DataDir::at(tmp.path().join("data")).expect("data");
        let main = tmp.path().join("shop");
        std::fs::create_dir_all(main.join("src")).expect("mkdir");
        git(&main, &["init", "-q", "-b", "main"]);
        for (k, v) in [
            ("user.name", "t"),
            ("user.email", "t@t"),
            ("commit.gpgsign", "false"),
        ] {
            git(&main, &["config", k, v]);
        }
        std::fs::write(
            main.join("src/session.ts"),
            "export class Session {\n  refresh(): void {}\n}\n",
        )
        .expect("write");
        git(&main, &["add", "."]);
        git(&main, &["commit", "-q", "-m", "init"]);
        let base = git(&main, &["rev-parse", "HEAD"]);
        let store = Store::open(&data).expect("store");
        let (parent, _) = workspaces::add(
            &store,
            workspaces::NewWorkspace {
                kind: "local".into(),
                host_id: "local".into(),
                root: main.to_string_lossy().into_owned(),
                name: "shop".into(),
                wsl_distro: None,
            },
            None,
        )
        .await
        .expect("parent");
        let mut lanes = Vec::new();
        for name in names {
            let wt = main.join(".tandem/worktrees").join(name);
            let branch = format!("tandem/{name}");
            git(
                &main,
                &[
                    "worktree",
                    "add",
                    "-q",
                    wt.to_str().expect("utf8"),
                    "-b",
                    &branch,
                ],
            );
            lanes.push(
                workspaces::add_lane(
                    &store,
                    workspaces::NewLane {
                        parent_id: parent.id.clone(),
                        root: wt.to_string_lossy().into_owned(),
                        name: (*name).into(),
                        branch,
                        base: base.clone(),
                    },
                )
                .await
                .expect("lane"),
            );
        }
        let hooks = tmp.path().join("hooks");
        std::fs::create_dir_all(&hooks).expect("hooks");
        Fx {
            git: Git::new("git", hooks),
            _tmp: tmp,
            store,
            lanes,
        }
    }

    #[tokio::test]
    async fn f_wt_10_1_4_a_rename_and_a_use_are_announced_once_and_noted_in_a_fence() {
        let fx = fixture(&["rename", "caller"]).await;
        let (a, b) = (&fx.lanes[0], &fx.lanes[1]);
        std::fs::write(
            Path::new(&a.root).join("src/session.ts"),
            "export class UserSession {\n  refresh(): void {}\n}\n",
        )
        .expect("write");
        std::fs::write(
            Path::new(&b.root).join("src/use.ts"),
            "import { Session } from './session';\nnew Session().refresh();\n",
        )
        .expect("write");
        let bus = EventBus::new();
        let mut rx = bus.subscribe(Topic::App);
        on_checkpoint(&fx.store, &fx.git, &bus, &b.root).await;
        let ev = rx.try_recv().expect("an overlap notice");
        assert_eq!(ev.name, "radar:overlap");
        assert_eq!(ev.payload["a"]["branch"], "tandem/caller");
        assert_eq!(ev.payload["b"]["branch"], "tandem/rename");
        assert_eq!(
            ev.payload["symbols"],
            serde_json::json!(["Session", "Session.refresh"])
        );
        // The same overlap again: no second notice.
        on_checkpoint(&fx.store, &fx.git, &bus, &b.root).await;
        assert!(rx.try_recv().is_err());
        // Both worktrees are marked.
        let ids: Vec<String> = fx.lanes.iter().map(|l| l.id.as_str().to_string()).collect();
        let m = marks(&ids);
        assert_eq!(m.len(), 2);
        assert!(m
            .iter()
            .all(|m| m.symbols == ["Session", "Session.refresh"]));
        // The note for the caller's agent: the names inside the fence.
        let text = note(&fx.store, &fx.git, &b.id, &a.id).await.expect("note");
        assert!(text.starts_with("Heads-up from Vibezeo"));
        assert!(text.contains(&untrusted::label("the overlapping symbols")));
        let fence = text.find("```text\n").expect("fence");
        assert!(text[fence..].contains("Other worktree: tandem/rename"));
        assert!(text[fence..].contains("- Session: used here, renamed or removed there"));
        for id in &ids {
            forget(id);
        }
    }

    #[test]
    fn f_wt_10_4_no_at_in_a_symbols_name_names_a_file_for_the_agent() {
        // TypeScript names a module or a method with a string, which can
        // spell any path, and another agent wrote it (D-3022).
        let defs = symbols::definitions(
            symbols::Lang::TypeScript,
            "declare module \"@~/.ssh/id_rsa\" {\n  export function f(): void;\n}\n\
             class A {\n  \"@/etc/passwd\"() {}\n}\n",
        );
        let found: Vec<SharedSymbol> = defs
            .iter()
            .map(|d| SharedSymbol {
                name: d.name.clone(),
                qualified: d.qualified.clone(),
                here: SymbolChange::Used,
                there: SymbolChange::Changed,
            })
            .collect();
        assert!(
            found.iter().any(|s| s.qualified.contains("@~/.ssh/id_rsa"))
                && found.iter().any(|s| s.qualified.contains("@/etc/passwd")),
            "{found:?}"
        );
        let text = note_text("tandem/other", &found);
        assert!(!text.contains("@~/") && !text.contains("@/etc"), "{text}");
        assert!(text.contains("\u{FF20}~/.ssh/id_rsa") && text.contains("\u{FF20}/etc/passwd"));
        assert!(text.ends_with(untrusted::DEFANGED), "{text}");
        // Plain names: just the fence, with no note.
        let plain = SharedSymbol {
            name: "refresh".into(),
            qualified: "Session.refresh".into(),
            here: SymbolChange::Used,
            there: SymbolChange::Changed,
        };
        assert!(!note_text("tandem/other", &[plain]).contains(untrusted::DEFANGED));
    }

    #[tokio::test]
    async fn f_wt_10_3_unrelated_work_shares_nothing() {
        let fx = fixture(&["one", "two"]).await;
        for (lane, name) in fx.lanes.iter().zip(["alpha", "beta"]) {
            std::fs::write(
                Path::new(&lane.root).join(format!("src/{name}.ts")),
                format!("export function {name}() {{ return 1; }}\n"),
            )
            .expect("write");
        }
        let bus = EventBus::new();
        let mut rx = bus.subscribe(Topic::App);
        on_checkpoint(&fx.store, &fx.git, &bus, &fx.lanes[0].root).await;
        assert!(rx.try_recv().is_err());
        let e = note(&fx.store, &fx.git, &fx.lanes[0].id, &fx.lanes[1].id)
            .await
            .expect_err("nothing shared");
        assert_eq!(e.code, ErrorCode::NotFound);
        for l in &fx.lanes {
            forget(l.id.as_str());
        }
    }
}
