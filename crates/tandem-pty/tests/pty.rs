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

/// Starts `shell` with the installed integration scripts and a token, in a
/// HOME with no startup files of its own.
fn integrated(
    shell: &str,
    pane: &str,
    home: &std::path::Path,
    scripts: &std::path::Path,
) -> Option<SpawnSpec> {
    use tandem_pty::integration::{launch, IntegratedShell};
    let program = PathBuf::from(shell);
    if !program.exists() {
        return None;
    }
    let kind = IntegratedShell::detect(&program)?;
    let l = launch(kind, scripts, None).expect("launch");
    let mut s = spec(pane, "");
    s.program = program;
    s.args = l.args;
    s.env
        .insert("HOME".into(), home.to_string_lossy().into_owned());
    s.env
        .insert(tandem_pty::env::TOKEN_VAR.into(), "tok-it".into());
    s.env.extend(l.env);
    s.cwd = home.to_path_buf();
    s.mark_token = Some("tok-it".into());
    Some(s)
}

#[test]
fn f_cd_10_shells_report_cwd_marks_and_last_output() {
    let home = tempfile::tempdir().expect("home");
    let data = tempfile::tempdir().expect("data");
    let scripts = tandem_pty::integration::install(data.path()).expect("install");
    let listing = tempfile::tempdir().expect("listing");
    for f in ["alpha.txt", "beta.txt"] {
        std::fs::write(listing.path().join(f), b"x").expect("file");
    }
    let marks: Arc<std::sync::Mutex<Vec<(String, tandem_pty::Mark)>>> = Arc::default();
    let seen = marks.clone();
    let mgr = PtyManager::new().with_mark_sink(Arc::new(move |pane, m| {
        seen.lock()
            .expect("marks")
            .push((pane.to_string(), m.clone()));
    }));
    let mut ran = 0;
    for shell in ["/bin/bash", "/usr/bin/zsh", "/usr/bin/fish"] {
        let pane = format!("si-{}", shell.rsplit('/').next().unwrap_or("x"));
        let Some(s) = integrated(shell, &pane, home.path(), &scripts) else {
            continue;
        };
        ran += 1;
        eprintln!("shell integration: {shell}");
        let sink = Arc::new(VecSink::default());
        mgr.spawn(s, Some(sink.clone() as Arc<dyn OutputSink>))
            .expect("spawn");
        let prompts = |n: usize| {
            let marks = marks.clone();
            let pane = pane.clone();
            move || {
                marks
                    .lock()
                    .map(|m| {
                        m.iter()
                            .filter(|(p, k)| *p == pane && *k == tandem_pty::Mark::PromptStart)
                            .count()
                            >= n
                    })
                    .unwrap_or(false)
            }
        };
        assert!(
            wait_until(Duration::from_secs(10), prompts(1)),
            "{shell}: first prompt"
        );
        // AC1: cd /tmp updates the cwd.
        mgr.write(&pane, b"cd /tmp\r").expect("write");
        assert!(
            wait_until(Duration::from_secs(5), || mgr
                .state(&pane)
                .and_then(|s| s.cwd)
                .as_deref()
                == Some("/tmp")),
            "{shell}: cwd is /tmp: {:?}",
            mgr.state(&pane)
        );
        // AC2: a program's own 133 mark isn't one.
        let before = marks.lock().map(|m| m.len()).unwrap_or(0);
        mgr.write(&pane, b"printf '\\033]133;A\\007spoof\\n'\r")
            .expect("write");
        assert!(wait_until(Duration::from_secs(5), || text(&sink).contains("spoof")));
        assert!(
            wait_until(Duration::from_secs(5), prompts(3)),
            "{shell}: prompt after printf"
        );
        let added: Vec<tandem_pty::Mark> = marks.lock().expect("marks")[before..]
            .iter()
            .filter(|(p, _)| *p == pane)
            .map(|(_, m)| m.clone())
            .collect();
        assert_eq!(
            added
                .iter()
                .filter(|m| **m == tandem_pty::Mark::PromptStart)
                .count(),
            1,
            "{shell}: only the shell's own prompt mark: {added:?}"
        );
        assert!(
            !text(&sink).contains("tok-it"),
            "{shell}: the token never reaches the view"
        );
        // AC3: the last command's output is exactly what it printed.
        let cmd = format!("ls -1 {}\r", listing.path().display());
        mgr.write(&pane, cmd.as_bytes()).expect("write");
        assert!(
            wait_until(Duration::from_secs(5), || mgr
                .last_output(&pane)
                .map(|o| o.0)
                == Some("alpha.txt\nbeta.txt".into())),
            "{shell}: last output {:?}",
            mgr.last_output(&pane)
        );
        assert_eq!(mgr.last_output(&pane).and_then(|o| o.1), Some(0));
        // The shell's children don't inherit the token.
        mgr.write(&pane, b"env | grep -c TANDEM_SHELL_TOKEN\r")
            .expect("write");
        assert!(
            wait_until(Duration::from_secs(5), || mgr
                .last_output(&pane)
                .map(|o| o.0)
                == Some("0".into())),
            "{shell}: token in env: {:?}",
            mgr.last_output(&pane)
        );
        mgr.kill(&pane).expect("kill");
    }
    assert!(ran >= 1, "bash at least");
}

