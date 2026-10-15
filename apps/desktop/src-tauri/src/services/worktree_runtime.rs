//! Worktree runtime (F-WT-06): a port block and an index per worktree, the
//! variables its panes, setup, teardown and checks get, and the
//! `<slug>.localhost` address for its servers.
//!
//! Blocks live in `$DATA/worktree-ports.json`, keyed by the worktree's
//! workspace id, so they survive relaunch and never overlap across
//! repositories on this computer.

use std::collections::BTreeMap;
use std::net::{SocketAddr, TcpListener, ToSocketAddrs};
use std::path::Path;
use std::sync::{LazyLock, Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::{DataDir, ErrorCode, IpcError};
use tandem_git::setup::{self, RuntimeCtx};
use tandem_store::repo::workspaces::Workspace;

use crate::services::worktree_setup;

pub const PORTS_FILE: &str = "worktree-ports.json";
/// Blocks are taken from this range, end exclusive.
pub const PORT_RANGE: (u16, u16) = (41000, 49000);

/// A worktree's place in its repository's runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    /// The repository (its main checkout's canonical path).
    pub repo: String,
    /// 1, 2, 3… per repository, the lowest free.
    pub index: u32,
    /// The first port and how many; `None` without `ports`.
    pub ports: Option<(u16, u16)>,
}

/// `start..=end` for the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PortRange {
    pub start: u16,
    pub end: u16,
}

impl PortRange {
    fn of(start: u16, size: u16) -> Self {
        Self {
            start,
            end: start.saturating_add(size.saturating_sub(1)),
        }
    }
}

/// What making the worktree assigned (shown after it's made).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RuntimeInfo {
    pub index: u32,
    pub ports: Option<PortRange>,
    /// The block it would have had, had a port there not been busy.
    pub moved_from: Option<PortRange>,
}

/// Whether a port is taken on this computer. A trait so tests can fake it.
pub trait PortCheck {
    fn in_use(&self, port: u16) -> bool;
}

/// Something listens on 127.0.0.1 when binding it fails.
pub struct LoopbackCheck;

impl PortCheck for LoopbackCheck {
    fn in_use(&self, port: u16) -> bool {
        TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).is_err()
    }
}

static LOCK: Mutex<()> = Mutex::new(());
static DATA: OnceLock<DataDir> = OnceLock::new();

/// Called once at startup, so the terminal service can find the blocks.
pub fn init(data: &DataDir) {
    let _ = DATA.set(data.clone());
}

