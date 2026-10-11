//! Windows PATH for panes and engines (ARCHITECTURE §5.2, F-CD-03): an app
//! started from Explorer keeps the PATH Explorer had at sign-in, so a tool
//! installed since (Node, an engine's installer) isn't found. Windows itself
//! builds a new PATH from the registry: the machine value
//! (`HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment`),
//! then the user's (`HKCU\Environment`), each `%VAR%`-expanded. We do the
//! same and keep the process's own entries after them.
//!
//! The merge is plain code, tested with a fake registry on every platform;
//! only [`SystemRegistry`] (Windows) touches the real one.

use std::collections::BTreeMap;

/// One `Path` value from the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegValue {
    pub value: String,
    /// `REG_EXPAND_SZ`: `%VAR%` references are expanded.
    pub expand: bool,
}

/// Where the machine and user environment values live.
pub trait EnvRegistry {
    /// `name` under the machine environment key.
    fn machine(&self, name: &str) -> Option<RegValue>;
    /// `name` under `HKCU\Environment`.
    fn user(&self, name: &str) -> Option<RegValue>;
}

/// The registry with nothing in it (non-Windows, or when it can't be read).
pub struct NoRegistry;

impl EnvRegistry for NoRegistry {
    fn machine(&self, _name: &str) -> Option<RegValue> {
        None
    }
    fn user(&self, _name: &str) -> Option<RegValue> {
        None
    }
}