/// F-CD-04 AC5: a background pane (no view attached, so nothing holds the
/// reader back) stays within F-CD-03's bound, `lines × 200 B + 2 MB`, after
/// 1 GB of output. Slow: `cargo test -p tandem-pty --release -- --include-ignored f_cd_04_5`.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "streams 1 GB of output"]
fn f_cd_04_5_background_pane_memory_stays_bounded_after_1_gb() {
    let rss = || -> usize {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("VmRSS:"))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|kb| kb.parse::<usize>().ok())
            })
            .unwrap_or(0)
            * 1024
    };
    let lines = 10_000;
    let mut s = spec(
        "bg",
        "head -c 1073741824 /dev/zero | tr '\\0' 'x' | fold -w 100",
    );
    s.scrollback_lines = lines;
    let mgr = PtyManager::new();
    let before = rss();
    mgr.spawn(s, None).expect("spawn");
    assert!(
        wait_until(Duration::from_secs(900), || mgr.exit_info("bg").is_some()),
        "1 GB never finished"
    );
    let bound = lines * 200 + 2 * 1024 * 1024;
    let ring = mgr.scrollback("bg").expect("ring").len();
    assert!(ring <= bound, "ring holds {ring} bytes, bound {bound}");
    // Allocator and thread overhead aside, the process grew by at most the bound.
    let grown = rss().saturating_sub(before);
    assert!(
        grown <= bound + 16 * 1024 * 1024,
        "the process grew by {grown} bytes streaming 1 GB (bound {bound})"
    );
}

/// Ignores SIGHUP and SIGTERM: only SIGKILL ends it.
const STUBBORN: &str = "trap '' HUP TERM; echo ready; while :; do sleep 0.1; done";

#[test]
fn f_sh_07_quit_force_kills_what_outlives_the_deadline_in_parallel() {
    let mgr = PtyManager::new();
    let mut pids = Vec::new();
    for i in 0..3 {
        let sink = Arc::new(VecSink::default());
        let info = mgr
            .spawn(spec(&format!("q{i}"), STUBBORN), Some(sink.clone()))
            .expect("spawn");
        assert!(wait_until(Duration::from_secs(5), || text(&sink).contains("ready")));
        pids.push(info.pid);
    }
    let started = std::time::Instant::now();
    mgr.shutdown_within(Duration::from_millis(1_200));
    let took = started.elapsed();
    // Parallel, and bounded by the deadline (not 3 × 1.2 s, never the full
    // HUP→TERM→KILL ladder).
    assert!(took < Duration::from_millis(2_000), "took {took:?}");
    for pid in pids {
        let gone = wait_until(Duration::from_secs(2), || {
            // SAFETY: signal 0 only checks that the pid exists.
            let alive = unsafe { libc::kill(pid as i32, 0) } == 0;
            let zombie = std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .map(|s| {
                    s.rsplit(')')
                        .next()
                        .is_some_and(|r| r.trim_start().starts_with('Z'))
                })
                .unwrap_or(true);
            !alive || zombie
        });
        assert!(gone, "pid {pid} survived the quit");
    }
    assert_eq!(mgr.live_count(), 0);
}

