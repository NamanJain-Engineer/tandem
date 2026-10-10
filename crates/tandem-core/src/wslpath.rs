//! WSL (F-RM-02, ARCHITECTURE §13.2, §5.10): path translation both ways
//! (drops into WSL panes, §5.9; Files reading a distro from Windows), the
//! `wsl.exe -l` output (UTF-16LE), and the spawn arguments. Pure functions,
//! tested on every platform; the `wsl.exe` calls live in the app.

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

/// The Windows path for a distro's Linux path (Files reads it from
/// Windows): `/mnt/c/x y` → `C:\x y`, anything else →
/// `\\wsl.localhost\<distro>\…`. `None` for a relative path.
pub fn from_wsl_path(distro: &str, linux: &str) -> Option<String> {
    if !linux.starts_with('/') || distro.is_empty() || distro.contains(['\\', '/']) {
        return None;
    }
    let mut parts = linux.trim_end_matches('/').splitn(4, '/');
    let _root = parts.next();
    if let (Some("mnt"), Some(drive)) = (parts.next(), parts.next()) {
        if drive.len() == 1 && drive.as_bytes()[0].is_ascii_alphabetic() {
            let rest = parts.next().unwrap_or("").replace('/', "\\");
            return Some(format!("{}:\\{rest}", drive.to_ascii_uppercase()));
        }
    }
    let rest = linux.trim_start_matches('/').replace('/', "\\");
    Some(
        format!("\\\\wsl.localhost\\{distro}\\{rest}")
            .trim_end_matches('\\')
            .to_string(),
    )
}

/// One distribution as `wsl.exe -l -v` lists it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Distro {
    pub name: String,
    pub is_default: bool,
    /// WSL 1 or 2, when `-l -v` said.
    pub version: Option<u8>,
}

/// `wsl.exe` writes UTF-16LE (with or without a BOM); an older or
/// redirected one may write UTF-8. Either way: text with no NULs.
pub fn decode_wsl_output(raw: &[u8]) -> String {
    let utf16 = raw.len() >= 2
        && (raw.starts_with(&[0xFF, 0xFE])
            || raw.iter().skip(1).step_by(2).take(16).all(|b| *b == 0));
    let text = if utf16 {
        let units: Vec<u16> = raw
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(raw).into_owned()
    };
    text.replace(['\u{feff}', '\0'], "")
}

/// `wsl -l -q`: one distro name per line.
pub fn parse_distro_names(raw: &[u8]) -> Vec<String> {
    decode_wsl_output(raw)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// `wsl -l -v`: `* Ubuntu   Running   2` (the star marks the default), after
/// a header line.
pub fn parse_distros_verbose(raw: &[u8]) -> Vec<Distro> {
    decode_wsl_output(raw)
        .lines()
        .skip(1)
        .filter_map(|l| {
            let l = l.trim_end();
            let (is_default, rest) = match l.trim_start().strip_prefix('*') {
                Some(r) => (true, r),
                None => (false, l),
            };
            let mut words: Vec<&str> = rest.split_whitespace().collect();
            let version = words.last().and_then(|v| v.parse::<u8>().ok());
            if version.is_some() {
                words.pop();
                words.pop(); // the state (Running, Stopped)
            }
            let name = words.join(" ");
            (!name.is_empty()).then_some(Distro {
                name,
                is_default,
                version,
            })
        })
        .collect()
}

/// A distro name `wsl.exe -d` can be given: no separators, no option.
pub fn valid_distro(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// How a WSL pane or engine starts: `wsl.exe -d <distro> --cd <path> --
/// <argv>`. No argv: the distro's login shell.
pub fn spawn_args(distro: &str, linux_cwd: &str, argv: &[String]) -> Vec<String> {
    let mut args = vec![
        "-d".to_string(),
        distro.to_string(),
        "--cd".to_string(),
        linux_cwd.to_string(),
    ];
    if !argv.is_empty() {
        args.push("--".into());
        args.extend(argv.iter().cloned());
    }
    args
}

/// Variables that cross from Windows into the distro (`WSLENV`): only
/// Vibezeo's pane id, status and shell-integration ones, never a provider
/// home or anything else of Windows' environment.
pub const WSLENV: &str =
    "TANDEM_PANE_ID/u:TANDEM_STATUS_URL/u:TANDEM_STATUS_SECRET/u:TANDEM_SHELL_TOKEN/u";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_rm_02_2_drives_and_wsl_shares_become_linux_paths() {
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

    #[test]
    fn f_rm_02_2_linux_paths_go_back_to_windows() {
        assert_eq!(
            from_wsl_path("Ubuntu", "/mnt/c/x y/z").as_deref(),
            Some(r"C:\x y\z")
        );
        assert_eq!(from_wsl_path("Ubuntu", "/mnt/d").as_deref(), Some(r"D:\"));
        assert_eq!(
            from_wsl_path("Ubuntu", "/home/a/my shop").as_deref(),
            Some(r"\\wsl.localhost\Ubuntu\home\a\my shop")
        );
        assert_eq!(
            from_wsl_path("Ubuntu", "/").as_deref(),
            Some(r"\\wsl.localhost\Ubuntu")
        );
        assert_eq!(from_wsl_path("Ubuntu", "relative"), None);
        assert_eq!(from_wsl_path(r"..\x", "/home"), None);
        // Both directions agree.
        for linux in ["/mnt/c/x y/z", "/home/a/b"] {
            let win = from_wsl_path("Ubuntu", linux).expect("win");
            assert_eq!(to_wsl_path(&win).as_deref(), Some(linux));
        }
    }

    fn utf16(s: &str, bom: bool) -> Vec<u8> {
        let mut out = if bom { vec![0xFF, 0xFE] } else { Vec::new() };
        for u in s.encode_utf16() {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out
    }

    #[test]
    fn f_rm_02_wsl_list_output_is_utf16() {
        for bom in [true, false] {
            let raw = utf16("Ubuntu-22.04\r\nDebian\r\n\r\n", bom);
            assert_eq!(parse_distro_names(&raw), ["Ubuntu-22.04", "Debian"]);
        }
        // A UTF-8 writer (redirected output on some builds) reads too.
        assert_eq!(
            parse_distro_names(b"Ubuntu\nkali-linux\n"),
            ["Ubuntu", "kali-linux"]
        );
        let verbose = utf16(
            "  NAME                   STATE           VERSION\r\n* Ubuntu                 Running         2\r\n  Debian                 Stopped         1\r\n",
            true,
        );
        assert_eq!(
            parse_distros_verbose(&verbose),
            [
                Distro {
                    name: "Ubuntu".into(),
                    is_default: true,
                    version: Some(2)
                },
                Distro {
                    name: "Debian".into(),
                    is_default: false,
                    version: Some(1)
                },
            ]
        );
    }

    #[test]
    fn f_rm_02_spawn_arguments() {
        assert_eq!(
            spawn_args("Ubuntu", "/home/a/x y", &[]),
            ["-d", "Ubuntu", "--cd", "/home/a/x y"]
        );
        assert_eq!(
            spawn_args("Ubuntu", "/home/a", &["claude".into(), "-p".into()]),
            ["-d", "Ubuntu", "--cd", "/home/a", "--", "claude", "-p"]
        );
        assert!(valid_distro("Ubuntu-22.04"));
        assert!(!valid_distro("-d"));
        assert!(!valid_distro("a b"));
        assert!(!valid_distro(""));
        assert!(WSLENV
            .split(':')
            .all(|v| v.starts_with("TANDEM_") && v.ends_with("/u")));
    }
}