/// A variable's value, its name matched case-insensitively (Windows names are).
fn lookup<'a>(vars: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    vars.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// `%NAME%` replaced with its value from `vars` (names matched
/// case-insensitively); unknown names stay as they are, as Windows leaves them.
pub fn expand(text: &str, vars: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => match lookup(vars, &after[..end]) {
                Some(value) => {
                    out.push_str(value);
                    rest = &after[end + 1..];
                }
                // Unknown: keep the `%` and read on from the next character,
                // as ExpandEnvironmentStrings does (`100% %A%` still expands A).
                None => {
                    out.push('%');
                    rest = after;
                }
            },
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A PATH entry compared the way Windows does: case-insensitive, without a
/// trailing separator or surrounding quotes.
fn entry_key(entry: &str) -> String {
    entry
        .trim()
        .trim_matches('"')
        .trim_end_matches(['\\', '/'])
        .to_lowercase()
}

/// The registry's PATH (machine, then user, expanded) followed by the
/// process's entries the registry doesn't have, each once.
pub fn merged_path(
    process_path: Option<&str>,
    reg: &dyn EnvRegistry,
    vars: &BTreeMap<String, String>,
) -> String {
    let value = |v: Option<RegValue>| {
        v.map(|r| {
            if r.expand {
                expand(&r.value, vars)
            } else {
                r.value
            }
        })
        .unwrap_or_default()
    };
    let machine = value(reg.machine("Path"));
    let user = value(reg.user("Path"));
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<&str> = Vec::new();
    for entry in machine
        .split(';')
        .chain(user.split(';'))
        .chain(process_path.unwrap_or_default().split(';'))
    {
        let key = entry_key(entry);
        if !key.is_empty() && seen.insert(key) {
            out.push(entry.trim());
        }
    }
    out.join(";")
}

/// `vars` with PATH replaced by [`merged_path`], under the name the process
/// env already uses (`Path` on Windows), or `Path` when there was none.
pub fn with_registry_path(
    mut vars: BTreeMap<String, String>,
    reg: &dyn EnvRegistry,
) -> BTreeMap<String, String> {
    let key = vars
        .keys()
        .find(|k| k.eq_ignore_ascii_case("PATH"))
        .cloned()
        .unwrap_or_else(|| "Path".to_string());
    let merged = merged_path(vars.get(&key).map(String::as_str), reg, &vars);
    if !merged.is_empty() {
        vars.insert(key, merged);
    }
    vars
}

/// The real registry (Windows).
#[cfg(windows)]
pub struct SystemRegistry;

#[cfg(windows)]
impl SystemRegistry {
    fn read(
        root: windows_sys::Win32::System::Registry::HKEY,
        subkey: &str,
        name: &str,
    ) -> Option<RegValue> {
        use windows_sys::Win32::Foundation::ERROR_SUCCESS;
        use windows_sys::Win32::System::Registry::{
            RegGetValueW, REG_EXPAND_SZ, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
        };
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
        let (subkey, name) = (wide(subkey), wide(name));
        let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
        let mut kind = 0u32;
        let mut bytes = 0u32;
        // SAFETY: the first call only asks for the size; the buffer passed
        // to the second has that many bytes, and every pointer is valid for
        // the call.
        unsafe {
            if RegGetValueW(
                root,
                subkey.as_ptr(),
                name.as_ptr(),
                flags,
                &mut kind,
                std::ptr::null_mut(),
                &mut bytes,
            ) != ERROR_SUCCESS
            {
                return None;
            }
            let mut buf = vec![0u16; (bytes as usize).div_ceil(2) + 1];
            bytes = (buf.len() * 2) as u32;
            if RegGetValueW(
                root,
                subkey.as_ptr(),
                name.as_ptr(),
                flags,
                &mut kind,
                buf.as_mut_ptr().cast(),
                &mut bytes,
            ) != ERROR_SUCCESS
            {
                return None;
            }
            let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
            Some(RegValue {
                value: String::from_utf16_lossy(&buf[..len]),
                expand: kind == REG_EXPAND_SZ,
            })
        }
    }
}

#[cfg(windows)]
impl EnvRegistry for SystemRegistry {
    fn machine(&self, name: &str) -> Option<RegValue> {
        Self::read(
            windows_sys::Win32::System::Registry::HKEY_LOCAL_MACHINE,
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
            name,
        )
    }
    fn user(&self, name: &str) -> Option<RegValue> {
        Self::read(
            windows_sys::Win32::System::Registry::HKEY_CURRENT_USER,
            "Environment",
            name,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeRegistry {
        machine: Option<RegValue>,
        user: Option<RegValue>,
    }

    impl EnvRegistry for FakeRegistry {
        fn machine(&self, name: &str) -> Option<RegValue> {
            (name == "Path").then(|| self.machine.clone()).flatten()
        }
        fn user(&self, name: &str) -> Option<RegValue> {
            (name == "Path").then(|| self.user.clone()).flatten()
        }
    }

    fn sz(value: &str) -> Option<RegValue> {
        Some(RegValue {
            value: value.into(),
            expand: false,
        })
    }

    fn expand_sz(value: &str) -> Option<RegValue> {
        Some(RegValue {
            value: value.into(),
            expand: true,
        })
    }

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn f_cd_03_windows_path_is_machine_then_user_then_the_process_entries() {
        let reg = FakeRegistry {
            machine: expand_sz(r"%SystemRoot%\system32;%SystemRoot%;C:\Program Files\nodejs\"),
            user: expand_sz(r"%USERPROFILE%\AppData\Local\Programs\claude;%UNSET%\bin"),
        };
        let vars = env(&[
            ("Path", r"C:\WINDOWS\system32;C:\Old\tool"),
            ("SystemRoot", r"C:\WINDOWS"),
            ("UserProfile", r"C:\Users\ana"),
        ]);
        let out = with_registry_path(vars, &reg);
        // A tool installed after sign-in (claude) is found; the process's own
        // entry stays, after the registry's; duplicates go, whatever their case
        // or trailing separator; an unknown variable stays as written.
        assert_eq!(
            out["Path"],
            r"C:\WINDOWS\system32;C:\WINDOWS;C:\Program Files\nodejs\;C:\Users\ana\AppData\Local\Programs\claude;%UNSET%\bin;C:\Old\tool"
        );
        assert!(!out.contains_key("PATH"), "the process's own name is kept");
    }

    #[test]
    fn f_cd_03_windows_path_plain_values_are_not_expanded_and_missing_ones_are_skipped() {
        let reg = FakeRegistry {
            machine: sz(r"C:\%literal%;C:\a"),
            user: None,
        };
        let out = with_registry_path(env(&[("PATH", r"C:\a;C:\b"), ("literal", "x")]), &reg);
        assert_eq!(out["PATH"], r"C:\%literal%;C:\a;C:\b");
        // Nothing in the registry: the process PATH, minus empty entries.
        let out = with_registry_path(env(&[("PATH", r"C:\a;;C:\b;")]), &NoRegistry);
        assert_eq!(out["PATH"], r"C:\a;C:\b");
        // No PATH anywhere: nothing is added.
        assert!(!with_registry_path(env(&[("X", "1")]), &NoRegistry).contains_key("Path"));
    }

    #[test]
    fn expand_handles_lone_and_empty_percent_signs() {
        let vars = env(&[("A", "1")]);
        assert_eq!(expand("100% %A%", &vars), "100% 1");
        assert_eq!(expand("%%A%", &vars), "%1");
        assert_eq!(expand("%a%-%B%", &vars), "1-%B%");
        assert_eq!(expand("50%", &vars), "50%");
    }

    /// The real registry (Wine or Windows) has a machine PATH with system32.
    #[cfg(windows)]
    #[test]
    fn f_cd_03_the_real_registry_has_a_machine_path() {
        let path = SystemRegistry.machine("Path").expect("machine Path");
        let vars: BTreeMap<String, String> = std::env::vars().collect();
        let value = if path.expand {
            expand(&path.value, &vars)
        } else {
            path.value
        };
        assert!(value.to_lowercase().contains("system32"), "{value}");
    }
}
