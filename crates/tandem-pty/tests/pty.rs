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
        mark_token: None,
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

#[test]
fn f_cd_03_5_a_hidden_pane_gets_da_answered_within_100ms() {
    // No sink: the pane isn't shown anywhere.
    let mgr = PtyManager::new();
    mgr.spawn(
        spec(
            "hidden",
            "stty raw -echo; t0=$(date +%s%N); printf '\\033[c'; r=$(dd bs=1 count=9 2>/dev/null | od -An -c); t1=$(date +%s%N); echo \"$(( (t1 - t0) / 1000000 )) $r\" > /tmp/tandem-da-$$; mv /tmp/tandem-da-$$ /tmp/tandem-da-done-$$; echo $$ > /tmp/tandem-da-pid",
        ),
        None,
    )
    .expect("spawn");
    let started = std::time::Instant::now();
    let reply = (|| loop {
        if let Ok(pid) = std::fs::read_to_string("/tmp/tandem-da-pid") {
            let path = format!("/tmp/tandem-da-done-{}", pid.trim());
            if let Ok(body) = std::fs::read_to_string(&path) {
                let _ = std::fs::remove_file(&path);
                let _ = std::fs::remove_file("/tmp/tandem-da-pid");
                return Some(body);
            }
        }
        if started.elapsed() > Duration::from_secs(5) {
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    })()
    .expect("the program got its answer");
    assert!(
        reply.contains('?') && reply.contains('c'),
        "DA1 reply: {reply}"
    );
    let ms: u64 = reply
        .split_whitespace()
        .next()
        .and_then(|v| v.parse().ok())
        .expect("elapsed");
    assert!(ms < 100, "answered in {ms} ms");
}

#[test]
fn f_cd_03_modes_reach_the_state_sink() {
    use std::sync::Mutex;
    use tandem_pty::PtyState;
    let seen: Arc<Mutex<Vec<PtyState>>> = Arc::default();
    let sink = seen.clone();
    let mgr = PtyManager::new().with_state_sink(Arc::new(move |s| {
        if let Ok(mut v) = sink.lock() {
            v.push(s);
        }
    }));
    mgr.spawn(
        spec("modes", "printf '\\033]2;build watch\\007\\033[?2004h'; sleep 0.3; printf '\\033[?2004l'; sleep 0.3"),
        None,
    )
    .expect("spawn");
    assert!(wait_until(Duration::from_secs(5), || {
        seen.lock()
            .map(|v| {
                v.iter()
                    .any(|s| s.bracketed_paste && s.title.as_deref() == Some("build watch"))
            })
            .unwrap_or(false)
    }));
    assert!(
        wait_until(Duration::from_secs(5), || {
            seen.lock()
                .map(|v| v.last().map(|s| !s.bracketed_paste).unwrap_or(false))
                .unwrap_or(false)
        }),
        "the last change is always sent"
    );
    let changes = seen.lock().map(|v| v.len()).unwrap_or(0);
    assert!(changes <= 3, "debounced: {changes} events");
}

#[test]
fn f_cd_03_8_children_are_recorded_until_they_exit() {
    use tandem_pty::orphans::{read_records, ChildRegistry, SystemTable};
    let dir = tempfile::tempdir().expect("tempdir");
    let registry = Arc::new(ChildRegistry::new(
        &dir.path().join("run"),
        Box::new(SystemTable::default()),
    ));
    let path = registry.path().to_path_buf();
    let mgr = PtyManager::new().with_registry(registry);
    let info = mgr.spawn(spec("tracked", "sleep 30"), None).expect("spawn");
    let records = read_records(&path);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].pid, info.pid);
    assert_eq!(records[0].exe, "sh");
    assert!(records[0].start_time > 0);
    mgr.kill("tracked").expect("kill");
    assert!(wait_until(Duration::from_secs(5), || read_records(&path).is_empty()));
    mgr.shutdown();
    assert!(!path.exists(), "a clean exit removes the registry file");
}
