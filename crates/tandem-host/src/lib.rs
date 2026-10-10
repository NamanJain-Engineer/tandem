//! `tandem-host`: the helper Vibezeo uploads to SSH hosts (ARCHITECTURE
//! §2.11, §13.1), and the pieces the desktop shares with it: the wire
//! [`protocol`], the [`client`], the [`env`] allowlist, and the [`ssh`]
//! transport that installs and starts it.

pub mod client;
pub mod detect;
pub mod env;
pub mod fsops;
pub mod gitops;
pub mod manifest;
pub mod protocol;
pub mod ssh;

#[cfg(unix)]
pub mod daemon;
#[cfg(unix)]
pub mod serve;

/// POSIX single quotes: `it's` → `'it'\''s'`.
pub fn quote(s: &str) -> String {
    if !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-+=:@,%".contains(&b))
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// A command line for a POSIX shell.
pub fn quote_argv<S: AsRef<str>>(argv: &[S]) -> String {
    argv.iter()
        .map(|a| quote(a.as_ref()))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_rm_01_remote_command_lines_are_quoted() {
        assert_eq!(quote("plain/path-1.0"), "plain/path-1.0");
        assert_eq!(quote("it's"), r"'it'\''s'");
        assert_eq!(quote(""), "''");
        assert_eq!(quote("$(id)"), "'$(id)'");
        assert_eq!(quote_argv(&["claude", "-p", "a b"]), "claude -p 'a b'");
    }
}
