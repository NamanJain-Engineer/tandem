//! Quoting paths for drops into terminals (ARCHITECTURE §5.9, F-CD-06).
//! Every dialect refuses control characters, so a dropped name can never
//! inject a newline (and so never runs anything). Nothing appends Enter.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ShellDialect {
    /// bash, zsh, fish, sh
    Posix,
    PowerShell,
    Cmd,
}

impl ShellDialect {
    /// The dialect for a shell program path.
    pub fn for_shell(program: &str) -> Self {
        let name = program
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(program)
            .trim_end_matches(".exe")
            .to_ascii_lowercase();
        match name.as_str() {
            "pwsh" | "powershell" => ShellDialect::PowerShell,
            "cmd" => ShellDialect::Cmd,
            _ => ShellDialect::Posix,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("This file name can't be inserted safely")]
pub struct UnsafeFilename;

impl From<UnsafeFilename> for crate::IpcError {
    fn from(e: UnsafeFilename) -> Self {
        crate::IpcError::new(crate::ErrorCode::UnsafeFilename, e.to_string())
    }
}

fn has_control(s: &str) -> bool {
    s.chars().any(|c| (c as u32) < 0x20 || c == '\u{7f}')
}

/// One quoted path for `dialect`.
pub fn quote_path(p: &str, dialect: ShellDialect) -> Result<String, UnsafeFilename> {
    if p.is_empty() || has_control(p) {
        return Err(UnsafeFilename);
    }
    Ok(match dialect {
        ShellDialect::Posix => format!("'{}'", p.replace('\'', "'\\''")),
        ShellDialect::PowerShell => format!("'{}'", p.replace('\'', "''")),
        ShellDialect::Cmd => {
            if p.contains(['"', '%', '!', '^', '`']) {
                return Err(UnsafeFilename);
            }
            format!("\"{p}\"")
        }
    })
}

/// Several paths, space-joined, no trailing newline (`term_quote_paths`).
/// Any unsafe name refuses the whole drop.
pub fn quote_paths<S: AsRef<str>>(
    paths: &[S],
    dialect: ShellDialect,
) -> Result<String, UnsafeFilename> {
    let quoted = paths
        .iter()
        .map(|p| quote_path(p.as_ref(), dialect))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(quoted.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shellquote_f_cd_06_2_posix_powershell_cmd() {
        assert_eq!(
            quote_path("a b'c", ShellDialect::Posix).as_deref(),
            Ok("'a b'\\''c'")
        );
        assert_eq!(
            quote_path("a b'c", ShellDialect::PowerShell).as_deref(),
            Ok("'a b''c'")
        );
        assert_eq!(
            quote_path("C:\\x y", ShellDialect::Cmd).as_deref(),
            Ok("\"C:\\x y\"")
        );
        assert_eq!(
            quote_path("$HOME/`x`;rm", ShellDialect::Posix).as_deref(),
            Ok("'$HOME/`x`;rm'")
        );
        for bad in ["50%", "a\"b", "x!y", "a^b", "a`b"] {
            assert!(quote_path(bad, ShellDialect::Cmd).is_err(), "{bad}");
        }
    }

    #[test]
    fn shellquote_f_cd_06_3_control_characters_are_refused() {
        for bad in ["bad\nname", "tab\there", "nul\0", "del\u{7f}", "\r", ""] {
            for d in [
                ShellDialect::Posix,
                ShellDialect::PowerShell,
                ShellDialect::Cmd,
            ] {
                assert_eq!(quote_path(bad, d), Err(UnsafeFilename), "{bad:?} {d:?}");
            }
        }
        assert!(
            quote_paths(&["ok", "bad\nname"], ShellDialect::Posix).is_err(),
            "one bad name refuses the drop"
        );
    }

    #[test]
    fn shellquote_joins_without_a_trailing_newline() {
        let out = quote_paths(&["/a b", "/c"], ShellDialect::Posix).expect("quoted");
        assert_eq!(out, "'/a b' '/c'");
        assert!(!out.ends_with('\n'));
    }

    #[test]
    fn shellquote_dialect_from_program() {
        assert_eq!(ShellDialect::for_shell("/bin/zsh"), ShellDialect::Posix);
        assert_eq!(
            ShellDialect::for_shell("C:\\Program Files\\PowerShell\\7\\pwsh.exe"),
            ShellDialect::PowerShell
        );
        assert_eq!(
            ShellDialect::for_shell("C:\\Windows\\System32\\cmd.exe"),
            ShellDialect::Cmd
        );
    }
}
