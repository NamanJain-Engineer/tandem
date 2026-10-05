//! Shell integration (F-CD-10, ARCHITECTURE §5.8): the scripts that make
//! bash, zsh, fish and PowerShell report their cwd and command boundaries,
//! installed under `$DATA/shell-integration/<version>/`, and how each shell
//! is started with them. The scripts read the launch token from
//! `TANDEM_SHELL_TOKEN` and remove it from the environment before anything
//! else runs.

use std::path::{Path, PathBuf};

use tandem_core::shellquote::{quote_path, ShellDialect, UnsafeFilename};

/// Every script, by its path under the install folder.
pub const FILES: &[(&str, &str)] = &[
    (
        "bash/tandem.bash",
        include_str!("../shell/bash/tandem.bash"),
    ),
    ("zsh/.zshenv", include_str!("../shell/zsh/.zshenv")),
    ("zsh/.zprofile", include_str!("../shell/zsh/.zprofile")),
    ("zsh/.zshrc", include_str!("../shell/zsh/.zshrc")),
    ("zsh/.zlogin", include_str!("../shell/zsh/.zlogin")),
    (
        "fish/tandem.fish",
        include_str!("../shell/fish/tandem.fish"),
    ),
    ("pwsh/tandem.ps1", include_str!("../shell/pwsh/tandem.ps1")),
];

/// The variable a zsh pane's shim reads the user's own ZDOTDIR from.
pub const REAL_ZDOTDIR_VAR: &str = "TANDEM_REAL_ZDOTDIR";
/// Set for bash: the rcfile reads the login startup files.
pub const BASH_LOGIN_VAR: &str = "TANDEM_SHELL_LOGIN";

/// The install folder's name: the app version and a hash of the scripts, so
/// a changed script never runs from an older copy.
pub fn version() -> String {
    // FNV-1a over every path and body.
    let mut hash: u32 = 0x811c_9dc5;
    for (path, body) in FILES {
        for b in path.bytes().chain(body.bytes()) {
            hash ^= u32::from(b);
            hash = hash.wrapping_mul(0x0100_0193);
        }
    }
    format!("{}-{hash:08x}", env!("CARGO_PKG_VERSION"))
}

