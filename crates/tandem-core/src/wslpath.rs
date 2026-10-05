//! Windows → WSL path translation for drops into WSL panes (§5.9, F-RM-02).

/// `C:\x y\z` → `/mnt/c/x y/z`; `\\wsl$\<distro>\home\a` and
/// `\\wsl.localhost\<distro>\home\a` → `/home/a`. Paths already in POSIX
/// form pass through. `None` for paths with no WSL equivalent (other UNC
/// shares, relative paths).
pub fn to_wsl_path(p: &str) -> Option<String> {
    if p.starts_with('/') {
        return Some(p.to_string());
    }
    let unc = p
        .strip_prefix(r"\\wsl$\")
        .or_else(|| p.strip_prefix(r"\\wsl.localhost\"));
    if let Some(rest) = unc {
        // Drop the distro name; what follows is the distro's own root.
        let after = rest.split_once('\\').map(|(_, path)| path).unwrap_or("");
        return Some(format!("/{}", after.replace('\\', "/")));
    }
    let bytes = p.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let drive = (bytes[0] as char).to_ascii_lowercase();
        let rest = p[2..].trim_start_matches(['\\', '/']).replace('\\', "/");
        return Some(if rest.is_empty() {
            format!("/mnt/{drive}")
        } else {
            format!("/mnt/{drive}/{rest}")
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wslpath_f_rm_02_2_drives_and_wsl_shares() {
        assert_eq!(to_wsl_path(r"C:\x y\z").as_deref(), Some("/mnt/c/x y/z"));
        assert_eq!(to_wsl_path(r"d:\").as_deref(), Some("/mnt/d"));
        assert_eq!(
            to_wsl_path(r"\\wsl$\Ubuntu\home\a").as_deref(),
            Some("/home/a")
        );
        assert_eq!(
            to_wsl_path(r"\\wsl.localhost\Ubuntu-22.04\home\a\b c").as_deref(),
            Some("/home/a/b c")
        );
        assert_eq!(
            to_wsl_path("/already/posix").as_deref(),
            Some("/already/posix")
        );
        assert_eq!(to_wsl_path(r"\\server\share\x"), None);
        assert_eq!(to_wsl_path("relative\\x"), None);
    }

    #[test]
    fn wslpath_then_quote_for_a_wsl_pane() {
        use crate::shellquote::{quote_path, ShellDialect};
        let p = to_wsl_path(r"C:\Users\Ana\it's here").expect("path");
        assert_eq!(
            quote_path(&p, ShellDialect::Posix).as_deref(),
            Ok("'/mnt/c/Users/Ana/it'\\''s here'")
        );
    }
}