fn read_all(data: &DataDir) -> BTreeMap<String, Assignment> {
    std::fs::read(data.root().join(PORTS_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn write_all(data: &DataDir, all: &BTreeMap<String, Assignment>) -> Result<(), IpcError> {
    let bytes =
        serde_json::to_vec_pretty(all).map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
    tandem_core::atomic::atomic_write(&data.root().join(PORTS_FILE), &bytes, Some(0o600))
        .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()).with_stage("ports"))
}

fn overlaps(a: (u16, u16), b: (u16, u16)) -> bool {
    let (a0, a1) = (u32::from(a.0), u32::from(a.0) + u32::from(a.1));
    let (b0, b1) = (u32::from(b.0), u32::from(b.0) + u32::from(b.1));
    a0 < b1 && b0 < a1
}

/// The worktree's assignment, made now if it has none: the lowest free
/// index in its repository and, with `size`, the lowest block in
/// [`PORT_RANGE`] that overlaps no other worktree's and has no port in use.
pub fn assign(
    data: &DataDir,
    lane_id: &str,
    repo: &str,
    size: Option<u16>,
    check: &dyn PortCheck,
) -> Result<(Assignment, Option<PortRange>), IpcError> {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut all = read_all(data);
    if let Some(found) = all.get(lane_id) {
        if found.ports.map(|(_, n)| n) == size {
            return Ok((found.clone(), None));
        }
    }
    let others: Vec<&Assignment> = all
        .iter()
        .filter(|(id, _)| id.as_str() != lane_id)
        .map(|(_, a)| a)
        .collect();
    let index = all
        .get(lane_id)
        .filter(|a| a.repo == repo)
        .map(|a| a.index)
        .unwrap_or_else(|| {
            (1u32..)
                .find(|i| !others.iter().any(|a| a.repo == repo && a.index == *i))
                .unwrap_or(1)
        });
    let mut moved_from = None;
    let ports = match size {
        None => None,
        Some(n) => {
            let taken: Vec<(u16, u16)> = others.iter().filter_map(|a| a.ports).collect();
            let mut start = PORT_RANGE.0;
            let mut found = None;
            while u32::from(start) + u32::from(n) <= u32::from(PORT_RANGE.1) {
                if let Some(t) = taken.iter().find(|t| overlaps((start, n), **t)) {
                    start = t.0.saturating_add(t.1);
                    continue;
                }
                if (start..start.saturating_add(n)).any(|p| check.in_use(p)) {
                    moved_from.get_or_insert(PortRange::of(start, n));
                    start = start.saturating_add(n);
                    continue;
                }
                found = Some((start, n));
                break;
            }
            Some(found.ok_or_else(|| {
                IpcError::new(
                    ErrorCode::Conflict,
                    "No free block of ports is left for this worktree",
                )
                .with_stage("ports")
            })?)
        }
    };
    let assignment = Assignment {
        repo: repo.to_string(),
        index,
        ports,
    };
    all.insert(lane_id.to_string(), assignment.clone());
    write_all(data, &all)?;
    Ok((assignment, moved_from))
}

pub fn assignment(data: &DataDir, lane_id: &str) -> Option<Assignment> {
    read_all(data).remove(lane_id)
}

/// Frees a removed worktree's block and index.
pub fn free(data: &DataDir, lane_id: &str) {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut all = read_all(data);
    if all.remove(lane_id).is_some() {
        let _ = write_all(data, &all);
    }
}

fn repo_key(main: &Path) -> String {
    std::fs::canonicalize(main)
        .unwrap_or_else(|_| main.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn repo_name(main: &Path) -> String {
    main.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".into())
}

fn is_local_lane(lane: &Workspace) -> bool {
    lane.kind == "local" && lane.parent_id.is_some() && lane.lane_branch.is_some()
}

/// What a new worktree gets (from `lanes::create`, local worktrees only).
/// `None` when the repository's file doesn't turn the runtime on.
pub fn assign_for_new_lane(
    data: &DataDir,
    lane: &Workspace,
    check: &dyn PortCheck,
) -> Result<Option<RuntimeInfo>, IpcError> {
    if !is_local_lane(lane) {
        return Ok(None);
    }
    let main = worktree_setup::main_root(&lane.root)?;
    let Ok(Some(config)) = setup::read(&main) else {
        return Ok(None);
    };
    if !config.runtime_on() {
        return Ok(None);
    }
    let (a, moved_from) = assign(
        data,
        lane.id.as_str(),
        &repo_key(&main),
        config.ports,
        check,
    )?;
    tracing::info!(
        code = "worktree_runtime.assigned",
        index = a.index,
        "worktree runtime"
    );
    Ok(Some(RuntimeInfo {
        index: a.index,
        ports: a.ports.map(|(s, n)| PortRange::of(s, n)),
        moved_from,
    }))
}

/// What each new worktree got, until `lane_setup` reads it.
static NOTES: LazyLock<Mutex<BTreeMap<String, RuntimeInfo>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

/// Assigns a new worktree its runtime (with the data folder from [`init`])
/// and keeps what it got for `lane_setup`. Errors are logged, never fatal:
/// the worktree is made either way.
pub fn note_new_lane(lane: &Workspace) {
    let Some(data) = DATA.get() else {
        return;
    };
    match assign_for_new_lane(data, lane, &LoopbackCheck) {
        Ok(Some(info)) => {
            let mut notes = NOTES.lock().unwrap_or_else(|p| p.into_inner());
            if notes.len() > 64 {
                notes.clear();
            }
            notes.insert(lane.id.as_str().to_string(), info);
        }
        Ok(None) => {}
        Err(_) => tracing::warn!(code = "worktree_runtime.assign_failed", "no port block"),
    }
}

pub fn take_note(lane_id: &str) -> Option<RuntimeInfo> {
    NOTES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(lane_id)
}

/// [`free`] with the data folder from [`init`].
pub fn free_global(lane_id: &str) {
    if let Some(data) = DATA.get() {
        free(data, lane_id);
    }
}

/// The variables for a worktree's panes, setup, teardown and checks. Empty
/// for anything that isn't a local worktree of a repository whose file turns
/// the runtime on. The file's `env` only once it's trusted here.
pub fn vars_for(data: &DataDir, lane: &Workspace) -> BTreeMap<String, String> {
    if !is_local_lane(lane) {
        return BTreeMap::new();
    }
    let Ok(main) = worktree_setup::main_root(&lane.root) else {
        return BTreeMap::new();
    };
    let Ok(Some(config)) = setup::read(&main) else {
        return BTreeMap::new();
    };
    if !config.runtime_on() {
        return BTreeMap::new();
    }
    let assigned = match assignment(data, lane.id.as_str()) {
        Some(a) if a.ports.map(|(_, n)| n) == config.ports => a,
        // A file that gained ports after the worktree was made.
        _ => match assign(
            data,
            lane.id.as_str(),
            &repo_key(&main),
            config.ports,
            &LoopbackCheck,
        ) {
            Ok((a, _)) => a,
            Err(_) => return BTreeMap::new(),
        },
    };
    let trusted = config
        .digest()
        .is_some_and(|d| worktree_setup::is_trusted(data, &main, &d));
    let ctx = RuntimeCtx {
        slug: lane.name.clone(),
        index: assigned.index,
        repo: repo_name(&main),
        worktree: lane.root.clone(),
        ports: assigned.ports,
    };
    config.runtime_vars(&ctx, trusted)
}

/// [`vars_for`] with the data folder set by [`init`] (the terminal service).
pub fn vars_for_workspace(lane: &Workspace) -> BTreeMap<String, String> {
    match DATA.get() {
        Some(data) => vars_for(data, lane),
        None => BTreeMap::new(),
    }
}

/// The block a worktree has, for the rail and the ports panel.
pub fn ports_of(data: &DataDir, lane_id: &str) -> Option<PortRange> {
    assignment(data, lane_id)
        .and_then(|a| a.ports)
        .map(|(s, n)| PortRange::of(s, n))
}

/// [`ports_of`] with the data folder from [`init`].
pub fn ports_of_global(lane_id: &str) -> Option<PortRange> {
    DATA.get().and_then(|d| ports_of(d, lane_id))
}

/// Whether this computer resolves `<name>.localhost` to loopback (checked
/// once per run; current glibc with systemd-resolved, and the browsers, do).
pub fn localhost_subdomains_resolve() -> bool {
    static RESOLVES: LazyLock<bool> =
        LazyLock::new(|| resolves_to_loopback("vibezeo-check.localhost"));
    *RESOLVES
}

pub fn resolves_to_loopback(host: &str) -> bool {
    match (host, 80).to_socket_addrs() {
        Ok(addrs) => {
            let addrs: Vec<SocketAddr> = addrs.collect();
            !addrs.is_empty() && addrs.iter().all(|a| a.ip().is_loopback())
        }
        Err(_) => false,
    }
}

/// The host name a worktree's servers are opened at: `<slug>.localhost`
/// where that resolves, else `localhost`.
pub fn host_for(slug: Option<&str>) -> String {
    match slug {
        Some(s) if localhost_subdomains_resolve() && valid_label(s) => format!("{s}.localhost"),
        _ => "localhost".into(),
    }
}

/// A DNS label: letters, digits and inner hyphens, at most 63 characters.
fn valid_label(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && !s.starts_with('-')
        && !s.ends_with('-')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// F-WT-06 AC6: the names whose `NAME=value` the secret redactor would
/// change.
pub fn secret_looking(env: &BTreeMap<String, String>) -> Vec<String> {
    env.iter()
        .filter(|(k, v)| {
            let line = format!("{k}={v}");
            tandem_core::redact::Redactor::global().redact(&line) != line
        })
        .map(|(k, _)| k.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    struct Busy(HashSet<u16>);
    impl PortCheck for Busy {
        fn in_use(&self, port: u16) -> bool {
            self.0.contains(&port)
        }
    }

    fn data() -> (tempfile::TempDir, DataDir) {
        let tmp = tempfile::tempdir().expect("tmp");
        let data = DataDir::at(tmp.path().join("data")).expect("data");
        (tmp, data)
    }

    #[test]
    fn f_wt_06_1_blocks_are_disjoint_stable_and_freed() {
        let (_t, data) = data();
        let free_ports = Busy(HashSet::new());
        let (a, _) = assign(&data, "wt_a", "/r/app", Some(10), &free_ports).expect("a");
        let (b, _) = assign(&data, "wt_b", "/r/app", Some(10), &free_ports).expect("b");
        assert_eq!(a.ports, Some((41000, 10)));
        assert_eq!(b.ports, Some((41010, 10)));
        assert_eq!((a.index, b.index), (1, 2));
        // Another repository's blocks never overlap; its index starts at 1.
        let (c, _) = assign(&data, "wt_c", "/r/other", Some(5), &free_ports).expect("c");
        assert_eq!(c.ports, Some((41020, 5)));
        assert_eq!(c.index, 1);
        // Asking again (a relaunch) gives the same block, read from the file.
        let (again, moved) = assign(&data, "wt_a", "/r/app", Some(10), &free_ports).expect("again");
        assert_eq!(again, a);
        assert_eq!(moved, None);
        assert!(data.root().join(PORTS_FILE).is_file());
        // Freeing makes the block and the index available again.
        free(&data, "wt_a");
        let (d, _) = assign(&data, "wt_d", "/r/app", Some(10), &free_ports).expect("d");
        assert_eq!(d.ports, Some((41000, 10)));
        assert_eq!(d.index, 1);
    }

    #[test]
    fn f_wt_06_3_a_busy_port_moves_the_worktree_to_the_next_block_and_says_so() {
        let (_t, data) = data();
        let busy = Busy([41004].into_iter().collect());
        let (a, moved) = assign(&data, "wt_a", "/r/app", Some(10), &busy).expect("a");
        assert_eq!(a.ports, Some((41010, 10)));
        assert_eq!(
            moved,
            Some(PortRange {
                start: 41000,
                end: 41009
            })
        );
    }

    #[test]
    fn f_wt_06_3_a_full_range_says_so() {
        let (_t, data) = data();
        struct AllBusy;
        impl PortCheck for AllBusy {
            fn in_use(&self, _: u16) -> bool {
                true
            }
        }
        let err = assign(&data, "wt_a", "/r/app", Some(100), &AllBusy).expect_err("full");
        assert!(err.message.contains("No free block"), "{}", err.message);
    }

    #[test]
    fn f_wt_06_4_hosts_use_the_slug_only_when_it_is_a_label() {
        assert!(resolves_to_loopback("localhost"));
        assert_eq!(host_for(None), "localhost");
        assert_eq!(host_for(Some("bad slug")), "localhost");
        let expected = if localhost_subdomains_resolve() {
            "fix-auth.localhost"
        } else {
            "localhost"
        };
        assert_eq!(host_for(Some("fix-auth")), expected);
    }

    #[test]
    fn f_wt_06_6_secret_looking_values_are_named() {
        let env: BTreeMap<String, String> = [
            (
                "DATABASE_URL".to_string(),
                "postgres://localhost/app_{{slug}}".to_string(),
            ),
            (
                "STRIPE_KEY".to_string(),
                "sk_live_51HcXyZabcdefghijklmnopqrstuvwxyz0123456789".to_string(),
            ),
        ]
        .into_iter()
        .collect();
        assert_eq!(secret_looking(&env), vec!["STRIPE_KEY".to_string()]);
    }

    fn git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git runs")
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    #[tokio::test]
    async fn f_wt_06_2_a_worktree_gets_its_variables_and_the_files_env_once_trusted() {
        use tandem_store::repo::workspaces;
        let (tmp, data) = data();
        let main = tmp.path().join("shop");
        std::fs::create_dir_all(main.join(".vibezeo")).expect("mkdir");
        git(&main, &["init", "-q", "-b", "main"]);
        std::fs::write(main.join("README.md"), "x\n").expect("write");
        git(&main, &["add", "."]);
        git(&main, &["commit", "-q", "-m", "init"]);
        std::fs::write(
            main.join(setup::SETUP_FILE),
            r#"{"ports":10,"env":{"DB":"app_{{slug}}","PORT":"{{port+1}}"}}"#,
        )
        .expect("write");
        let wt = main.join(".tandem/worktrees/fix-auth");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                wt.to_str().expect("utf8"),
                "-b",
                "tandem/fix-auth",
            ],
        );
        let store = tandem_store::Store::open(&data).expect("store");
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
        let lane = workspaces::add_lane(
            &store,
            workspaces::NewLane {
                parent_id: parent.id.clone(),
                root: wt.to_string_lossy().into_owned(),
                name: "fix-auth".into(),
                branch: "tandem/fix-auth".into(),
                base: "abc".into(),
            },
        )
        .await
        .expect("lane");
        let info = assign_for_new_lane(&data, &lane, &Busy(HashSet::new()))
            .expect("assign")
            .expect("runtime on");
        assert_eq!(
            info.ports,
            Some(PortRange {
                start: 41000,
                end: 41009
            })
        );
        let before = vars_for(&data, &lane);
        assert_eq!(before["COMPOSE_PROJECT_NAME"], "shop-fix-auth");
        assert_eq!(before["VIBEZEO_PORT"], "41000");
        assert!(!before.contains_key("DB"), "untrusted env stays out");
        let digest = setup::read(&main)
            .expect("read")
            .expect("some")
            .digest()
            .expect("digest");
        worktree_setup::trust(&data, &main, &digest).expect("trust");
        let after = vars_for(&data, &lane);
        assert_eq!(after["DB"], "app_fix-auth");
        assert_eq!(after["PORT"], "41001");
        // The parent checkout itself is no worktree: nothing.
        assert!(vars_for(&data, &parent).is_empty());
    }
}
