//! OS key codes → the keys a binding names. Compiled everywhere, so each
//! platform's table is unit-tested on every OS.

use super::binding::Modifier;
use super::RawKey;

/// X keysyms for the keys a binding can name.
pub fn keysym_to_raw(sym: u32) -> Option<RawKey> {
    Some(match sym {
        0xffe3 | 0xffe4 => RawKey::Modifier(Modifier::Ctrl),
        0xffe7..=0xffea => RawKey::Modifier(Modifier::Alt),
        0xffe1 | 0xffe2 => RawKey::Modifier(Modifier::Shift),
        0xffeb | 0xffec => RawKey::Modifier(Modifier::Meta),
        0xff1b => RawKey::Key("Esc".into()),
        0x20 => RawKey::Key("Space".into()),
        0xff09 => RawKey::Key("Tab".into()),
        0xff0d => RawKey::Key("Enter".into()),
        0xff08 => RawKey::Key("Backspace".into()),
        0xffff => RawKey::Key("Delete".into()),
        0xff63 => RawKey::Key("Insert".into()),
        0xff50 => RawKey::Key("Home".into()),
        0xff57 => RawKey::Key("End".into()),
        0xff55 => RawKey::Key("PageUp".into()),
        0xff56 => RawKey::Key("PageDown".into()),
        0xff52 => RawKey::Key("Up".into()),
        0xff54 => RawKey::Key("Down".into()),
        0xff51 => RawKey::Key("Left".into()),
        0xff53 => RawKey::Key("Right".into()),
        0xffe5 => RawKey::Key("CapsLock".into()),
        0xff13 => RawKey::Key("Pause".into()),
        0xff14 => RawKey::Key("ScrollLock".into()),
        0xff61 => RawKey::Key("PrintScreen".into()),
        0xff67 => RawKey::Key("Menu".into()),
        0xffbe..=0xffd5 => RawKey::Key(format!("F{}", sym - 0xffbe + 1)),
        0x30..=0x39 => RawKey::Key(char::from(sym as u8).to_string()),
        0x41..=0x5a => RawKey::Key(char::from(sym as u8).to_string()),
        0x61..=0x7a => RawKey::Key(char::from(sym as u8).to_ascii_uppercase().to_string()),
        0x60 => RawKey::Key("`".into()),
        0x2d => RawKey::Key("-".into()),
        0x3d => RawKey::Key("=".into()),
        0x5b => RawKey::Key("[".into()),
        0x5d => RawKey::Key("]".into()),
        0x5c => RawKey::Key("\\".into()),
        0x3b => RawKey::Key(";".into()),
        0x27 => RawKey::Key("'".into()),
        0x2c => RawKey::Key(",".into()),
        0x2e => RawKey::Key(".".into()),
        0x2f => RawKey::Key("/".into()),
        _ => return None,
    })
}

/// Windows virtual-key codes.
pub fn vk_to_raw(vk: u32) -> Option<RawKey> {
    Some(match vk {
        0x10 | 0xA0 | 0xA1 => RawKey::Modifier(Modifier::Shift),
        0x11 | 0xA2 | 0xA3 => RawKey::Modifier(Modifier::Ctrl),
        0x12 | 0xA4 | 0xA5 => RawKey::Modifier(Modifier::Alt),
        0x5B | 0x5C => RawKey::Modifier(Modifier::Meta),
        0x1B => RawKey::Key("Esc".into()),
        0x20 => RawKey::Key("Space".into()),
        0x09 => RawKey::Key("Tab".into()),
        0x0D => RawKey::Key("Enter".into()),
        0x08 => RawKey::Key("Backspace".into()),
        0x2E => RawKey::Key("Delete".into()),
        0x2D => RawKey::Key("Insert".into()),
        0x24 => RawKey::Key("Home".into()),
        0x23 => RawKey::Key("End".into()),
        0x21 => RawKey::Key("PageUp".into()),
        0x22 => RawKey::Key("PageDown".into()),
        0x26 => RawKey::Key("Up".into()),
        0x28 => RawKey::Key("Down".into()),
        0x25 => RawKey::Key("Left".into()),
        0x27 => RawKey::Key("Right".into()),
        0x14 => RawKey::Key("CapsLock".into()),
        0x13 => RawKey::Key("Pause".into()),
        0x91 => RawKey::Key("ScrollLock".into()),
        0x2C => RawKey::Key("PrintScreen".into()),
        0x5D => RawKey::Key("Menu".into()),
        0x30..=0x39 | 0x41..=0x5A => RawKey::Key(char::from(vk as u8).to_string()),
        0x70..=0x87 => RawKey::Key(format!("F{}", vk - 0x70 + 1)),
        0xC0 => RawKey::Key("`".into()),
        0xBD => RawKey::Key("-".into()),
        0xBB => RawKey::Key("=".into()),
        0xDB => RawKey::Key("[".into()),
        0xDD => RawKey::Key("]".into()),
        0xDC => RawKey::Key("\\".into()),
        0xBA => RawKey::Key(";".into()),
        0xDE => RawKey::Key("'".into()),
        0xBC => RawKey::Key(",".into()),
        0xBE => RawKey::Key(".".into()),
        0xBF => RawKey::Key("/".into()),
        _ => return None,
    })
}