/// Writes the scripts under `root` (when missing or different) and returns
/// the folder that holds them.
pub fn install(root: &Path) -> std::io::Result<PathBuf> {
    let dir = root.join(version());
    for (rel, body) in FILES {
        let path = dir.join(rel);
        if std::fs::read(&path).ok().as_deref() == Some(body.as_bytes()) {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        tandem_core::atomic::atomic_write(&path, body.as_bytes(), Some(0o644))
            .map_err(|e| std::io::Error::other(e.to_string()))?;
    }
    Ok(dir)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegratedShell {
    Bash,
    Zsh,
    Fish,
    Pwsh,
}

impl IntegratedShell {
    /// The shells with scripts; `None` for the rest (sh, dash, cmd…).
    pub fn detect(shell: &Path) -> Option<Self> {
        let name = shell
            .file_stem()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        match name.as_str() {
            "bash" => Some(Self::Bash),
            "zsh" => Some(Self::Zsh),
            "fish" => Some(Self::Fish),
            "pwsh" | "powershell" => Some(Self::Pwsh),
            _ => None,
        }
    }

    /// Its key in the `term_shell_integration` setting.
    pub fn setting_key(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
            Self::Fish => "fish",
            Self::Pwsh => "pwsh",
        }
    }
}

/// How to start an integrated interactive login shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

fn script(dir: &Path, rel: &str) -> String {
    dir.join(rel).to_string_lossy().into_owned()
}

/// Arguments and environment for `kind` with the scripts in `dir`.
/// `user_zdotdir` is the user's own ZDOTDIR, if they set one.
pub fn launch(
    kind: IntegratedShell,
    dir: &Path,
    user_zdotdir: Option<&str>,
) -> Result<Launch, UnsafeFilename> {
    Ok(match kind {
        IntegratedShell::Bash => Launch {
            args: vec![
                "--rcfile".into(),
                script(dir, "bash/tandem.bash"),
                "-i".into(),
            ],
            env: vec![(BASH_LOGIN_VAR.into(), "1".into())],
        },
        IntegratedShell::Zsh => {
            let mut env = vec![("ZDOTDIR".into(), script(dir, "zsh"))];
            if let Some(z) = user_zdotdir.filter(|z| !z.is_empty()) {
                env.push((REAL_ZDOTDIR_VAR.into(), z.into()));
            }
            Launch {
                args: vec!["-l".into()],
                env,
            }
        }
        IntegratedShell::Fish => Launch {
            args: vec![
                "-l".into(),
                "--init-command".into(),
                format!(
                    "source {}",
                    quote_path(&script(dir, "fish/tandem.fish"), ShellDialect::Posix)?
                ),
            ],
            env: vec![],
        },
        IntegratedShell::Pwsh => Launch {
            args: vec![
                "-NoLogo".into(),
                "-NoExit".into(),
                "-Command".into(),
                format!(
                    ". {}",
                    quote_path(&script(dir, "pwsh/tandem.ps1"), ShellDialect::PowerShell)?
                ),
            ],
            env: vec![],
        },
    })
}

/// The end of an agent wrapper (§5.2): the shell that takes the pane back,
/// started with integration and the saved token (`$t`) back in its
/// environment. POSIX shells and fish `exec` it; PowerShell dot-sources the
/// script in place (the wrapper is already that shell).
pub fn handback_tail(
    kind: IntegratedShell,
    shell: &Path,
    dir: &Path,
) -> Result<String, UnsafeFilename> {
    let again = quote_path(&shell.to_string_lossy(), ShellDialect::Posix)?;
    let token = crate::env::TOKEN_VAR;
    Ok(match kind {
        IntegratedShell::Bash => format!(
            "{token}=\"$t\" {BASH_LOGIN_VAR}=1 exec {again} --rcfile {} -i",
            quote_path(&script(dir, "bash/tandem.bash"), ShellDialect::Posix)?
        ),
        IntegratedShell::Zsh => format!(
            "{token}=\"$t\" {REAL_ZDOTDIR_VAR}=\"${{ZDOTDIR-}}\" ZDOTDIR={} exec {again} -l",
            quote_path(&script(dir, "zsh"), ShellDialect::Posix)?
        ),
        IntegratedShell::Fish => {
            let init = format!(
                "source {}",
                quote_path(&script(dir, "fish/tandem.fish"), ShellDialect::Posix)?
            );
            format!(
                "exec env {token}=$t {again} -l --init-command {}",
                quote_path(&init, ShellDialect::Posix)?
            )
        }
        IntegratedShell::Pwsh => format!(
            "$env:{token} = $t; . {}",
            quote_path(&script(dir, "pwsh/tandem.ps1"), ShellDialect::PowerShell)?
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_writes_every_script_once_and_versions_the_folder() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = install(tmp.path()).expect("install");
        assert!(dir.ends_with(version()));
        for (rel, body) in FILES {
            assert_eq!(std::fs::read_to_string(dir.join(rel)).expect("read"), *body);
        }
        // A second install leaves the files alone.
        let before = std::fs::metadata(dir.join("bash/tandem.bash"))
            .and_then(|m| m.modified())
            .expect("mtime");
        install(tmp.path()).expect("again");
        let after = std::fs::metadata(dir.join("bash/tandem.bash"))
            .and_then(|m| m.modified())
            .expect("mtime");
        assert_eq!(before, after);
    }

    #[test]
    fn shells_and_their_launches() {
        let dir = Path::new("/data/si/v1");
        assert_eq!(
            IntegratedShell::detect(Path::new("/bin/bash")),
            Some(IntegratedShell::Bash)
        );
        assert_eq!(
            IntegratedShell::detect(Path::new("/usr/bin/zsh")),
            Some(IntegratedShell::Zsh)
        );
        assert_eq!(
            IntegratedShell::detect(Path::new("C:/x/pwsh.exe")),
            Some(IntegratedShell::Pwsh)
        );
        assert_eq!(IntegratedShell::detect(Path::new("/bin/dash")), None);
        let bash = launch(IntegratedShell::Bash, dir, None).expect("bash");
        assert_eq!(
            bash.args,
            vec!["--rcfile", "/data/si/v1/bash/tandem.bash", "-i"]
        );
        let zsh = launch(IntegratedShell::Zsh, dir, Some("/home/a/.config/zsh")).expect("zsh");
        assert_eq!(
            zsh.env,
            vec![
                ("ZDOTDIR".to_string(), "/data/si/v1/zsh".to_string()),
                (
                    REAL_ZDOTDIR_VAR.to_string(),
                    "/home/a/.config/zsh".to_string()
                )
            ]
        );
        let fish = launch(IntegratedShell::Fish, dir, None).expect("fish");
        assert_eq!(fish.args[2], "source '/data/si/v1/fish/tandem.fish'");
    }

    #[test]
    fn handback_tails_restore_the_token_for_the_new_shell() {
        let dir = Path::new("/d/si");
        let bash = handback_tail(IntegratedShell::Bash, Path::new("/bin/bash"), dir).expect("bash");
        assert_eq!(
            bash,
            "TANDEM_SHELL_TOKEN=\"$t\" TANDEM_SHELL_LOGIN=1 exec '/bin/bash' --rcfile '/d/si/bash/tandem.bash' -i"
        );
        let zsh = handback_tail(IntegratedShell::Zsh, Path::new("/bin/zsh"), dir).expect("zsh");
        assert!(zsh.contains(
            "TANDEM_REAL_ZDOTDIR=\"${ZDOTDIR-}\" ZDOTDIR='/d/si/zsh' exec '/bin/zsh' -l"
        ));
    }
}
