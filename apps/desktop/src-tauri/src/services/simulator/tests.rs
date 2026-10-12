use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

const PHONE: &str = "8A3F1C2E-1111-4A6B-9C1D-0123456789AB";
const PAD: &str = "5B2E9D10-2222-4C3D-8E9F-0123456789AB";
const WATCH: &str = "7C1D8E20-3333-4D4E-9F00-0123456789AB";

/// What `xcrun simctl list --json devices runtimes` prints on a Mac with
/// iOS 18 and 17 and a watchOS runtime (trimmed to the fields read).
fn list_json(phone_state: &str) -> String {
    serde_json::json!({
        "runtimes": [
            { "identifier": "com.apple.CoreSimulator.SimRuntime.iOS-17-5", "name": "iOS 17.5", "version": "17.5", "platform": "iOS", "isAvailable": true },
            { "identifier": "com.apple.CoreSimulator.SimRuntime.watchOS-11-0", "name": "watchOS 11.0", "version": "11.0", "platform": "watchOS", "isAvailable": true },
            { "identifier": "com.apple.CoreSimulator.SimRuntime.iOS-18-0", "name": "iOS 18.0", "version": "18.0", "platform": "iOS", "isAvailable": true },
            { "identifier": "com.apple.CoreSimulator.SimRuntime.iOS-16-0", "name": "iOS 16.0", "version": "16.0", "platform": "iOS", "isAvailable": false }
        ],
        "devices": {
            "com.apple.CoreSimulator.SimRuntime.iOS-18-0": [
                { "udid": PAD, "name": "iPad Air 11-inch (M2)", "state": "Shutdown", "isAvailable": true },
                { "udid": PHONE, "name": "iPhone 16", "state": phone_state, "isAvailable": true },
                { "udid": "00000000-0000-0000-0000-000000000000", "name": "iPhone Gone", "state": "Shutdown", "isAvailable": false }
            ],
            "com.apple.CoreSimulator.SimRuntime.iOS-17-5": [
                { "udid": "11111111-0000-0000-0000-000000000000", "name": "iPhone 15", "state": "Shutdown", "isAvailable": true }
            ],
            "com.apple.CoreSimulator.SimRuntime.watchOS-11-0": [
                { "udid": WATCH, "name": "Apple Watch Series 10", "state": "Shutdown", "isAvailable": true }
            ]
        }
    })
    .to_string()
}

type Respond = Box<dyn Fn(&[String]) -> Result<Vec<u8>, SimError> + Send + Sync>;

struct FakeHost {
    calls: Mutex<Vec<Vec<String>>>,
    idb_calls: Mutex<Vec<Vec<String>>>,
    respond: Respond,
    idb: bool,
    scratch: tempfile::TempDir,
    shots: AtomicUsize,
}

impl FakeHost {
    fn new(idb: bool, respond: Respond) -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(vec![]),
            idb_calls: Mutex::new(vec![]),
            respond,
            idb,
            scratch: tempfile::tempdir().expect("tempdir"),
            shots: AtomicUsize::new(0),
        })
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().expect("lock").clone()
    }

    fn idb_calls(&self) -> Vec<Vec<String>> {
        self.idb_calls.lock().expect("lock").clone()
    }
}

#[async_trait]
impl SimHost for FakeHost {
    async fn simctl(&self, args: &[String], _timeout: Duration) -> Result<Vec<u8>, SimError> {
        self.calls.lock().expect("lock").push(args.to_vec());
        if args.first().map(String::as_str) == Some("io") {
            // Writes a PNG-ish frame to the path it was given; the content
            // changes every third shot so unchanged frames can be skipped.
            let n = self.shots.fetch_add(1, Ordering::SeqCst);
            let path = args.last().cloned().unwrap_or_default();
            std::fs::write(&path, format!("frame-{}", n / 3))
                .map_err(|e| SimError::Failed(e.to_string()))?;
            return Ok(vec![]);
        }
        (self.respond)(args)
    }

