//! A Dictate shortcut: modifiers, at most one other key, or a mouse button
//! (F-DI-01). Stored as text like `Ctrl+Alt`, `Fn`, `Ctrl+Shift+Space`,
//! `Mouse4`.

use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
    /// ⌘ on macOS, the Windows key, Super on Linux.
    Meta,
    /// Fn / 🌐 (macOS only).
    Fn,
}

impl Modifier {
    fn name(self) -> &'static str {
        match self {
            Self::Ctrl => "Ctrl",
            Self::Alt => "Alt",
            Self::Shift => "Shift",
            Self::Meta => "Meta",
            Self::Fn => "Fn",
        }
    }
}

/// The non-modifier part.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Trigger {
    /// A key by its name: `A`…`Z`, `0`…`9`, `F1`…`F24`, `Space`, `Esc`, …
    Key(String),
    Mouse4,
    Mouse5,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Binding {
    pub modifiers: Vec<Modifier>,
    pub trigger: Option<Trigger>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BindingError {
    #[error("Choose a key")]
    Empty,
    #[error("\"{0}\" isn't a key Tandem knows")]
    Unknown(String),
    #[error("Use one key with any modifiers")]
    TwoKeys,
    #[error("Toggle needs a key that isn't only modifiers.")]
    ToggleModifierOnly,
    #[error("Esc cancels a take, so it can't start one")]
    Escape,
    #[error("Alt+Space opens the window menu on this system")]
    AltSpace,
    #[error("Fn is only available on macOS")]
    FnElsewhere,
}

/// Which OS the rules are checked for (tests pass another one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
    Linux,
}

impl Os {
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

const KEY_NAMES: &[&str] = &[
    "Space",
    "Tab",
    "Enter",
    "Backspace",
    "Delete",
    "Insert",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "Up",
    "Down",
    "Left",
    "Right",
    "CapsLock",
    "Pause",
    "ScrollLock",
    "PrintScreen",
    "Menu",
    "Esc",
    "`",
    "-",
    "=",
    "[",
    "]",
    "\\",
    ";",
    "'",
    ",",
    ".",
    "/",
];

fn key_name(raw: &str) -> Option<String> {
    let lower = raw.to_lowercase();
    let alias = match lower.as_str() {
        "escape" => "Esc",
        "return" => "Enter",
        "spacebar" => "Space",
        "del" => "Delete",
        "ins" => "Insert",
        "pgup" => "PageUp",
        "pgdn" => "PageDown",
        "arrowup" => "Up",
        "arrowdown" => "Down",
        "arrowleft" => "Left",
        "arrowright" => "Right",
        _ => "",
    };
    if !alias.is_empty() {
        return Some(alias.to_string());
    }
    if let Some(k) = KEY_NAMES.iter().find(|k| k.to_lowercase() == lower) {
        return Some(k.to_string());
    }
    let mut chars = raw.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_alphanumeric() {
            return Some(c.to_ascii_uppercase().to_string());
        }
    }
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        if (1..=24).contains(&n) {
            return Some(format!("F{n}"));
        }
    }
    None
}

fn modifier(raw: &str) -> Option<Modifier> {
    Some(match raw.to_lowercase().as_str() {
        "ctrl" | "control" | "ctl" | "⌃" => Modifier::Ctrl,
        "alt" | "option" | "opt" | "⌥" => Modifier::Alt,
        "shift" | "⇧" => Modifier::Shift,
        "meta" | "cmd" | "command" | "super" | "win" | "windows" | "⌘" => Modifier::Meta,
        "fn" | "globe" | "🌐" => Modifier::Fn,
        _ => return None,
    })
}

impl FromStr for Binding {
    type Err = BindingError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut modifiers = Vec::new();
        let mut trigger = None;
        // `+` itself can't be bound, so splitting on it is unambiguous.
        for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            if let Some(m) = modifier(part) {
                if !modifiers.contains(&m) {
                    modifiers.push(m);
                }
                continue;
            }
            let t = match part.to_lowercase().as_str() {
                "mouse4" | "xbutton1" | "back" => Trigger::Mouse4,
                "mouse5" | "xbutton2" | "forward" => Trigger::Mouse5,
                _ => {
                    Trigger::Key(key_name(part).ok_or_else(|| BindingError::Unknown(part.into()))?)
                }
            };
            if trigger.replace(t).is_some() {
                return Err(BindingError::TwoKeys);
            }
        }
        if modifiers.is_empty() && trigger.is_none() {
            return Err(BindingError::Empty);
        }
        modifiers.sort();
        Ok(Self { modifiers, trigger })
    }
}

impl fmt::Display for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<String> = self
            .modifiers
            .iter()
            .map(|m| m.name().to_string())
            .collect();
        match &self.trigger {
            Some(Trigger::Key(k)) => parts.push(k.clone()),
            Some(Trigger::Mouse4) => parts.push("Mouse4".into()),
            Some(Trigger::Mouse5) => parts.push("Mouse5".into()),
            None => {}
        }
        f.write_str(&parts.join("+"))
    }
}

impl Binding {
    pub fn is_modifier_only(&self) -> bool {
        self.trigger.is_none()
    }

    /// Fn (🌐) alone: macOS's default hold key, which some keyboards and
    /// system settings never deliver (F-DI-02).
    pub fn is_fn_alone(&self) -> bool {
        self.trigger.is_none() && self.modifiers == [Modifier::Fn]
    }

    /// What Dictate switches to when Fn isn't heard: Control+Option.
    pub fn fn_fallback() -> Self {
        Self {
            modifiers: vec![Modifier::Ctrl, Modifier::Alt],
            trigger: None,
        }
    }

