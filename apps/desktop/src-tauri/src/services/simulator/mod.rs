//! The Simulator pane (F-CD-21, macOS only; ARCHITECTURE §3 "Simulator").
//!
//! Everything here is plain logic over a `SimHost` (the `xcrun simctl` and
//! `idb` processes), so it builds and is tested on every platform with a fake.
//! The real host lives in `macos.rs`, compiled only on macOS; elsewhere
//! [`host`] is `None` and the app registers no simulator command, permission,
//! picker tile or setting (D-1400).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tandem_core::{ErrorCode, IpcError};
use ts_rs::TS;

// The real host. It uses only std and tokio, so tests build it everywhere
// and the Linux gate type-checks it (D-1400).
#[cfg(any(target_os = "macos", test))]
mod macos;

/// How long a `simctl` call may take (boot waits longer, below).
pub const CALL_TIMEOUT: Duration = Duration::from_secs(20);
/// Booting a device until it reports booted (F-CD-21 AC1 allows 30 s to the
/// home screen; a cold first boot of a new runtime can take longer).
pub const BOOT_TIMEOUT: Duration = Duration::from_secs(180);
/// 10 frames a second (PRODUCT_SPEC F-CD-21).
pub const FRAME_INTERVAL: Duration = Duration::from_millis(100);
/// Typed text per `idb ui text` call.
pub const MAX_TEXT: usize = 1000;

