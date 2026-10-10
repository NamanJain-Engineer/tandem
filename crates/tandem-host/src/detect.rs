//! `engines.detect` on the host (F-EN-01 per host): one login shell is
//! asked where each command is (`command -v`), so the host's own PATH
//! setup applies, then each found engine prints its version.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::protocol::{DetectWanted, ErrorKind, Found, RpcError};

/// How long the login shell may take.
pub const SHELL_TIMEOUT: Duration = Duration::from_secs(10);
pub const VERSION_TIMEOUT: Duration = Duration::from_secs(5);

/// A bare command name (registry ids and commands are; nothing to quote).
pub fn bare(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && !name.starts_with(['-', '.'])
}

/// The script the login shell runs: one `id<TAB>path` line per engine
/// found, first matching command wins.
pub fn script(wanted: &[DetectWanted]) -> String {
    let mut s = String::new();
    for w in wanted.iter().filter(|w| bare(&w.id)) {
        let names: Vec<&str> = w
            .commands
            .iter()
            .map(String::as_str)
            .filter(|c| bare(c))
            .collect();
        if names.is_empty() {
            continue;
        }
        s.push_str(&format!(
            "for c in {}; do p=$(command -v \"$c\" 2>/dev/null) && case \"$p\" in /*) printf '%s\\t%s\\n' {} \"$p\"; break;; esac; done\n",
            names.join(" "),
            w.id
        ));
    }
    s
}

pub fn parse(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(id, p)| (id.to_string(), p.to_string()))
        .collect()
}

fn run_with_timeout(mut cmd: Command, limit: Duration) -> Option<String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < limit => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    Some(out)
}

pub fn detect(wanted: &[DetectWanted]) -> Result<Vec<Found>, RpcError> {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| s.starts_with('/'))
        .unwrap_or_else(|| "/bin/sh".into());
    let mut cmd = Command::new(&shell);
    cmd.args(["-lc", &script(wanted)]);
    let out = run_with_timeout(cmd, SHELL_TIMEOUT).ok_or_else(|| {
        RpcError::new(
            ErrorKind::Io,
            "The host's login shell didn't answer in 10 s",
        )
    })?;
    Ok(parse(&out)
        .into_iter()
        .filter_map(|(id, path)| {
            let w = wanted.iter().find(|w| w.id == id)?;
            let version = (!w.version_args.is_empty())
                .then(|| {
                    let mut c = Command::new(&path);
                    c.args(&w.version_args);
                    run_with_timeout(c, VERSION_TIMEOUT)
                })
                .flatten()
                .and_then(|v| {
                    v.lines()
                        .next()
                        .map(|l| l.trim().chars().take(200).collect())
                });
            Some(Found { id, path, version })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_en_01_detection_script_quotes_nothing_it_did_not_check() {
        let wanted = vec![
            DetectWanted {
                id: "claude".into(),
                commands: vec!["claude".into()],
                version_args: vec!["--version".into()],
            },
            DetectWanted {
                id: "evil".into(),
                commands: vec!["x; rm -rf ~".into(), "$(id)".into()],
                version_args: vec![],
            },
        ];
        let s = script(&wanted);
        assert!(s.contains("for c in claude;"));
        assert!(!s.contains("rm -rf"));
        assert!(!s.contains("$(id)"));
        assert_eq!(
            parse("claude\t/usr/local/bin/claude\nnoise\n"),
            [("claude".to_string(), "/usr/local/bin/claude".to_string())]
        );
    }

    #[cfg(unix)]
    #[test]
    fn f_en_01_finds_an_engine_on_the_login_path() {
        let found = detect(&[DetectWanted {
            id: "sh".into(),
            commands: vec!["no-such-engine-xyz".into(), "sh".into()],
            version_args: vec![],
        }])
        .expect("detect");
        assert_eq!(found.len(), 1);
        assert!(found[0].path.ends_with("/sh"));
    }
}