#[test]
fn f_sh_07_quit_ends_cooperative_shells_without_waiting_for_the_deadline() {
    let mgr = PtyManager::new();
    for i in 0..4 {
        mgr.spawn(
            spec(&format!("c{i}"), "echo up; sleep 30"),
            Some(Arc::new(VecSink::default())),
        )
        .expect("spawn");
    }
    let started = std::time::Instant::now();
    mgr.shutdown_within(Duration::from_secs(5));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "took {:?}",
        started.elapsed()
    );
    assert_eq!(mgr.live_count(), 0);
}

#[test]
fn f_ex_02_1_shells_report_each_command_line_with_its_exit_and_folder() {
    let home = tempfile::tempdir().expect("home");
    let data = tempfile::tempdir().expect("data");
    let scripts = tandem_pty::integration::install(data.path()).expect("install");
    let marks: Arc<std::sync::Mutex<Vec<(String, tandem_pty::Mark)>>> = Arc::default();
    let seen = marks.clone();
    let mgr = PtyManager::new().with_mark_sink(Arc::new(move |pane, m| {
        seen.lock()
            .expect("marks")
            .push((pane.to_string(), m.clone()));
    }));
    let mut ran_shells = 0;
    for shell in ["/bin/bash", "/usr/bin/zsh", "/usr/bin/fish"] {
        let pane = format!("recall-{}", shell.rsplit('/').next().unwrap_or("x"));
        let Some(mut s) = integrated(shell, &pane, home.path(), &scripts) else {
            continue;
        };
        ran_shells += 1;
        // No HISTCONTROL: bash keeps the space-prefixed line in its own
        // history, and the integration still leaves it out.
        s.env.remove("HISTCONTROL");
        s.env.insert(
            "HISTFILE".into(),
            home.path().join(".hist").to_string_lossy().into_owned(),
        );
        let sink = Arc::new(VecSink::default());
        mgr.spawn(s, Some(sink.clone() as Arc<dyn OutputSink>))
            .expect("spawn");
        let ran = |marks: &Arc<std::sync::Mutex<Vec<(String, tandem_pty::Mark)>>>,
                   pane: &str|
         -> Vec<tandem_pty::Ran> {
            marks
                .lock()
                .map(|m| {
                    m.iter()
                        .filter(|(p, _)| p == pane)
                        .filter_map(|(_, k)| {
                            if let tandem_pty::Mark::Ran(r) = k {
                                Some(r.clone())
                            } else {
                                None
                            }
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        let prompt_count = |marks: &Arc<std::sync::Mutex<Vec<(String, tandem_pty::Mark)>>>,
                            pane: &str| {
            marks
                .lock()
                .map(|m| {
                    m.iter()
                        .filter(|(p, k)| p == pane && *k == tandem_pty::Mark::PromptStart)
                        .count()
                })
                .unwrap_or(0)
        };
        assert!(
            wait_until(Duration::from_secs(10), || prompt_count(&marks, &pane) >= 1),
            "{shell}: first prompt"
        );
        for (i, line) in ["echo one", "false", " echo hidden"].iter().enumerate() {
            mgr.write(&pane, format!("{line}\r").as_bytes())
                .expect("write");
            assert!(
                wait_until(Duration::from_secs(5), || prompt_count(&marks, &pane)
                    >= i + 2),
                "{shell}: prompt after {line}"
            );
        }
        let got = ran(&marks, &pane);
        assert_eq!(
            got.iter()
                .map(|r| (r.command.as_str(), r.exit))
                .collect::<Vec<_>>(),
            [("echo one", Some(0)), ("false", Some(1))],
            "{shell}"
        );
        let home_canon = std::fs::canonicalize(home.path()).expect("canon");
        for r in &got {
            assert_eq!(
                r.cwd.as_deref().map(std::path::Path::new),
                Some(home_canon.as_path()),
                "{shell}"
            );
            assert!(r.duration_ms.is_some() && r.started_at > 0, "{shell}");
        }
        // The command line never reaches the view.
        assert!(!text(&sink).contains("cmdline_url"), "{shell}");
        mgr.kill(&pane).expect("kill");
    }
    assert!(ran_shells >= 1);
}
