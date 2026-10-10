//! F-RT-03: the OS launchers, each with a recording command runner.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tandem_routines::launcher::*;

fn spec(home: Option<&str>) -> LauncherSpec {
    LauncherSpec {
        exe: PathBuf::from("/opt/Vibezeo/tandem"),
        tandem_home: home.map(PathBuf::from),
        logs: PathBuf::from("/data/logs"),
    }
}

#[test]
fn f_rt_03_1_launchd_writes_the_plist_and_bootstraps_it_then_removes_it() {
    let dir = tempfile::tempdir().expect("dir");
    let runner = Arc::new(RecordingRunner::default());
    let l = LaunchdLauncher {
        runner: runner.clone(),
        home: dir.path().to_path_buf(),
        uid: 501,
    };
    assert!(!l.status().installed);
    let status = l.install(&spec(Some("/tmp/h"))).expect("install");
    assert!(status.installed);
    assert_eq!(status.kind, LauncherKind::Launchd);
    let plist = dir
        .path()
        .join("Library/LaunchAgents/app.tandem.routines.plist");
    assert_eq!(
        status.path.as_deref(),
        Some(plist.to_string_lossy().as_ref())
    );
    let body = std::fs::read_to_string(&plist).expect("plist");
    for needle in [
        "<string>app.tandem.routines</string>",
        "<string>/opt/Vibezeo/tandem</string>",
        "<string>--run-routines</string>",
        "<integer>300</integer>",
        "<key>RunAtLoad</key>\n  <false/>",
        "<string>Background</string>",
        "<string>/data/logs/routines-launchd.log</string>",
        "<key>TANDEM_HOME</key>\n    <string>/tmp/h</string>",
    ] {
        assert!(body.contains(needle), "plist has {needle}:\n{body}");
    }
    let lines = runner.lines();
    assert_eq!(lines[0], "launchctl bootout gui/501/app.tandem.routines");
    assert_eq!(
        lines[1],
        format!("launchctl bootstrap gui/501 {}", plist.display())
    );
    l.uninstall().expect("uninstall");
    assert!(!plist.exists());
    assert!(!l.status().installed);
    assert_eq!(
        runner.lines().last().map(String::as_str),
        Some("launchctl bootout gui/501/app.tandem.routines")
    );
}

#[test]
fn f_rt_03_1_launchd_failure_leaves_no_plist() {
    let dir = tempfile::tempdir().expect("dir");
    let runner = Arc::new(RecordingRunner::answering(|c| CommandOutput {
        success: c.args.first().map(String::as_str) != Some("bootstrap"),
        stderr: "Bootstrap failed: 5".into(),
        ..Default::default()
    }));
    let l = LaunchdLauncher {
        runner,
        home: dir.path().to_path_buf(),
        uid: 501,
    };
    let err = l.install(&spec(None)).expect_err("bootstrap failed");
    assert!(err.to_string().contains("Bootstrap failed"));
    assert!(!l.status().installed);
}

#[test]
fn f_rt_03_1_task_scheduler_creates_and_deletes_the_task() {
    let installed = Arc::new(Mutex::new(false));
    let state = installed.clone();
    let runner = Arc::new(RecordingRunner::answering(move |c| {
        let mut on = state.lock().expect("lock");
        let ok = match c.args.first().map(String::as_str) {
            Some("/Create") => {
                *on = true;
                true
            }
            Some("/Delete") => {
                *on = false;
                true
            }
            Some("/Query") => *on,
            _ => false,
        };
        CommandOutput {
            success: ok,
            ..Default::default()
        }
    }));
    let l = TaskSchedulerLauncher {
        runner: runner.clone(),
    };
    assert!(!l.status().installed);
    let status = l.install(&spec(None)).expect("install");
    assert!(status.installed);
    assert_eq!(status.kind, LauncherKind::TaskScheduler);
    let create = runner
        .calls()
        .into_iter()
        .find(|c| c.args.first().map(String::as_str) == Some("/Create"))
        .expect("create");
    assert_eq!(
        create.args,
        [
            "/Create",
            "/F",
            "/TN",
            "Tandem\\Routines",
            "/SC",
            "MINUTE",
            "/MO",
            "5",
            "/TR",
            "\"/opt/Vibezeo/tandem\" --run-routines"
        ]
    );
    assert_eq!(
        TaskSchedulerLauncher::command_line(&spec(Some("C:\\T"))),
        "cmd /c \"set \"TANDEM_HOME=C:\\T\" && \"/opt/Vibezeo/tandem\" --run-routines\""
    );
    l.uninstall().expect("uninstall");
    assert!(!l.status().installed);
    assert!(runner
        .lines()
        .contains(&"schtasks /Delete /F /TN Tandem\\Routines".to_string()));
}

