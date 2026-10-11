//! Real-PTY tests on Windows (ConPTY through portable-pty, and the pane's
//! kill-on-close Job Object). The Unix suite is `tests/pty.rs`. These run
//! under Wine from Linux (`scripts/windows-check.sh --wine`) and on Windows;
//! what Wine can't show is listed in HUMAN_TODO.
#![cfg(windows)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tandem_pty::{wait_until, FrameKind, OutputSink, PtyManager, SpawnSpec, VecSink};

fn cmd(pane: &str, line: &str) -> SpawnSpec {
    let mut env = BTreeMap::new();
    for key in ["PATH", "SystemRoot", "COMSPEC", "TEMP", "TMP"] {
        if let Ok(value) = std::env::var(key) {
            env.insert(key.to_string(), value);
        }
    }
    SpawnSpec {
        pane_id: pane.into(),
        program: PathBuf::from("cmd.exe"),
        // /d skips AutoRun; /v:on gives `!var!` for values set on the line.
        args: vec!["/d".into(), "/v:on".into(), "/c".into(), line.into()],
        cwd: std::env::temp_dir(),
        env,
        cols: 80,
        rows: 24,
        scrollback_lines: 1000,
        mark_token: None,
    }
}

fn text(sink: &VecSink) -> String {
    String::from_utf8_lossy(&sink.data()).into_owned()
}

/// Every process whose parent chain reaches `root`.
fn descendants(root: u32) -> Vec<u32> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let parents: Vec<(u32, u32)> = sys
        .processes()
        .values()
        .filter_map(|p| Some((p.pid().as_u32(), p.parent()?.as_u32())))
        .collect();
    let mut out = Vec::new();
    let mut frontier = vec![root];
    while let Some(p) = frontier.pop() {
        for (child, _) in parents.iter().filter(|(_, pp)| *pp == p) {
            if !out.contains(child) {
                out.push(*child);
                frontier.push(*child);
            }
        }
    }
    out
}

fn alive(pid: u32) -> bool {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = System::new();
    let pid = Pid::from_u32(pid);
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    sys.process(pid).is_some()
}

#[test]
fn f_cd_03_conpty_streams_output_then_the_exit_code() {
    let mgr = PtyManager::new();
    let sink = Arc::new(VecSink::default());
    let info = mgr
        .spawn(
            cmd("w1", "echo hello-from-conpty& exit /b 3"),
            Some(sink.clone() as Arc<dyn OutputSink>),
        )
        .expect("spawn");
    assert!(info.pid > 0);
    assert!(
        wait_until(Duration::from_secs(10), || sink.has_exit()),
        "exit frame; got {:?}",
        text(&sink)
    );
    assert!(
        text(&sink).contains("hello-from-conpty"),
        "{:?}",
        text(&sink)
    );
    let frames = sink.frames.lock().expect("frames");
    let last = frames.last().expect("frames");
    assert_eq!(last.kind, FrameKind::Exit, "exit frame is last");
    let exit: serde_json::Value = serde_json::from_slice(&last.payload).expect("exit json");
    assert_eq!(exit["code"], 3);
}

#[test]
fn f_cd_03_conpty_input_reaches_the_process() {
    let mgr = PtyManager::new();
    let sink = Arc::new(VecSink::default());
    mgr.spawn(
        cmd("w2", "set /p L=& echo got:!L!"),
        Some(sink.clone() as Arc<dyn OutputSink>),
    )
    .expect("spawn");
    mgr.write("w2", b"ping\r").expect("write");
    assert!(
        wait_until(Duration::from_secs(10), || text(&sink).contains("got:ping")),
        "{:?}",
        text(&sink)
    );
    let replay = mgr
        .attach("w2", Arc::new(VecSink::default()))
        .expect("attach");
    assert!(String::from_utf8_lossy(&replay.replay).contains("got:ping"));
}

/// F-CD-03.4 on Windows: closing a pane ends its whole tree through the
/// pane's Job Object, including a child started with `start /b`.
#[test]
fn f_cd_03_4_kill_ends_the_whole_tree_through_the_job() {
    let mgr = PtyManager::new();
    let sink = Arc::new(VecSink::default());
    // The first ping is a pause, so the job holds cmd before it starts children.
    let info = mgr
        .spawn(
            cmd(
                "w3",
                "ping -n 2 127.0.0.1 >nul& start /b ping -n 300 127.0.0.1 >nul& echo started& ping -n 301 127.0.0.1 >nul",
            ),
            Some(sink.clone() as Arc<dyn OutputSink>),
        )
        .expect("spawn");
    assert!(
        wait_until(Duration::from_secs(15), || text(&sink).contains("started")),
        "{:?}",
        text(&sink)
    );
    let mut kids = Vec::new();
    assert!(
        wait_until(Duration::from_secs(5), || {
            kids = descendants(info.pid);
            kids.len() >= 2
        }),
        "both pings are descendants: {kids:?}"
    );
    mgr.kill("w3").expect("kill");
    assert!(
        wait_until(Duration::from_secs(8), || kids.iter().all(|k| !alive(*k))
            && !alive(info.pid)),
        "no process from the pane survives: {:?}",
        kids.iter().filter(|k| alive(**k)).collect::<Vec<_>>()
    );
    assert!(matches!(
        mgr.write("w3", b"x"),
        Err(tandem_pty::PtyError::NotFound)
    ));
}
