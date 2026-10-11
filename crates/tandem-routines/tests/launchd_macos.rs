//! F-RT-03 on macOS (D-2109): the LaunchAgent names the app, survives
//! launchd's slow `bootout`, refuses an app that isn't installed, and the
//! Homebrew cask removes it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use tandem_routines::launcher::*;

fn spec(exe: &str) -> LauncherSpec {
    LauncherSpec {
        exe: PathBuf::from(exe),
        tandem_home: None,
        logs: PathBuf::from("/data/logs"),
    }
}

fn launcher(dir: &Path, runner: Arc<RecordingRunner>) -> LaunchdLauncher {
    LaunchdLauncher {
        runner,
        home: dir.to_path_buf(),
        uid: 501,
    }
}

const APP_EXE: &str = "/Applications/Vibezeo.app/Contents/MacOS/tandem";

#[test]
fn f_rt_03_1_the_launch_agent_is_attributed_to_the_app() {
    let body = LaunchdLauncher::plist(&spec(APP_EXE));
    assert!(
        body.contains(
            "<key>AssociatedBundleIdentifiers</key>\n  <string>com.vibezeo.tandem</string>"
        ),
        "{body}"
    );
    // The same id the app is bundled with.
    let conf: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../apps/desktop/src-tauri/tauri.conf.json"),
        )
        .expect("tauri.conf.json"),
    )
    .expect("json");
    assert_eq!(conf["identifier"], MAC_BUNDLE_ID);
}

#[test]
fn f_rt_03_1_bootstrap_is_retried_while_launchd_removes_the_old_job() {
    let dir = tempfile::tempdir().expect("dir");
    let failures = Arc::new(AtomicU32::new(1));
    let left = failures.clone();
    let runner = Arc::new(RecordingRunner::answering(move |c| {
        let bootstrap = c.args.first().map(String::as_str) == Some("bootstrap");
        let fail = bootstrap
            && left
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok();
        CommandOutput {
            success: !fail,
            stderr: if fail {
                "Bootstrap failed: 5: Input/output error".into()
            } else {
                String::new()
            },
            ..Default::default()
        }
    }));
    let l = launcher(dir.path(), runner.clone());
    let status = l
        .install(&spec(APP_EXE))
        .expect("installed on the second try");
    assert!(status.installed);
    let tries = runner
        .lines()
        .iter()
        .filter(|l| l.starts_with("launchctl bootstrap"))
        .count();
    assert_eq!(tries, 2);
    assert_eq!(failures.load(Ordering::SeqCst), 0);
}

#[test]
fn f_rt_03_1_a_translocated_or_mounted_app_is_refused_before_anything_is_written() {
    for (exe, place) in [
        (
            "/private/var/folders/x/T/AppTranslocation/0A1B/d/Vibezeo.app/Contents/MacOS/tandem",
            "a temporary copy",
        ),
        (
            "/Volumes/Vibezeo/Vibezeo.app/Contents/MacOS/tandem",
            "its disk image",
        ),
    ] {
        let dir = tempfile::tempdir().expect("dir");
        let runner = Arc::new(RecordingRunner::default());
        let l = launcher(dir.path(), runner.clone());
        let err = l.install(&spec(exe)).expect_err(exe).to_string();
        assert!(
            err.contains(place) && err.contains("Move Vibezeo to Applications"),
            "{err}"
        );
        assert!(runner.lines().is_empty(), "{exe}: no launchctl call");
        assert!(!l.status().installed);
    }
    // Installed copies, including on an external drive, are fine.
    for exe in [
        APP_EXE,
        "/Users/me/Applications/Vibezeo.app/Contents/MacOS/tandem",
        "/Volumes/External/Applications/Vibezeo.app/Contents/MacOS/tandem",
    ] {
        assert_eq!(transient_app_location(Path::new(exe)), None, "{exe}");
    }
}

#[test]
fn d_2107_the_homebrew_cask_removes_the_launch_agent() {
    let cask = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/macos/vibezeo.rb"),
    )
    .expect("cask");
    assert!(cask.contains(&format!("uninstall launchctl: \"{LAUNCHD_LABEL}\"")));
    assert!(cask.contains(&format!("~/Library/LaunchAgents/{LAUNCHD_LABEL}.plist")));
    assert!(cask.contains(&format!("~/Library/Application Support/{MAC_BUNDLE_ID}")));
    assert!(cask.contains("depends_on macos: \">= :sonoma\""));
    assert!(
        !cask
            .lines()
            .any(|l| !l.trim_start().starts_with('#') && l.contains(":no_check")),
        "every release pins the dmg's SHA-256"
    );
}