    /// The default hold key: Fn on macOS, Ctrl+Win on Windows, Ctrl+Alt on Linux.
    pub fn default_hold(os: Os) -> Self {
        let s = match os {
            Os::Mac => "Fn",
            Os::Windows => "Ctrl+Meta",
            Os::Linux => "Ctrl+Alt",
        };
        s.parse().unwrap_or(Self {
            modifiers: vec![Modifier::Ctrl],
            trigger: None,
        })
    }

    /// The macOS fallback when Fn events never arrive.
    pub fn mac_fallback() -> Self {
        Self {
            modifiers: vec![Modifier::Ctrl, Modifier::Alt],
            trigger: None,
        }
    }

    /// Checks a binding for a mode on an OS.
    pub fn validate(&self, toggle: bool, os: Os) -> Result<(), BindingError> {
        if toggle && self.is_modifier_only() {
            return Err(BindingError::ToggleModifierOnly);
        }
        if self.trigger == Some(Trigger::Key("Esc".into())) {
            return Err(BindingError::Escape);
        }
        if os != Os::Mac {
            if self.modifiers.contains(&Modifier::Fn) {
                return Err(BindingError::FnElsewhere);
            }
            if self.modifiers == [Modifier::Alt]
                && self.trigger == Some(Trigger::Key("Space".into()))
            {
                return Err(BindingError::AltSpace);
            }
        }
        Ok(())
    }

    /// How the shortcut reads in the UI on an OS (`Ctrl+Win`, `⌃⌥`, `Fn`).
    pub fn label(&self, os: Os) -> String {
        let m = |m: &Modifier| -> String {
            match (os, m) {
                (Os::Mac, Modifier::Ctrl) => "Control".into(),
                (Os::Mac, Modifier::Alt) => "Option".into(),
                (Os::Mac, Modifier::Meta) => "Command".into(),
                (Os::Windows, Modifier::Meta) => "Win".into(),
                (Os::Linux, Modifier::Meta) => "Super".into(),
                (_, other) => other.name().into(),
            }
        };
        let mut parts: Vec<String> = self.modifiers.iter().map(m).collect();
        match &self.trigger {
            Some(Trigger::Key(k)) => parts.push(k.clone()),
            Some(Trigger::Mouse4) => parts.push("Mouse 4".into()),
            Some(Trigger::Mouse5) => parts.push("Mouse 5".into()),
            None => {}
        }
        parts.join("+")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_di_02_fn_alone_and_its_fallback() {
        let fn_key: Binding = "Fn".parse().expect("fn");
        assert!(fn_key.is_fn_alone());
        assert!(!"Fn+Ctrl".parse::<Binding>().expect("b").is_fn_alone());
        assert_eq!(Binding::fn_fallback().to_string(), "Ctrl+Alt");
        assert!(Binding::fn_fallback().validate(false, Os::Mac).is_ok());
    }

    #[test]
    fn f_di_01_parses_and_prints_canonically() {
        let b: Binding = "control + option".parse().expect("parse");
        assert_eq!(b.to_string(), "Ctrl+Alt");
        assert!(b.is_modifier_only());
        let b: Binding = "shift+ctrl+space".parse().expect("parse");
        assert_eq!(b.to_string(), "Ctrl+Shift+Space");
        assert_eq!(
            "mouse4".parse::<Binding>().expect("m").to_string(),
            "Mouse4"
        );
        assert_eq!("f13".parse::<Binding>().expect("f").to_string(), "F13");
        assert_eq!("".parse::<Binding>(), Err(BindingError::Empty));
        assert_eq!("Ctrl+A+B".parse::<Binding>(), Err(BindingError::TwoKeys));
        assert!(matches!(
            "Hyper".parse::<Binding>(),
            Err(BindingError::Unknown(_))
        ));
    }

    #[test]
    fn f_di_01_4_toggle_rejects_a_modifier_only_binding() {
        let b: Binding = "Ctrl+Alt".parse().expect("parse");
        assert_eq!(
            b.validate(true, Os::Linux),
            Err(BindingError::ToggleModifierOnly)
        );
        assert_eq!(
            BindingError::ToggleModifierOnly.to_string(),
            "Toggle needs a key that isn't only modifiers."
        );
        assert_eq!(b.validate(false, Os::Linux), Ok(()));
        let k: Binding = "Ctrl+Alt+D".parse().expect("parse");
        assert_eq!(k.validate(true, Os::Linux), Ok(()));
    }

    #[test]
    fn f_di_01_defaults_and_os_rules() {
        assert_eq!(Binding::default_hold(Os::Mac).to_string(), "Fn");
        assert_eq!(Binding::default_hold(Os::Windows).to_string(), "Ctrl+Meta");
        assert_eq!(Binding::default_hold(Os::Linux).to_string(), "Ctrl+Alt");
        assert_eq!(
            Binding::default_hold(Os::Windows).label(Os::Windows),
            "Ctrl+Win"
        );
        assert_eq!(Binding::mac_fallback().label(Os::Mac), "Control+Option");
        let fn_key = Binding::default_hold(Os::Mac);
        assert_eq!(
            fn_key.validate(false, Os::Linux),
            Err(BindingError::FnElsewhere)
        );
        let alt_space: Binding = "Alt+Space".parse().expect("parse");
        assert_eq!(
            alt_space.validate(false, Os::Windows),
            Err(BindingError::AltSpace)
        );
        assert_eq!(alt_space.validate(false, Os::Mac), Ok(()));
        let esc: Binding = "Esc".parse().expect("parse");
        assert_eq!(esc.validate(false, Os::Mac), Err(BindingError::Escape));
    }
}