    async fn idb(&self, args: &[String]) -> Result<Vec<u8>, SimError> {
        if !self.idb {
            return Err(SimError::NoIdb);
        }
        self.idb_calls.lock().expect("lock").push(args.to_vec());
        if args.first().map(String::as_str) == Some("describe") {
            return Ok(br#"{"screen_dimensions":{"width":1179,"height":2556,"density":3.0,"width_points":393,"height_points":852}}"#.to_vec());
        }
        Ok(vec![])
    }

    fn has_idb(&self) -> bool {
        self.idb
    }

    fn scratch_dir(&self) -> PathBuf {
        self.scratch.path().to_path_buf()
    }
}

fn mac(phone_state: &'static str) -> Respond {
    Box::new(
        move |args: &[String]| match args.first().map(String::as_str) {
            Some("list") => Ok(list_json(phone_state).into_bytes()),
            _ => Ok(vec![]),
        },
    )
}

fn service(host: &Arc<FakeHost>) -> Arc<SimulatorService> {
    Arc::new(SimulatorService::new(host.clone()))
}

#[test]
fn f_cd_21_the_device_list_keeps_available_ios_devices_booted_first_newest_runtime_first() {
    let (runtimes, devices) = parse_list(list_json("Booted").as_bytes()).expect("parses");
    assert_eq!(
        runtimes.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["iOS 18.0", "iOS 17.5"],
        "watchOS and unavailable runtimes are left out, newest first"
    );
    let names: Vec<(&str, &str, SimDeviceState)> = devices
        .iter()
        .map(|d| (d.name.as_str(), d.runtime.as_str(), d.state))
        .collect();
    assert_eq!(
        names,
        [
            ("iPhone 16", "iOS 18.0", SimDeviceState::Booted),
            (
                "iPad Air 11-inch (M2)",
                "iOS 18.0",
                SimDeviceState::Shutdown
            ),
            ("iPhone 15", "iOS 17.5", SimDeviceState::Shutdown),
        ]
    );
    assert!(parse_list(b"not json").is_err());
}

#[tokio::test]
async fn f_cd_21_2_without_xcode_or_an_ios_runtime_the_simulator_is_not_offered() {
    let no_xcode = FakeHost::new(true, Box::new(|_: &[String]| Err(SimError::NoXcode)));
    let status = service(&no_xcode).status().await;
    assert!(!status.available);
    assert_eq!(
        status.reason.as_deref(),
        Some("Xcode's command-line tools aren't installed.")
    );

    let watch_only = FakeHost::new(
        true,
        Box::new(|_: &[String]| {
            Ok(serde_json::json!({
            "runtimes": [{ "identifier": "com.apple.CoreSimulator.SimRuntime.watchOS-11-0", "name": "watchOS 11.0", "version": "11.0", "platform": "watchOS", "isAvailable": true }],
            "devices": {}
        })
        .to_string()
        .into_bytes())
        }),
    );
    let status = service(&watch_only).status().await;
    assert!(!status.available);
    assert_eq!(
        status.reason.as_deref(),
        Some("No iOS Simulator runtime is installed.")
    );
    assert!(status.devices.is_empty());

    let ok = FakeHost::new(false, mac("Shutdown"));
    let status = service(&ok).status().await;
    assert!(status.available);
    assert_eq!(status.devices.len(), 3);
    assert!(!status.input, "no idb: shown, but clicks aren't forwarded");
}

#[test]
fn f_cd_21_xcrun_failures_read_as_missing_tools_or_keep_their_first_line() {
    assert_eq!(
        classify_failure(
            "xcrun: error: unable to find utility \"simctl\", not a developer tool or in PATH\n"
        ),
        SimError::NoXcode
    );
    assert_eq!(
        classify_failure(
            "xcrun: error: invalid active developer path (/Library/Developer/CommandLineTools)"
        ),
        SimError::NoXcode
    );
    assert_eq!(
        classify_failure("\nAn error was encountered processing the command (domain=com.apple.CoreSimulator.SimError, code=405):\nUnable to boot device in current state: Booted\n"),
        SimError::Failed("An error was encountered processing the command (domain=com.apple.CoreSimulator.SimError, code=405):".into())
    );
    let err = SimError::NoXcode.into_ipc();
    assert_eq!(err.code, ErrorCode::Unavailable);
    assert!(err
        .hint
        .as_deref()
        .unwrap_or("")
        .contains("xcode-select --install"));
}

#[tokio::test]
async fn f_cd_21_boot_waits_for_booted_and_a_booted_device_needs_nothing() {
    let host = FakeHost::new(true, mac("Shutdown"));
    let sim = service(&host);
    let booted = sim.boot(PHONE).await.expect("boots");
    assert_eq!(booted.state, SimDeviceState::Booted);
    assert_eq!(
        host.calls().last(),
        Some(&vec![
            "bootstatus".to_string(),
            PHONE.to_string(),
            "-b".to_string()
        ])
    );

    let host = FakeHost::new(true, mac("Booted"));
    service(&host).boot(PHONE).await.expect("already booted");
    assert!(
        host.calls().iter().all(|c| c[0] != "bootstatus"),
        "nothing to boot"
    );

    // "Unable to boot device in current state: Booted" (a race) is success.
    let racing = FakeHost::new(
        true,
        Box::new(|args: &[String]| match args[0].as_str() {
            "list" => Ok(list_json("Shutdown").into_bytes()),
            _ => Err(SimError::Failed(
                "Unable to boot device in current state: Booted".into(),
            )),
        }),
    );
    service(&racing)
        .boot(PHONE)
        .await
        .expect("a race to boot is fine");
}

#[tokio::test]
async fn f_cd_21_shutdown_and_install_check_their_device_and_bundle() {
    let host = FakeHost::new(
        true,
        Box::new(|args: &[String]| match args[0].as_str() {
            "list" => Ok(list_json("Booted").into_bytes()),
            "shutdown" => Err(SimError::Failed(
                "Unable to shutdown device in current state: Shutdown".into(),
            )),
            _ => Ok(vec![]),
        }),
    );
    let sim = service(&host);
    assert_eq!(
        sim.shutdown(PHONE).await.expect("shut").state,
        SimDeviceState::Shutdown
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let app = dir.path().join("Demo.app");
    std::fs::create_dir(&app).expect("bundle");
    sim.install(PHONE, &app).await.expect("installs");
    assert_eq!(host.calls().last().map(|c| c[0].as_str()), Some("install"));
    let not_app = dir.path().join("Demo.ipa");
    std::fs::write(&not_app, b"x").expect("file");
    assert_eq!(
        sim.install(PHONE, &not_app)
            .await
            .expect_err("not .app")
            .code,
        ErrorCode::Invalid
    );
    // The iPad isn't booted.
    assert_eq!(
        sim.install(PAD, &app)
            .await
            .expect_err("not booted")
            .message,
        "Boot the device first."
    );
}

#[tokio::test]
async fn f_cd_21_a_malformed_udid_never_reaches_a_process() {
    let host = FakeHost::new(true, mac("Booted"));
    let sim = service(&host);
    for bad in [
        "booted",
        "../../etc",
        "8A3F1C2E-1111-4A6B-9C1D-0123456789A;",
        "",
    ] {
        assert_eq!(
            sim.boot(bad).await.expect_err("refused").code,
            ErrorCode::Invalid
        );
        assert_eq!(
            sim.tap(bad, 1.0, 1.0).await.expect_err("refused").code,
            ErrorCode::Invalid
        );
        assert!(sim.stream_start("pane", bad, Box::new(|_| true)).is_err());
    }
    assert!(host.calls().is_empty());
    assert!(host.idb_calls().is_empty());
    assert!(valid_udid(PHONE));
}

#[tokio::test]
async fn f_cd_21_input_is_forwarded_through_idb_in_points_and_refused_without_it() {
    let host = FakeHost::new(true, mac("Booted"));
    let sim = service(&host);
    sim.tap(PHONE, 196.4, 400.6).await.expect("taps");
    sim.tap(PHONE, 10.0, 20.0).await.expect("taps again");
    let calls = host.idb_calls();
    assert_eq!(
        calls.iter().filter(|c| c[0] == "describe").count(),
        1,
        "the screen size is cached"
    );
    assert!(calls.contains(&vec![
        "ui".into(),
        "tap".into(),
        "196".into(),
        "401".into(),
        "--udid".into(),
        PHONE.into()
    ]));
    assert_eq!(
        sim.tap(PHONE, 394.0, 10.0)
            .await
            .expect_err("off screen")
            .code,
        ErrorCode::Invalid
    );
    sim.text(PHONE, "hi\u{1b}[31m\nthere").await.expect("types");
    assert!(host.idb_calls().contains(&vec![
        "ui".into(),
        "text".into(),
        "hi[31mthere".into(),
        "--udid".into(),
        PHONE.into()
    ]));
    sim.key(PHONE, SimKey::Enter).await.expect("enter");
    assert!(host.idb_calls().contains(&vec![
        "ui".into(),
        "key".into(),
        "40".into(),
        "--udid".into(),
        PHONE.into()
    ]));
    sim.home(PHONE).await.expect("home");

    let no_idb = FakeHost::new(false, mac("Booted"));
    let err = service(&no_idb)
        .tap(PHONE, 1.0, 1.0)
        .await
        .expect_err("no idb");
    assert_eq!(err.code, ErrorCode::Unavailable);
    assert!(err.hint.as_deref().unwrap_or("").contains("idb"));
}

#[test]
fn f_cd_21_the_screen_size_comes_from_idb_describe() {
    let s = parse_screen(br#"{"screen_dimensions":{"width":1179,"height":2556,"density":3.0,"width_points":393,"height_points":852}}"#)
        .expect("screen");
    assert_eq!(
        (
            s.width_points,
            s.height_points,
            s.width_pixels,
            s.height_pixels
        ),
        (393.0, 852.0, 1179.0, 2556.0)
    );
    assert!(parse_screen(br#"{"screen_dimensions":{"width":0}}"#).is_none());
    assert!(parse_screen(b"{}").is_none());
    assert_eq!(SimKey::Backspace.hid(), 42);
}

#[tokio::test(start_paused = true)]
async fn f_cd_21_the_stream_sends_10_screenshots_a_second_skipping_unchanged_frames_until_stopped()
{
    let host = FakeHost::new(true, mac("Booted"));
    let sim = service(&host);
    let frames: Arc<Mutex<Vec<Vec<u8>>>> = Arc::default();
    let sink = frames.clone();
    sim.stream_start(
        "pane-1",
        PHONE,
        Box::new(move |png| {
            sink.lock().expect("lock").push(png);
            true
        }),
    )
    .expect("starts");
    for _ in 0..12 {
        tokio::time::advance(FRAME_INTERVAL).await;
        tokio::task::yield_now().await;
    }
    let shots = host.shots.load(Ordering::SeqCst);
    assert!(
        (9..=13).contains(&shots),
        "about 10 screenshots a second, took {shots}"
    );
    let sent = frames.lock().expect("lock").clone();
    // The fake's picture changes every third shot.
    assert_eq!(sent.len(), shots.div_ceil(3));
    assert_eq!(sent[0], b"frame-0");
    assert_eq!(sim.streaming(), 1);
    sim.stream_stop("pane-1");
    let before = host.shots.load(Ordering::SeqCst);
    for _ in 0..5 {
        tokio::time::advance(FRAME_INTERVAL).await;
        tokio::task::yield_now().await;
    }
    assert!(host.shots.load(Ordering::SeqCst) <= before + 1, "stopped");
    assert_eq!(sim.streaming(), 0);
    // Screenshots don't pile up in the scratch folder.
    assert_eq!(
        std::fs::read_dir(host.scratch.path())
            .map(|d| d.count())
            .unwrap_or(0),
        0
    );
}

#[tokio::test(start_paused = true)]
async fn f_cd_21_a_closed_pane_ends_its_stream() {
    let host = FakeHost::new(true, mac("Booted"));
    let sim = service(&host);
    sim.stream_start("pane-2", PHONE, Box::new(|_| false))
        .expect("starts");
    for _ in 0..3 {
        tokio::time::advance(FRAME_INTERVAL).await;
        tokio::task::yield_now().await;
    }
    assert_eq!(sim.streaming(), 0, "the sink said the pane is gone");
}

#[test]
fn f_cd_21_2_off_macos_there_is_no_simulator_host() {
    if cfg!(target_os = "macos") {
        assert!(host().is_some());
    } else {
        assert!(host().is_none(), "no simulator outside macOS");
    }
}

/// The real host on a machine without Xcode (this Linux container): `xcrun`
/// isn't there, which reads as missing tools, and `idb` is absent too.
#[tokio::test]
async fn f_cd_21_2_the_real_host_without_xcrun_reports_missing_tools() {
    if which("xcrun") {
        return; // a Mac with Xcode: the manual QA in HUMAN_TODO covers it.
    }
    let host = macos::XcrunHost::new();
    let out = host
        .simctl(&["list".into(), "--json".into()], CALL_TIMEOUT)
        .await;
    assert_eq!(out, Err(SimError::NoXcode));
    let status = SimulatorService::new(Arc::new(host)).status().await;
    assert!(!status.available);
}

fn which(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
        .unwrap_or(false)
}
