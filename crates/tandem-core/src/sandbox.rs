//! Running inside a Flatpak (F-RM-07, D-2032). The person's shells, coding
//! CLIs and tools live on the host, not in the sandbox, so terminals,
//! threads, login-env capture and engine detection start through
//! `flatpak-spawn --host` with exactly the environment and folder they'd get
//! outside it. Git is bundled in the Flatpak instead. Outside a Flatpak
//! nothing changes.

use std::ffi::{OsStr, OsString};
use std::path::Path;

/// Whether this process runs inside a Flatpak sandbox.
pub fn in_flatpak() -> bool {
    std::env::var_os("FLATPAK_ID").is_some() && Path::new("/.flatpak-info").exists()
}

/// The argv that runs `program args` on the host: in `cwd` when given, with
/// exactly `env` when given (otherwise the host's own environment). The
/// first element is `flatpak-spawn`.
pub fn flatpak_host_argv<K, V>(
    program: &OsStr,
    args: &[OsString],
    cwd: Option<&Path>,
    env: Option<&[(K, V)]>,
) -> Vec<OsString>
where
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    let mut out: Vec<OsString> = vec![
        "flatpak-spawn".into(),
        "--host".into(),
        "--watch-bus".into(),
    ];
    if let Some(dir) = cwd {
        let mut d = OsString::from("--directory=");
        d.push(dir.as_os_str());
        out.push(d);
    }
    if let Some(vars) = env {
        out.push("--clear-env".into());
        for (k, v) in vars {
            let mut e = OsString::from("--env=");
            e.push(k.as_ref());
            e.push("=");
            e.push(v.as_ref());
            out.push(e);
        }
    }
    out.push(program.to_os_string());
    out.extend(args.iter().cloned());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_rm_07_3_host_commands_carry_their_folder_and_exact_environment() {
        let argv = flatpak_host_argv(
            OsStr::new("/bin/zsh"),
            &["-l".into()],
            Some(Path::new("/home/me/code")),
            Some(&[("TERM", "xterm-256color"), ("TANDEM_PANE_ID", "pn_1")]),
        );
        let argv: Vec<String> = argv
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            argv,
            [
                "flatpak-spawn",
                "--host",
                "--watch-bus",
                "--directory=/home/me/code",
                "--clear-env",
                "--env=TERM=xterm-256color",
                "--env=TANDEM_PANE_ID=pn_1",
                "/bin/zsh",
                "-l"
            ]
        );
        let bare =
            flatpak_host_argv::<&str, &str>(OsStr::new("npm"), &["prefix".into()], None, None);
        assert_eq!(bare.len(), 5);
        assert_eq!(bare[3], "npm");
    }
}
