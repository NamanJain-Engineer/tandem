//! Which microphone Dictate opens (F-DI-03, ARCHITECTURE §12.1). Pure, so the
//! rules are unit-tested without any audio hardware.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DeviceInfo {
    /// Stable within a run: the backend's device name.
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub bluetooth: bool,
    pub loopback: bool,
}

impl DeviceInfo {
    /// Builds an entry, deriving the Bluetooth and loopback flags from what
    /// the OS names the device (and, on Windows, its id).
    pub fn new(id: impl Into<String>, name: impl Into<String>, is_default: bool) -> Self {
        let id = id.into();
        let name = name.into();
        let bluetooth = is_bluetooth(&id) || is_bluetooth(&name);
        let loopback = is_loopback(&name);
        Self {
            id,
            name,
            is_default,
            bluetooth,
            loopback,
        }
    }
}

/// What the user picked in Settings › Dictate › Microphone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MicPrefs {
    /// `None` = System default.
    pub primary: Option<String>,
    pub backup: Option<String>,
}

const LOOPBACK: &[&str] = &[
    "stereo mix",
    "blackhole",
    "loopback",
    "vb-audio",
    "cable",
    "monitor of",
    "soundflower",
    "what u hear",
    "wave out mix",
    "virtual",
];

const BLUETOOTH: &[&str] = &[
    "bluez",
    "bthenum",
    "bluetooth",
    "airpods",
    "headset",
    "hands-free",
    "hands free",
    "handsfree",
    "hfp",
];

/// Loopback and virtual devices: never picked automatically.
pub fn is_loopback(name: &str) -> bool {
    let n = name.to_lowercase();
    LOOPBACK.iter().any(|w| n.contains(w))
}

/// Bluetooth inputs (macOS reports the transport; elsewhere the name or id says).
pub fn is_bluetooth(name_or_id: &str) -> bool {
    let n = name_or_id.to_lowercase();
    BLUETOOTH.iter().any(|w| n.contains(w))
}

/// Picks the device for a take. An explicit choice always wins while it's
/// present. Otherwise: the OS default unless it's loopback, or Bluetooth while
/// another real mic exists; then the first real non-Bluetooth input; then a
/// Bluetooth one (it's the only mic there is).
pub fn choose(devices: &[DeviceInfo], prefs: &MicPrefs) -> Option<String> {
    if let Some(id) = &prefs.primary {
        if devices.iter().any(|d| &d.id == id) {
            return Some(id.clone());
        }
    }
    auto(devices)
}

/// The device to fail over to when the primary stops delivering audio.
pub fn backup(devices: &[DeviceInfo], prefs: &MicPrefs, failed: &str) -> Option<String> {
    let id = prefs.backup.as_ref()?;
    (id != failed && devices.iter().any(|d| &d.id == id)).then(|| id.clone())
}

fn auto(devices: &[DeviceInfo]) -> Option<String> {
    let real: Vec<&DeviceInfo> = devices.iter().filter(|d| !d.loopback).collect();
    let wired = real.iter().any(|d| !d.bluetooth);
    let usable = |d: &&&DeviceInfo| !wired || !d.bluetooth;
    real.iter()
        .filter(usable)
        .find(|d| d.is_default)
        .or_else(|| real.iter().find(|d| !d.bluetooth))
        .or_else(|| real.first())
        .map(|d| d.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn devs() -> Vec<DeviceInfo> {
        vec![
            DeviceInfo::new("bluez_input.AA", "WH-1000 Headset", true),
            DeviceInfo::new("alsa.builtin", "Built-in Microphone", false),
            DeviceInfo::new("monitor", "Monitor of Built-in Audio", false),
        ]
    }

    #[test]
    fn f_di_03_2_bluetooth_default_loses_to_the_built_in_mic() {
        let d = devs();
        assert!(d[0].bluetooth && !d[1].bluetooth && d[2].loopback);
        assert_eq!(
            choose(&d, &MicPrefs::default()).as_deref(),
            Some("alsa.builtin")
        );
    }

    #[test]
    fn f_di_03_explicit_choice_wins_even_for_bluetooth() {
        let prefs = MicPrefs {
            primary: Some("bluez_input.AA".into()),
            backup: None,
        };
        assert_eq!(choose(&devs(), &prefs).as_deref(), Some("bluez_input.AA"));
        // A choice that's gone falls back to the automatic rule.
        let gone = MicPrefs {
            primary: Some("usb.gone".into()),
            backup: None,
        };
        assert_eq!(choose(&devs(), &gone).as_deref(), Some("alsa.builtin"));
    }

    #[test]
    fn f_di_03_loopback_is_never_auto_selected() {
        let d = vec![
            DeviceInfo::new("bh", "BlackHole 2ch", true),
            DeviceInfo::new("mix", "Stereo Mix (Realtek)", false),
        ];
        assert_eq!(choose(&d, &MicPrefs::default()), None);
        let d = vec![
            DeviceInfo::new("bh", "BlackHole 2ch", true),
            DeviceInfo::new("usb", "USB Microphone", false),
        ];
        assert_eq!(choose(&d, &MicPrefs::default()).as_deref(), Some("usb"));
    }

    #[test]
    fn f_di_03_bluetooth_only_is_still_usable() {
        let d = vec![DeviceInfo::new(
            r"BTHENUM\{0000111e}",
            "Headset Microphone",
            true,
        )];
        assert_eq!(
            choose(&d, &MicPrefs::default()).as_deref(),
            Some(r"BTHENUM\{0000111e}")
        );
    }

    #[test]
    fn f_di_03_backup_is_a_different_present_device() {
        let prefs = MicPrefs {
            primary: Some("alsa.builtin".into()),
            backup: Some("bluez_input.AA".into()),
        };
        assert_eq!(
            backup(&devs(), &prefs, "alsa.builtin").as_deref(),
            Some("bluez_input.AA")
        );
        assert_eq!(backup(&devs(), &prefs, "bluez_input.AA"), None);
    }
}
