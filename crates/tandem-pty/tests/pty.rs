//! Real-PTY tests (unix). Windows ConPTY gets its own suite in M3.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tandem_pty::{wait_until, FrameKind, OutputSink, PtyManager, SpawnSpec, TermColors, VecSink};

fn spec(pane: &str, script: &str) -> SpawnSpec {
    let mut env = BTreeMap::new();
    env.insert(
        "PATH".into(),
        std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
    );
    env.insert("TERM".into(), "xterm-256color".into());
    SpawnSpec {
        pane_id: pane.into(),
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".into(), script.into()],
        cwd: std::env::temp_dir(),
        env,
        cols: 80,
        rows: 24,
        scrollback_lines: 1000,
    }
}

fn text(sink: &VecSink) -> String {
    String::from_utf8_lossy(&sink.data()).into_owned()
}

#[test]
fn f_cd_03_spawn_streams_output_then_exit_frame() {
    let mgr = PtyManager::new();
    let sink = Arc::new(VecSink::default());
    let info = mgr
        .spawn(
            spec("p1", "echo hello-from-pty; exit 3"),
            Some(sink.clone() as Arc<dyn OutputSink>),
        )
        .expect("spawn");
    assert!(info.pid > 0);
    assert!(
        wait_until(Duration::from_secs(5), || sink.has_exit()),
        "exit frame"
    );
    assert!(text(&sink).contains("hello-from-pty"));
    let frames = sink.frames.lock().expect("frames");
    let last = frames.last().expect("frames");
    assert_eq!(last.kind, FrameKind::Exit, "exit frame is last");
    let exit: serde_json::Value = serde_json::from_slice(&last.payload).expect("exit json");
    assert_eq!(exit["code"], 3);
    let seqs: Vec<u64> = frames.iter().map(|f| f.seq).collect();
    assert!(
        seqs.windows(2).all(|w| w[1] == w[0] + 1),
        "sequence numbers are contiguous"
    );
}

#[test]
fn f_cd_03_write_reaches_the_process_and_ring_replays() {
    let mgr = PtyManager::new();
    let sink = Arc::new(VecSink::default());
    mgr.spawn(
        spec("p2", "read line; echo got:$line"),
        Some(sink.clone() as Arc<dyn OutputSink>),
    )
    .expect("spawn");
    mgr.write("p2", b"ping\r").expect("write");
    assert!(wait_until(Duration::from_secs(5), || text(&sink).contains("got:ping")));
    let replay = mgr
        .attach("p2", Arc::new(VecSink::default()))
        .expect("attach");
    assert!(
        String::from_utf8_lossy(&replay).contains("got:ping"),
        "ring keeps output for reattach"
    );
}

#[test]
fn f_cd_03_4_kill_ends_the_whole_tree() {
    let mgr = PtyManager::new();
    let sink = Arc::new(VecSink::default());
    let info = mgr
        .spawn(
            spec(
                "p3",
                "trap '' HUP; sleep 300 & setsid sleep 301 & echo started; wait",
            ),
            Some(sink.clone() as Arc<dyn OutputSink>),
        )
        .expect("spawn");
    assert!(wait_until(Duration::from_secs(5), || text(&sink).contains("started")));
    let kids = tandem_pty::descendants(info.pid);
    assert!(kids.len() >= 2, "both sleeps are descendants: {kids:?}");
    mgr.kill("p3").expect("kill");
    let alive =
        |pid: u32| std::path::Path::new(&format!("/proc/{pid}")).exists() && !is_zombie(pid);
    assert!(
        wait_until(Duration::from_secs(8), || kids.iter().all(|k| !alive(*k))
            && !alive(info.pid)),
        "no process from the pane survives"
    );
    assert!(matches!(
        mgr.write("p3", b"x"),
        Err(tandem_pty::PtyError::NotFound)
    ));
}

fn is_zombie(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| {
            s.rfind(')')
                .map(|i| s[i + 1..].trim_start().starts_with('Z'))
        })
        .unwrap_or(true)
}

#[test]
fn f_sh_05_2_osc_11_is_answered_with_the_theme_background() {
    let mgr = PtyManager::new();
    mgr.set_theme_colors(TermColors {
        fg: "#1e293b".into(),
        bg: "#f8fafc".into(),
        cursor: "#0d9488".into(),
    });
    let sink = Arc::new(VecSink::default());
    // Raw mode so the reply isn't echoed or line-buffered; print it with ESC visible.
    let script =
        r#"stty raw -echo; printf '\033]11;?\033\\'; head -c 25 | tr '\033' 'E'; stty sane"#;
    mgr.spawn(
        spec("p4", script),
        Some(sink.clone() as Arc<dyn OutputSink>),
    )
    .expect("spawn");
    assert!(wait_until(Duration::from_secs(5), || sink.has_exit()));
    assert!(
        text(&sink).contains("E]11;rgb:f8f8/fafa/fcfc"),
        "got {:?}",
        text(&sink)
    );
}

#[test]
fn f_cd_04_batches_large_output_into_few_frames() {
    let mgr = PtyManager::new();
    let sink = Arc::new(VecSink::default());
    mgr.spawn(
        spec(
            "p5",
            "i=0; while [ $i -lt 2000 ]; do echo line-$i; i=$((i+1)); done",
        ),
        Some(sink.clone() as Arc<dyn OutputSink>),
    )
    .expect("spawn");
    assert!(wait_until(Duration::from_secs(10), || sink.has_exit()));
    assert!(text(&sink).contains("line-1999"));
    let frames = sink.frames.lock().expect("frames").len();
    assert!(
        frames < 400,
        "2000 lines arrive in batches, not per line ({frames} frames)"
    );
}

#[test]
fn resize_and_unknown_pane() {
    let mgr = PtyManager::new();
    let sink = Arc::new(VecSink::default());
    mgr.spawn(
        spec("p6", "sleep 0.3; stty size"),
        Some(sink.clone() as Arc<dyn OutputSink>),
    )
    .expect("spawn");
    mgr.resize("p6", 100, 30).expect("resize");
    assert!(wait_until(Duration::from_secs(5), || sink.has_exit()));
    assert!(text(&sink).contains("30 100"), "got {:?}", text(&sink));
    assert!(matches!(
        mgr.resize("nope", 1, 1),
        Err(tandem_pty::PtyError::NotFound)
    ));
}