/// The real host on macOS; `None` everywhere else.
pub fn host() -> Option<Arc<dyn SimHost>> {
    #[cfg(target_os = "macos")]
    {
        Some(Arc::new(macos::XcrunHost::new()))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Why a host call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimError {
    /// `xcrun` or `simctl` isn't there: no Xcode command-line tools.
    NoXcode,
    /// The process ran and failed; its stderr (trimmed, short).
    Failed(String),
    Timeout,
    /// `idb` isn't installed, so input can't be forwarded.
    NoIdb,
}

impl SimError {
    pub fn into_ipc(self) -> IpcError {
        match self {
            SimError::NoXcode => IpcError::new(
                ErrorCode::Unavailable,
                "The Simulator needs Xcode's command-line tools.",
            )
            .with_hint("Install Xcode from the App Store, open it once, then run `xcode-select --install`.")
            .with_stage("simulator"),
            SimError::Failed(msg) => IpcError::new(ErrorCode::Internal, msg).with_stage("simulator"),
            SimError::Timeout => IpcError::new(
                ErrorCode::Timeout,
                "The Simulator didn't answer in time.",
            )
            .with_stage("simulator"),
            SimError::NoIdb => IpcError::new(
                ErrorCode::Unavailable,
                "Clicks and typing need idb.",
            )
            .with_hint("Install it with `brew install facebook/fb/idb-companion` and `pip3 install fb-idb`.")
            .with_stage("simulator"),
        }
    }
}

/// Reads what a failed `xcrun simctl` printed: a missing tool chain reads as
/// [`SimError::NoXcode`], anything else keeps its first line.
pub fn classify_failure(stderr: &str) -> SimError {
    let lower = stderr.to_lowercase();
    if lower.contains("unable to find utility \"simctl\"")
        || lower.contains("xcrun: error: invalid active developer path")
        || lower.contains("requires xcode")
        || lower.contains("no developer tools were found")
    {
        return SimError::NoXcode;
    }
    let line = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("The Simulator command failed.");
    SimError::Failed(line.chars().take(300).collect())
}

/// The processes the pane drives; faked in tests.
#[async_trait]
pub trait SimHost: Send + Sync {
    /// `xcrun simctl <args>`: stdout, or why it failed.
    async fn simctl(&self, args: &[String], timeout: Duration) -> Result<Vec<u8>, SimError>;
    /// `idb <args>`: stdout, or [`SimError::NoIdb`] when it isn't installed.
    async fn idb(&self, args: &[String]) -> Result<Vec<u8>, SimError>;
    fn has_idb(&self) -> bool;
    /// Where screenshots are written before they're read back.
    fn scratch_dir(&self) -> PathBuf;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SimDeviceState {
    Shutdown,
    Booting,
    Booted,
    ShuttingDown,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SimRuntime {
    pub identifier: String,
    /// "iOS 18.0".
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SimDevice {
    pub udid: String,
    /// "iPhone 16".
    pub name: String,
    pub runtime: String,
    pub state: SimDeviceState,
}

/// `sim_status`: whether the pane can be offered, and what to pick from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SimStatus {
    /// Xcode's tools answer and at least one iOS runtime is installed (F-CD-21 AC2).
    pub available: bool,
    /// Why not, when not.
    pub reason: Option<String>,
    pub runtimes: Vec<SimRuntime>,
    /// Available iOS devices, booted first, then by runtime (newest) and name.
    pub devices: Vec<SimDevice>,
    /// `idb` is installed: clicks and typing are forwarded.
    pub input: bool,
}

#[derive(Deserialize)]
struct RawList {
    #[serde(default)]
    runtimes: Vec<RawRuntime>,
    #[serde(default)]
    devices: HashMap<String, Vec<RawDevice>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRuntime {
    identifier: String,
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    platform: Option<String>,
    #[serde(default = "yes")]
    is_available: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDevice {
    udid: String,
    name: String,
    #[serde(default)]
    state: String,
    #[serde(default = "yes")]
    is_available: bool,
}

fn yes() -> bool {
    true
}

fn version_key(v: &str) -> Vec<u32> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// Parses `xcrun simctl list --json devices runtimes`: the available iOS
/// runtimes and their available devices (watchOS, tvOS and visionOS are left
/// out; the pane is for phones and tablets).
pub fn parse_list(json: &[u8]) -> Result<(Vec<SimRuntime>, Vec<SimDevice>), SimError> {
    let raw: RawList = serde_json::from_slice(json)
        .map_err(|_| SimError::Failed("The Simulator's device list couldn't be read.".into()))?;
    let is_ios = |r: &RawRuntime| match &r.platform {
        Some(p) => p == "iOS",
        None => r.identifier.contains(".iOS-"),
    };
    let mut runtimes: Vec<SimRuntime> = raw
        .runtimes
        .iter()
        .filter(|r| r.is_available && is_ios(r))
        .map(|r| SimRuntime {
            identifier: r.identifier.clone(),
            name: r.name.clone(),
            version: r.version.clone(),
        })
        .collect();
    runtimes.sort_by_key(|r| std::cmp::Reverse(version_key(&r.version)));
    let mut devices = Vec::new();
    for rt in &runtimes {
        for d in raw.devices.get(&rt.identifier).into_iter().flatten() {
            if !d.is_available {
                continue;
            }
            devices.push(SimDevice {
                udid: d.udid.clone(),
                name: d.name.clone(),
                runtime: rt.name.clone(),
                state: match d.state.as_str() {
                    "Shutdown" => SimDeviceState::Shutdown,
                    "Booted" => SimDeviceState::Booted,
                    "Booting" => SimDeviceState::Booting,
                    "Shutting Down" => SimDeviceState::ShuttingDown,
                    _ => SimDeviceState::Unknown,
                },
            });
        }
    }
    // Booted first; otherwise runtimes stay newest first and names in order.
    devices.sort_by_key(|d| d.state != SimDeviceState::Booted);
    Ok((runtimes, devices))
}

/// A device's screen in points (what `idb ui tap` takes) and in pixels (what
/// a screenshot has).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SimScreen {
    pub width_points: f64,
    pub height_points: f64,
    pub width_pixels: f64,
    pub height_pixels: f64,
}

/// Parses `idb describe --json --udid <udid>`'s `screen_dimensions`.
pub fn parse_screen(json: &[u8]) -> Option<SimScreen> {
    let v: serde_json::Value = serde_json::from_slice(json).ok()?;
    let d = v.get("screen_dimensions")?;
    let n = |k: &str| d.get(k).and_then(|x| x.as_f64()).filter(|x| *x > 0.0);
    Some(SimScreen {
        width_points: n("width_points")?,
        height_points: n("height_points")?,
        width_pixels: n("width")?,
        height_pixels: n("height")?,
    })
}

/// Keys forwarded as HID usages (`idb ui key`); printable text goes through
/// `idb ui text` instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SimKey {
    Enter,
    Backspace,
    Tab,
    Escape,
    Left,
    Right,
    Up,
    Down,
}

impl SimKey {
    /// USB HID keyboard usage ids.
    pub fn hid(self) -> u8 {
        match self {
            SimKey::Enter => 40,
            SimKey::Escape => 41,
            SimKey::Backspace => 42,
            SimKey::Tab => 43,
            SimKey::Right => 79,
            SimKey::Left => 80,
            SimKey::Down => 81,
            SimKey::Up => 82,
        }
    }
}

/// A udid is a UUID; anything else is refused before it reaches a process.
pub fn valid_udid(udid: &str) -> bool {
    udid.len() == 36
        && udid.chars().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

fn bad_udid() -> IpcError {
    IpcError::new(ErrorCode::Invalid, "That isn't a simulator device.")
}

/// Drives the simulators for the panes; one per app.
pub struct SimulatorService {
    host: Arc<dyn SimHost>,
    /// Pane id → its frame loop's stop flag.
    streams: Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>>,
    /// Screens by udid, once `idb describe` answered.
    screens: Mutex<HashMap<String, SimScreen>>,
}

impl SimulatorService {
    pub fn new(host: Arc<dyn SimHost>) -> Self {
        Self {
            host,
            streams: Mutex::new(HashMap::new()),
            screens: Mutex::new(HashMap::new()),
        }
    }

    async fn simctl(&self, args: &[&str], timeout: Duration) -> Result<Vec<u8>, SimError> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        self.host.simctl(&args, timeout).await
    }

    /// `sim_status` (F-CD-21 AC2: no tools or no runtime → not offered).
    pub async fn status(&self) -> SimStatus {
        let unavailable = |reason: &str| SimStatus {
            available: false,
            reason: Some(reason.into()),
            runtimes: vec![],
            devices: vec![],
            input: false,
        };
        let out = match self
            .simctl(&["list", "--json", "devices", "runtimes"], CALL_TIMEOUT)
            .await
        {
            Ok(out) => out,
            Err(SimError::NoXcode) => {
                return unavailable("Xcode's command-line tools aren't installed.")
            }
            Err(SimError::Timeout) => return unavailable("The Simulator didn't answer in time."),
            Err(SimError::Failed(msg)) => return unavailable(&msg),
            Err(SimError::NoIdb) => return unavailable("The Simulator isn't available."),
        };
        match parse_list(&out) {
            Ok((runtimes, _)) if runtimes.is_empty() => {
                unavailable("No iOS Simulator runtime is installed.")
            }
            Ok((runtimes, devices)) => SimStatus {
                available: true,
                reason: None,
                runtimes,
                devices,
                input: self.host.has_idb(),
            },
            Err(_) => unavailable("The Simulator's device list couldn't be read."),
        }
    }

    async fn device(&self, udid: &str) -> Result<SimDevice, IpcError> {
        let status = self.status().await;
        if !status.available {
            return Err(SimError::Failed(status.reason.unwrap_or_default()).into_ipc());
        }
        status
            .devices
            .into_iter()
            .find(|d| d.udid == udid)
            .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "That simulator device is gone."))
    }

    /// Boots the device and waits until it reports booted. Booting a device
    /// that is already booted is fine.
    pub async fn boot(&self, udid: &str) -> Result<SimDevice, IpcError> {
        if !valid_udid(udid) {
            return Err(bad_udid());
        }
        let device = self.device(udid).await?;
        if device.state != SimDeviceState::Booted {
            // `bootstatus -b` boots when needed and returns once it's booted.
            self.simctl(&["bootstatus", udid, "-b"], BOOT_TIMEOUT)
                .await
                .or_else(|e| match e {
                    SimError::Failed(m) if m.contains("current state: Booted") => Ok(vec![]),
                    e => Err(e),
                })
                .map_err(SimError::into_ipc)?;
        }
        Ok(SimDevice {
            state: SimDeviceState::Booted,
            ..device
        })
    }

    pub async fn shutdown(&self, udid: &str) -> Result<SimDevice, IpcError> {
        if !valid_udid(udid) {
            return Err(bad_udid());
        }
        let device = self.device(udid).await?;
        self.simctl(&["shutdown", udid], CALL_TIMEOUT)
            .await
            .or_else(|e| match e {
                SimError::Failed(m) if m.contains("current state: Shutdown") => Ok(vec![]),
                e => Err(e),
            })
            .map_err(SimError::into_ipc)?;
        Ok(SimDevice {
            state: SimDeviceState::Shutdown,
            ..device
        })
    }

    /// Installs a built `.app` bundle on a booted device.
    pub async fn install(&self, udid: &str, app: &Path) -> Result<(), IpcError> {
        if !valid_udid(udid) {
            return Err(bad_udid());
        }
        if app.extension().and_then(|e| e.to_str()) != Some("app") || !app.is_dir() {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "Choose a built .app bundle (a folder ending in .app).",
            ));
        }
        let device = self.device(udid).await?;
        if device.state != SimDeviceState::Booted {
            return Err(IpcError::new(ErrorCode::Invalid, "Boot the device first."));
        }
        let path = app.to_string_lossy().into_owned();
        self.simctl(&["install", udid, &path], BOOT_TIMEOUT)
            .await
            .map(|_| ())
            .map_err(SimError::into_ipc)
    }

    /// One PNG of the device's screen.
    pub async fn screenshot(&self, udid: &str, tag: &str) -> Result<Vec<u8>, SimError> {
        let dir = self.host.scratch_dir();
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("sim-{tag}.png"));
        let p = path.to_string_lossy().into_owned();
        self.simctl(&["io", udid, "screenshot", "--type=png", &p], CALL_TIMEOUT)
            .await?;
        let bytes = std::fs::read(&path)
            .map_err(|_| SimError::Failed("The screenshot couldn't be read.".into()))?;
        let _ = std::fs::remove_file(&path);
        Ok(bytes)
    }

    /// The device's screen size, from `idb describe` (cached).
    pub async fn screen(&self, udid: &str) -> Result<SimScreen, IpcError> {
        if !valid_udid(udid) {
            return Err(bad_udid());
        }
        if let Some(s) = self.screens.lock().ok().and_then(|m| m.get(udid).copied()) {
            return Ok(s);
        }
        let out = self
            .host
            .idb(&[
                "describe".into(),
                "--json".into(),
                "--udid".into(),
                udid.into(),
            ])
            .await
            .map_err(SimError::into_ipc)?;
        let screen = parse_screen(&out).ok_or_else(|| {
            SimError::Failed("idb didn't report the device's screen size.".into()).into_ipc()
        })?;
        if let Ok(mut m) = self.screens.lock() {
            m.insert(udid.to_string(), screen);
        }
        Ok(screen)
    }

    /// A click at a point on the device's screen (points, not pixels).
    pub async fn tap(&self, udid: &str, x: f64, y: f64) -> Result<(), IpcError> {
        let screen = self.screen(udid).await?;
        if !(x.is_finite() && y.is_finite())
            || x < 0.0
            || y < 0.0
            || x > screen.width_points
            || y > screen.height_points
        {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "That point is off the screen.",
            ));
        }
        let (x, y) = (
            format!("{}", x.round() as i64),
            format!("{}", y.round() as i64),
        );
        self.host
            .idb(&[
                "ui".into(),
                "tap".into(),
                x,
                y,
                "--udid".into(),
                udid.into(),
            ])
            .await
            .map(|_| ())
            .map_err(SimError::into_ipc)
    }

    pub async fn text(&self, udid: &str, text: &str) -> Result<(), IpcError> {
        if !valid_udid(udid) {
            return Err(bad_udid());
        }
        // Control characters travel as keys, never inside text.
        let clean: String = text
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_TEXT)
            .collect();
        if clean.is_empty() {
            return Ok(());
        }
        self.host
            .idb(&[
                "ui".into(),
                "text".into(),
                clean,
                "--udid".into(),
                udid.into(),
            ])
            .await
            .map(|_| ())
            .map_err(SimError::into_ipc)
    }

    pub async fn key(&self, udid: &str, key: SimKey) -> Result<(), IpcError> {
        if !valid_udid(udid) {
            return Err(bad_udid());
        }
        self.host
            .idb(&[
                "ui".into(),
                "key".into(),
                key.hid().to_string(),
                "--udid".into(),
                udid.into(),
            ])
            .await
            .map(|_| ())
            .map_err(SimError::into_ipc)
    }

    pub async fn home(&self, udid: &str) -> Result<(), IpcError> {
        if !valid_udid(udid) {
            return Err(bad_udid());
        }
        self.host
            .idb(&[
                "ui".into(),
                "button".into(),
                "HOME".into(),
                "--udid".into(),
                udid.into(),
            ])
            .await
            .map(|_| ())
            .map_err(SimError::into_ipc)
    }

    /// Starts (or restarts) a pane's frame loop: a screenshot every 100 ms,
    /// sent only when it changed. It stops with [`Self::stream_stop`], when
    /// `send` reports the pane is gone, or after ~3 s of failed screenshots
    /// (the device shut down).
    pub fn stream_start(
        self: &Arc<Self>,
        pane: &str,
        udid: &str,
        send: Box<dyn Fn(Vec<u8>) -> bool + Send + Sync>,
    ) -> Result<(), IpcError> {
        if !valid_udid(udid) {
            return Err(bad_udid());
        }
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        if let Ok(mut m) = self.streams.lock() {
            if let Some(old) = m.insert(pane.to_string(), stop.clone()) {
                old.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let this = self.clone();
        let (pane, udid) = (pane.to_string(), udid.to_string());
        tokio::spawn(async move {
            let mut last: Option<u64> = None;
            let mut failures = 0u32;
            let tag = pane.clone();
            while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                match this.screenshot(&udid, &tag).await {
                    Ok(png) => {
                        failures = 0;
                        let hash = fnv(&png);
                        if last != Some(hash) {
                            last = Some(hash);
                            if !send(png) {
                                break;
                            }
                        }
                    }
                    Err(err) => {
                        failures += 1;
                        // A device that shut down stops answering; give up after ~3 s.
                        if failures >= 30 {
                            tracing::warn!(?err, "simulator: the frame stream stopped");
                            break;
                        }
                    }
                }
                tokio::time::sleep(FRAME_INTERVAL).await;
            }
            if let Ok(mut m) = this.streams.lock() {
                if m.get(&pane).is_some_and(|s| Arc::ptr_eq(s, &stop)) {
                    m.remove(&pane);
                }
            }
        });
        Ok(())
    }

    pub fn stream_stop(&self, pane: &str) {
        if let Some(stop) = self.streams.lock().ok().and_then(|mut m| m.remove(pane)) {
            stop.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    pub fn streaming(&self) -> usize {
        self.streams.lock().map(|m| m.len()).unwrap_or(0)
    }
}

/// A small, stable hash to skip unchanged frames.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}

#[cfg(test)]
mod tests;
