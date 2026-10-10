//! The Ports panel (F-EX-03): TCP ports listening in panes' process trees,
//! each with the pane that started it; Stop rechecks ownership first.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::ids::PaneId;
use tandem_core::{ErrorCode, IpcError};
use tandem_pty::ports::{self, PortProbe};
use tandem_pty::PtyManager;
use tandem_store::repo::{panes, workspaces};
use tandem_store::Store;

pub const NOT_OWNED: &str = "That process isn't running in this pane any more.";
/// Stop waits this long before saying "Still running".
pub const STOP_WAIT: Duration = Duration::from_secs(3);

/// `ports_list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ListeningPort {
    pub port: u16,
    pub address: String,
    pub all_interfaces: bool,
    pub pid: u32,
    pub process: String,
    #[ts(type = "number")]
    pub start_time: u64,
    pub pane_id: String,
    pub pane_name: Option<String>,
    pub workspace_id: Option<String>,
    pub workspace_name: Option<String>,
}

/// `ports_stop`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StopOutcome {
    /// False: still running after 3 s ("Force stop").
    pub stopped: bool,
}

pub struct PortsService {
    store: Store,
    probe: Arc<dyn PortProbe>,
}

fn unavailable(e: impl std::fmt::Display) -> IpcError {
    IpcError::new(ErrorCode::Unavailable, e.to_string()).with_stage("ports")
}

impl PortsService {
    pub fn new(store: Store) -> Arc<Self> {
        Self::with_probe(store, Arc::from(ports::system_probe()))
    }

    pub fn with_probe(store: Store, probe: Arc<dyn PortProbe>) -> Arc<Self> {
        Arc::new(Self { store, probe })
    }

    /// Live panes and their root processes.
    fn roots(pty: &PtyManager) -> Vec<(String, u32)> {
        pty.panes()
            .into_iter()
            .filter(|p| pty.is_live(p))
            .filter_map(|p| pty.pid(&p).map(|pid| (p, pid)))
            .collect()
    }

    pub async fn list(&self, pty: &PtyManager) -> Result<Vec<ListeningPort>, IpcError> {
        self.list_roots(Self::roots(pty)).await
    }

    /// The ports listening in the trees under `roots` (pane → root pid).
    pub async fn list_roots(
        &self,
        roots: Vec<(String, u32)>,
    ) -> Result<Vec<ListeningPort>, IpcError> {
        if roots.is_empty() {
            return Ok(Vec::new());
        }
        let probe = self.probe.clone();
        let (trees, listeners) = tokio::task::spawn_blocking(move || {
            let trees = ports::trees(&roots);
            let pids: HashSet<u32> = trees.keys().copied().collect();
            let listeners = probe.listeners(&pids);
            (trees, listeners)
        })
        .await
        .map_err(unavailable)?;
        let listeners = listeners.map_err(unavailable)?;
        let mut out = Vec::new();
        for l in listeners {
            let Some((pane, proc_info)) = trees.get(&l.pid) else {
                continue;
            };
            let record = panes::get(&self.store, &PaneId::from_db(pane.clone()))
                .await
                .ok();
            let ws = match &record {
                Some(r) => workspaces::get(&self.store, &r.workspace_id).await.ok(),
                None => None,
            };
            out.push(ListeningPort {
                port: l.port,
                address: l.address,
                all_interfaces: l.all_interfaces,
                pid: l.pid,
                process: proc_info.name.clone(),
                start_time: proc_info.start_time,
                pane_id: pane.clone(),
                pane_name: record.and_then(|r| r.human_name),
                workspace_id: ws.as_ref().map(|w| w.id.as_str().to_string()),
                workspace_name: ws.map(|w| w.name),
            });
        }
        out.sort_by(|a, b| a.port.cmp(&b.port).then(a.pid.cmp(&b.pid)));
        Ok(out)
    }

    /// `ports_stop`: only a process that is still in `pane`'s tree with the
    /// same start time is signalled.
    pub async fn stop(
        &self,
        pty: &PtyManager,
        pane: &PaneId,
        pid: u32,
        start_time: u64,
        force: bool,
    ) -> Result<StopOutcome, IpcError> {
        let root = pty
            .pid(pane.as_str())
            .filter(|_| pty.is_live(pane.as_str()))
            .ok_or_else(|| IpcError::new(ErrorCode::Conflict, NOT_OWNED).with_stage("ports"))?;
        stop_owned(pane.as_str(), root, pid, start_time, force).await
    }
}