/// macOS virtual key codes (Carbon `kVK_*`), for key-down/up events.
pub fn mac_keycode_to_raw(code: u16) -> Option<RawKey> {
    let letters: &[(u16, &str)] = &[
        (0x00, "A"),
        (0x0B, "B"),
        (0x08, "C"),
        (0x02, "D"),
        (0x0E, "E"),
        (0x03, "F"),
        (0x05, "G"),
        (0x04, "H"),
        (0x22, "I"),
        (0x26, "J"),
        (0x28, "K"),
        (0x25, "L"),
        (0x2E, "M"),
        (0x2D, "N"),
        (0x1F, "O"),
        (0x23, "P"),
        (0x0C, "Q"),
        (0x0F, "R"),
        (0x01, "S"),
        (0x11, "T"),
        (0x20, "U"),
        (0x09, "V"),
        (0x0D, "W"),
        (0x07, "X"),
        (0x10, "Y"),
        (0x06, "Z"),
        (0x1D, "0"),
        (0x12, "1"),
        (0x13, "2"),
        (0x14, "3"),
        (0x15, "4"),
        (0x17, "5"),
        (0x16, "6"),
        (0x1A, "7"),
        (0x1C, "8"),
        (0x19, "9"),
        (0x31, "Space"),
        (0x35, "Esc"),
        (0x30, "Tab"),
        (0x24, "Enter"),
        (0x33, "Backspace"),
        (0x75, "Delete"),
        (0x73, "Home"),
        (0x77, "End"),
        (0x74, "PageUp"),
        (0x79, "PageDown"),
        (0x7E, "Up"),
        (0x7D, "Down"),
        (0x7B, "Left"),
        (0x7C, "Right"),
        (0x32, "`"),
        (0x1B, "-"),
        (0x18, "="),
        (0x21, "["),
        (0x1E, "]"),
        (0x2A, "\\"),
        (0x29, ";"),
        (0x27, "'"),
        (0x2B, ","),
        (0x2F, "."),
        (0x2C, "/"),
        (0x7A, "F1"),
        (0x78, "F2"),
        (0x63, "F3"),
        (0x76, "F4"),
        (0x60, "F5"),
        (0x61, "F6"),
        (0x62, "F7"),
        (0x64, "F8"),
        (0x65, "F9"),
        (0x6D, "F10"),
        (0x67, "F11"),
        (0x6F, "F12"),
        (0x69, "F13"),
        (0x6B, "F14"),
        (0x71, "F15"),
        (0x6A, "F16"),
        (0x40, "F17"),
        (0x4F, "F18"),
        (0x50, "F19"),
        (0x5A, "F20"),
    ];
    letters
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, n)| RawKey::Key((*n).to_string()))
}

/// macOS `CGEventFlags` bits → the modifiers held.
pub fn mac_flags_to_modifiers(flags: u64) -> Vec<Modifier> {
    const SHIFT: u64 = 0x0002_0000;
    const CONTROL: u64 = 0x0004_0000;
    const ALTERNATE: u64 = 0x0008_0000;
    const COMMAND: u64 = 0x0010_0000;
    const SECONDARY_FN: u64 = 0x0080_0000;
    let mut out = Vec::new();
    for (bit, m) in [
        (CONTROL, Modifier::Ctrl),
        (ALTERNATE, Modifier::Alt),
        (SHIFT, Modifier::Shift),
        (COMMAND, Modifier::Meta),
        (SECONDARY_FN, Modifier::Fn),
    ] {
        if flags & bit != 0 {
            out.push(m);
        }
    }
    out
}

/// Arrow and function keys set the Fn flag on macOS too; it only means the
/// Fn key while no such key is down.
pub fn mac_fn_flag_is_real(code: u16) -> bool {
    !matches!(
        code,
        0x7B..=0x7E | 0x60..=0x6F | 0x72..=0x7A | 0x40 | 0x4F | 0x50 | 0x5A
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_di_02_windows_vk_table() {
        assert_eq!(vk_to_raw(0xA2), Some(RawKey::Modifier(Modifier::Ctrl)));
        assert_eq!(vk_to_raw(0x5B), Some(RawKey::Modifier(Modifier::Meta)));
        assert_eq!(vk_to_raw(0x44), Some(RawKey::Key("D".into())));
        assert_eq!(vk_to_raw(0x87), Some(RawKey::Key("F24".into())));
        assert_eq!(vk_to_raw(0xE8), None, "the masking key is never a binding");
    }

    #[test]
    fn f_di_02_mac_tables() {
        assert_eq!(mac_keycode_to_raw(0x31), Some(RawKey::Key("Space".into())));
        assert_eq!(mac_keycode_to_raw(0x02), Some(RawKey::Key("D".into())));
        assert_eq!(
            mac_flags_to_modifiers(0x0080_0000 | 0x0004_0000),
            vec![Modifier::Ctrl, Modifier::Fn]
        );
        assert!(!mac_fn_flag_is_real(0x7E));
        assert!(mac_fn_flag_is_real(0x3F));
    }
}
