//! Safe path quoting for drops into terminals (§5.9, F-CD-06).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellDialect {
    Posix,
    PowerShell,
    Cmd,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("This file name can't be inserted safely")]
pub struct UnsafeFilename;

pub fn quote_path(p: &str, dialect: ShellDialect) -> Result<String, UnsafeFilename> {
    if p.chars().any(|c| (c as u32) < 0x20 || c == '\u{7f}') {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_per_dialect_and_refuses_control_chars() {
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
        assert!(quote_path("bad\nname", ShellDialect::Posix).is_err());
        assert!(quote_path("50%", ShellDialect::Cmd).is_err());
    }
}