/// Signals `pid` if it's in the tree under `root` with `start_time`, then
/// waits up to [`STOP_WAIT`] for it to go.
pub async fn stop_owned(
    owner: &str,
    root: u32,
    pid: u32,
    start_time: u64,
    force: bool,
) -> Result<StopOutcome, IpcError> {
    let owner = owner.to_string();
    let owned = tokio::task::spawn_blocking(move || {
        ports::trees(&[(owner, root)])
            .get(&pid)
            .is_some_and(|(_, p)| p.start_time == start_time)
    })
    .await
    .map_err(unavailable)?;
    if !owned {
        return Err(IpcError::new(ErrorCode::Conflict, NOT_OWNED).with_stage("ports"));
    }
    tracing::info!(code = "ports.stop", force, "stopping a pane's process");
    ports::signal(pid, force);
    let until = std::time::Instant::now() + STOP_WAIT;
    while std::time::Instant::now() < until {
        if !ports::alive(pid, start_time) {
            return Ok(StopOutcome { stopped: true });
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(StopOutcome {
        stopped: !ports::alive(pid, start_time),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::Command;
    use tandem_core::DataDir;
    use tandem_pty::ports::{Listener, PortsError};

    struct Fake(Vec<Listener>);
    impl PortProbe for Fake {
        fn listeners(&self, pids: &HashSet<u32>) -> Result<Vec<Listener>, PortsError> {
            Ok(self
                .0
                .iter()
                .filter(|l| pids.contains(&l.pid))
                .cloned()
                .collect())
        }
    }

    fn tree() -> (std::process::Child, u32) {
        let mut root = Command::new("sh")
            .args(["-c", "sleep 30 & echo $! ; wait"])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("spawn");
        use std::io::BufRead;
        let mut line = String::new();
        std::io::BufReader::new(root.stdout.take().expect("stdout"))
            .read_line(&mut line)
            .expect("read");
        (root, line.trim().parse().expect("pid"))
    }

    #[tokio::test]
    async fn f_ex_03_1_only_ports_in_a_panes_tree_are_listed_with_the_pane() {
        let tmp = tempfile::tempdir().expect("tmp");
        let store =
            Store::open(&DataDir::at(tmp.path().to_path_buf()).expect("data")).expect("store");
        let (mut root, child) = tree();
        let mut outsider = Command::new("sleep").arg("30").spawn().expect("spawn");
        let svc = PortsService::with_probe(
            store,
            Arc::new(Fake(vec![
                Listener {
                    port: 5173,
                    address: "127.0.0.1".into(),
                    all_interfaces: false,
                    pid: child,
                },
                Listener {
                    port: 8080,
                    address: "0.0.0.0".into(),
                    all_interfaces: true,
                    pid: outsider.id(),
                },
            ])),
        );
        let got = svc
            .list_roots(vec![("pn_a".into(), root.id())])
            .await
            .expect("list");
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(
            (
                got[0].port,
                got[0].pane_id.as_str(),
                got[0].process.as_str()
            ),
            (5173, "pn_a", "sleep")
        );
        let _ = root.kill();
        let _ = root.wait();
        let _ = outsider.kill();
        let _ = outsider.wait();
    }

    #[tokio::test]
    async fn f_ex_03_4_stop_refuses_a_process_outside_the_tree_or_with_another_start_time() {
        let (mut root, child) = tree();
        let mut outsider = Command::new("sleep").arg("30").spawn().expect("spawn");
        let start = ports::trees(&[("p".into(), root.id())])[&child]
            .1
            .start_time;
        let other_start = ports::trees(&[("p".into(), outsider.id())])[&outsider.id()]
            .1
            .start_time;
        let err = stop_owned("p", root.id(), outsider.id(), other_start, false)
            .await
            .expect_err("not owned");
        assert_eq!(err.code, ErrorCode::Conflict);
        let err = stop_owned("p", root.id(), child, start + 7, false)
            .await
            .expect_err("reused pid");
        assert_eq!(err.message, NOT_OWNED);
        assert!(ports::alive(child, start), "nothing was signalled");
        let done = stop_owned("p", root.id(), child, start, false)
            .await
            .expect("stop");
        assert!(done.stopped);
        assert!(!ports::alive(child, start));
        let _ = root.kill();
        let _ = root.wait();
        let _ = outsider.kill();
        let _ = outsider.wait();
    }
}