#[test]
fn f_rt_03_1_systemd_writes_the_timer_and_service_then_removes_them() {
    let dir = tempfile::tempdir().expect("dir");
    let runner = Arc::new(RecordingRunner::answering(|c| CommandOutput {
        // No crontab: `crontab -l` fails.
        success: c.program != "crontab",
        ..Default::default()
    }));
    let l = SystemdUserLauncher::new(runner.clone(), dir.path());
    let status = l.install(&spec(Some("/tmp/h"))).expect("install");
    assert!(status.installed);
    assert_eq!(status.kind, LauncherKind::Systemd);
    let units = dir.path().join(".config/systemd/user");
    let service = std::fs::read_to_string(units.join("tandem-routines.service")).expect("service");
    let timer = std::fs::read_to_string(units.join("tandem-routines.timer")).expect("timer");
    assert!(service.contains("Type=oneshot"));
    assert!(service.contains("ExecStart=\"/opt/Vibezeo/tandem\" --run-routines"));
    assert!(service.contains("Environment=\"TANDEM_HOME=/tmp/h\""));
    for needle in [
        "OnBootSec=2min",
        "OnUnitActiveSec=5min",
        "Persistent=true",
        "WantedBy=timers.target",
    ] {
        assert!(timer.contains(needle), "{needle}");
    }
    let lines = runner.lines();
    assert!(lines.contains(&"systemctl --user daemon-reload".to_string()));
    assert!(lines.contains(&"systemctl --user enable --now tandem-routines.timer".to_string()));
    l.uninstall().expect("uninstall");
    assert!(!units.join("tandem-routines.timer").exists());
    assert!(!units.join("tandem-routines.service").exists());
    assert!(runner
        .lines()
        .contains(&"systemctl --user disable --now tandem-routines.timer".to_string()));
    assert!(!l.status().installed);
}

/// A crontab kept in memory by the fake runner.
fn cron_runner(
    start: &str,
    systemd_works: bool,
) -> (Arc<RecordingRunner>, Arc<Mutex<Option<String>>>) {
    let tab = Arc::new(Mutex::new((!start.is_empty()).then(|| start.to_string())));
    let state = tab.clone();
    let runner = Arc::new(RecordingRunner::answering(move |c| {
        let mut tab = state.lock().expect("lock");
        match (c.program.as_str(), c.args.first().map(String::as_str)) {
            ("systemctl", _) => CommandOutput {
                success: systemd_works,
                stderr: "Failed to connect to bus: No medium found".into(),
                ..Default::default()
            },
            ("crontab", Some("-l")) => match tab.as_ref() {
                Some(t) => CommandOutput {
                    success: true,
                    stdout: t.clone(),
                    ..Default::default()
                },
                None => CommandOutput {
                    success: false,
                    stderr: "no crontab for user".into(),
                    ..Default::default()
                },
            },
            ("crontab", Some("-")) => {
                *tab = c.stdin.clone();
                CommandOutput {
                    success: true,
                    ..Default::default()
                }
            }
            _ => CommandOutput::default(),
        }
    }));
    (runner, tab)
}

#[test]
fn f_rt_03_1_without_systemd_a_tagged_crontab_line_is_used_and_removed() {
    let dir = tempfile::tempdir().expect("dir");
    let (runner, tab) = cron_runner("0 3 * * * backup.sh\n", false);
    let l = SystemdUserLauncher::new(runner, dir.path());
    let status = l.install(&spec(None)).expect("install");
    assert!(status.installed);
    assert_eq!(status.kind, LauncherKind::Cron);
    assert!(!dir
        .path()
        .join(".config/systemd/user/tandem-routines.timer")
        .exists());
    let now = tab.lock().expect("lock").clone().expect("crontab");
    assert_eq!(
        now,
        "0 3 * * * backup.sh\n*/5 * * * * '/opt/Vibezeo/tandem' --run-routines # tandem-routines\n"
    );
    // Installing again doesn't add a second line.
    l.install(&spec(Some("/tmp/it's"))).expect("again");
    let now = tab.lock().expect("lock").clone().expect("crontab");
    assert_eq!(now.matches(CRON_TAG).count(), 1);
    assert!(now.contains("TANDEM_HOME='/tmp/it'\\''s' '/opt/Vibezeo/tandem'"));
    l.uninstall().expect("uninstall");
    assert_eq!(
        tab.lock().expect("lock").clone().as_deref(),
        Some("0 3 * * * backup.sh\n"),
        "the user's own lines stay"
    );
    assert!(!l.status().installed);
}

#[test]
fn f_rt_03_1_uninstalling_with_nothing_installed_is_fine() {
    let dir = tempfile::tempdir().expect("dir");
    let (runner, tab) = cron_runner("", true);
    let l = SystemdUserLauncher::new(runner.clone(), dir.path());
    l.uninstall().expect("nothing to remove");
    assert!(
        tab.lock().expect("lock").is_none(),
        "no empty crontab written"
    );
    assert!(!runner.lines().iter().any(|l| l.starts_with("systemctl")));
}

#[test]
fn f_rt_03_1_platform_launcher_picks_this_os() {
    let dir = tempfile::tempdir().expect("dir");
    let l = platform_launcher(Arc::new(RecordingRunner::default()), dir.path());
    let want = if cfg!(target_os = "macos") {
        LauncherKind::Launchd
    } else if cfg!(windows) {
        LauncherKind::TaskScheduler
    } else {
        LauncherKind::Systemd
    };
    assert_eq!(l.kind(), want);
}
