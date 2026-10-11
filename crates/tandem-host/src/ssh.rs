//! The SSH transport (ARCHITECTURE §13.1, F-RM-01): the system `ssh` with
//! one ControlMaster per host (`ControlPath=$DATA/run/ssh-%C`), host keys
//! checked strictly against Vibezeo's own `known_hosts` (read through ssh
//! itself for hosts behind `ProxyJump` or `ProxyCommand`), and `tandem-host`
//! installed into `~/.tandem-host/<protocol>/` with its SHA-256 checked on
//! the host before use.
//!
//! The master is started on its own (`-M -N -f`) with its log in a file:
//! a master that `ControlMaster=auto` forks keeps the caller's stderr pipe
//! open and the caller would wait forever. Every other command joins it
//! (`ControlMaster=no`).
//!
//! Windows' own OpenSSH has no connection sharing (no ControlMaster), so
//! there every command opens its own connection and "starting the master"
//! only checks that the host answers ([`Ssh::multiplex`]).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};

use crate::manifest::{platform, Manifest};
use crate::protocol::PROTOCOL;

/// Where the host keeps its binary, relative to the remote home.
pub fn remote_dir() -> String {
    format!(".tandem-host/{PROTOCOL}")
}

/// The remote binary, as a shell word (the remote shell expands `$HOME`).
pub fn remote_bin() -> String {
    format!("\"$HOME\"/{}/tandem-host", remote_dir())
}

pub const CONNECT_TIMEOUT_SECS: u32 = 10;
pub const EXEC_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshTarget {
    /// An alias from `~/.ssh/config`, a host name or `user@host`.
    pub target: String,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub identity_file: Option<String>,
}

impl SshTarget {
    /// `user@host:port` (or an alias) into its parts.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() || text.starts_with('-') || text.contains(char::is_whitespace) {
            return None;
        }
        let (user, rest) = match text.rsplit_once('@') {
            Some((u, r)) if !u.is_empty() => (Some(u.to_string()), r),
            Some(_) => return None,
            None => (None, text),
        };
        let (host, port) = match rest.rsplit_once(':') {
            // `[::1]:22` and bare IPv6 addresses.
            Some((h, p)) if !h.contains(':') || h.ends_with(']') => (
                h.trim_matches(['[', ']']).to_string(),
                Some(p.parse::<u16>().ok()?),
            ),
            _ => (rest.trim_matches(['[', ']']).to_string(), None),
        };
        if host.is_empty() || host.starts_with('-') {
            return None;
        }
        Some(Self {
            target: host,
            port,
            user,
            identity_file: None,
        })
    }

    pub fn label(&self) -> String {
        let mut s = String::new();
        if let Some(u) = &self.user {
            s.push_str(u);
            s.push('@');
        }
        s.push_str(&self.target);
        if let Some(p) = self.port.filter(|p| *p != 22) {
            s.push_str(&format!(":{p}"));
        }
        s
    }
}

/// What goes wrong, in the words F-RM-01 asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Unreachable,
    HostKey,
    KeyFile,
    SignIn,
    FolderMissing,
    UnsupportedArch,
    InstallFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct SshError {
    pub category: Category,
    pub message: String,
}

impl SshError {
    pub fn new(category: Category, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }
}

/// The category for what ssh printed when it failed.
pub fn classify(stderr: &str) -> Category {
    let s = stderr;
    if s.contains("REMOTE HOST IDENTIFICATION HAS CHANGED")
        || s.contains("Host key verification failed")
        || s.contains("No ED25519 host key is known")
        || s.contains("host key for")
    {
        Category::HostKey
    } else if s.contains("no such identity")
        || s.contains("Load key")
        || s.contains("UNPROTECTED PRIVATE KEY FILE")
        || s.contains("invalid format")
        || s.contains("Identity file")
    {
        Category::KeyFile
    } else if s.contains("Permission denied") || s.contains("Too many authentication failures") {
        Category::SignIn
    } else {
        Category::Unreachable
    }
}

/// The user-facing sentence for a category.
pub fn message(category: Category, host: &str, detail: &str) -> String {
    match category {
        Category::Unreachable => {
            format!("Couldn't reach {host}. Check the address and that it's online.")
        }
        Category::HostKey => {
            format!("The host key for {host} changed. Vibezeo won't connect until you verify it.")
        }
        Category::KeyFile => format!("The key file for {host} couldn't be used. {detail}"),
        Category::SignIn => {
            format!("{host} didn't accept the sign-in. Check the user name, key or password.")
        }
        Category::FolderMissing => format!("That folder doesn't exist on {host}."),
        Category::UnsupportedArch => format!("Vibezeo can't run its helper on {host} ({detail})."),
        Category::InstallFailed => format!("Couldn't install the helper on {host}. {detail}"),
    }
    .trim_end()
    .to_string()
}

fn last_lines(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("debug"))
        .collect();
    lines[lines.len().saturating_sub(3)..].join(" ")
}

fn fail(stderr: &str, host: &str) -> SshError {
    let cat = classify(stderr);
    SshError::new(cat, message(cat, host, &last_lines(stderr)))
}

/// What `ssh -G` resolved a target to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub hostname: String,
    pub port: u16,
    pub user: String,
    /// `ProxyJump` or `ProxyCommand` when the user's config reaches the
    /// host through one (D-1603).
    pub via: Option<&'static str>,
}

pub fn parse_g(out: &str) -> Option<Resolved> {
    let mut hostname = None;
    let mut port = 22;
    let mut user = String::new();
    for line in out.lines() {
        match line.split_once(' ') {
            Some(("hostname", v)) => hostname = Some(v.trim().to_string()),
            Some(("port", v)) => port = v.trim().parse().unwrap_or(22),
            Some(("user", v)) => user = v.trim().to_string(),
            _ => {}
        }
    }
    Some(Resolved {
        hostname: hostname?,
        port,
        user,
        via: proxied(out),
    })
}

/// How `ssh -G` says the host is reached when it isn't directly:
/// `ProxyJump` or `ProxyCommand`. `ssh-keyscan` connects directly, so such
/// a host's key is read through ssh itself (`Ssh::host_keys`, D-1603).
pub fn proxied(g_out: &str) -> Option<&'static str> {
    for line in g_out.lines() {
        match line.split_once(' ') {
            Some(("proxyjump", v)) if v.trim() != "none" => return Some("ProxyJump"),
            Some(("proxycommand", v)) if v.trim() != "none" => return Some("ProxyCommand"),
            _ => {}
        }
    }
    None
}

/// A host key as `ssh-keyscan` printed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostKey {
    /// The `known_hosts` line.
    pub line: String,
    pub key_type: String,
    /// `SHA256:…`, as `ssh-keygen -lf` shows it.
    pub fingerprint: String,
}

pub fn fingerprint(key_b64: &str) -> Option<String> {
    let blob = base64::engine::general_purpose::STANDARD
        .decode(key_b64)
        .ok()?;
    let digest = Sha256::digest(&blob);
    Some(format!(
        "SHA256:{}",
        base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest)
    ))
}

/// The keys in `ssh-keyscan` output, strongest first.
pub fn parse_keyscan(out: &str) -> Vec<HostKey> {
    let mut keys: Vec<HostKey> = out
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let (host, kt, key) = (f.next()?, f.next()?, f.next()?);
            Some(HostKey {
                line: format!("{host} {kt} {key}"),
                key_type: kt.to_string(),
                fingerprint: fingerprint(key)?,
            })
        })
        .collect();
    let rank = |t: &str| match t {
        "ssh-ed25519" => 0,
        t if t.starts_with("ecdsa") => 1,
        "rsa-sha2-512" | "rsa-sha2-256" | "ssh-rsa" => 2,
        _ => 3,
    };
    keys.sort_by_key(|k| rank(&k.key_type));
    keys
}

/// How a host is named in `known_hosts`.
pub fn known_name(r: &Resolved) -> String {
    if r.port == 22 {
        r.hostname.clone()
    } else {
        format!("[{}]:{}", r.hostname, r.port)
    }
}

/// The fingerprints `known_hosts` holds for `r`.
pub fn known_keys(known_hosts: &str, r: &Resolved) -> Vec<String> {
    let name = known_name(r);
    known_hosts
        .lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let hosts = f.next()?;
            let _kt = f.next()?;
            let key = f.next()?;
            hosts
                .split(',')
                .any(|h| h == name)
                .then(|| fingerprint(key))
                .flatten()
        })
        .collect()
}

/// The askpass helper: `SSH_ASKPASS=<program>` and its own variables.
#[derive(Debug, Clone)]
pub struct Askpass {
    pub program: PathBuf,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct Ssh {
    pub ssh: PathBuf,
    pub keyscan: PathBuf,
    /// `$DATA/run`: control sockets and master logs.
    pub run_dir: PathBuf,
    /// `$DATA/ssh/known_hosts`.
    pub known_hosts: PathBuf,
    pub askpass: Option<Askpass>,
    /// `ssh -F`: a config file instead of the user's own (tests).
    pub config: Option<PathBuf>,
    /// One shared connection per host (ControlMaster). On by default except
    /// on Windows, whose OpenSSH doesn't support it.
    pub multiplex: bool,
}

/// Names each host key probe's throwaway `known_hosts`.
static PROBES: AtomicU64 = AtomicU64::new(0);

/// What `install` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub platform: String,
    /// `false` when the right version was already there.
    pub uploaded: bool,
}

impl Ssh {
    pub fn new(run_dir: PathBuf, known_hosts: PathBuf) -> Self {
        Self {
            ssh: PathBuf::from("ssh"),
            keyscan: PathBuf::from("ssh-keyscan"),
            run_dir,
            known_hosts,
            askpass: None,
            config: None,
            multiplex: !cfg!(windows),
        }
    }

    pub fn control_path(&self) -> PathBuf {
        self.run_dir.join("ssh-%C")
    }

    /// Every option before the target.
    pub fn options(&self, t: &SshTarget, master: bool) -> Vec<String> {
        let mut o: Vec<String> = Vec::new();
        if let Some(c) = &self.config {
            o.push("-F".into());
            o.push(c.display().to_string());
        }
        let mut opt = |kv: String| {
            o.push("-o".into());
            o.push(kv);
        };
        if self.multiplex {
            opt(format!(
                "ControlMaster={}",
                if master { "yes" } else { "no" }
            ));
            opt(format!("ControlPath={}", self.control_path().display()));
            opt("ControlPersist=10m".into());
        }
        opt("StrictHostKeyChecking=yes".into());
        opt(format!("UserKnownHostsFile={}", self.known_hosts.display()));
        opt("GlobalKnownHostsFile=/dev/null".into());
        opt("UpdateHostKeys=no".into());
        opt(format!("ConnectTimeout={CONNECT_TIMEOUT_SECS}"));
        opt("ServerAliveInterval=15".into());
        opt("ServerAliveCountMax=3".into());
        opt("ForwardAgent=no".into());
        opt("ForwardX11=no".into());
        opt("ClearAllForwardings=yes".into());
        opt("PermitLocalCommand=no".into());
        opt("LogLevel=ERROR".into());
        if self.askpass.is_some() {
            opt("NumberOfPasswordPrompts=1".into());
        } else {
            opt("BatchMode=yes".into());
        }
        if let Some(key) = &t.identity_file {
            o.push("-i".into());
            o.push(key.clone());
            o.push("-o".into());
            o.push("IdentitiesOnly=yes".into());
        }
        if let Some(p) = t.port {
            o.push("-p".into());
            o.push(p.to_string());
        }
        if let Some(u) = &t.user {
            o.push("-l".into());
            o.push(u.clone());
        }
        o
    }

    fn base(&self, program: &Path) -> Command {
        let mut cmd = Command::new(program);
        // Vibezeo's own variables stay local, whatever `SendEnv` says.
        for (k, _) in std::env::vars() {
            if k.starts_with("TANDEM_") {
                cmd.env_remove(k);
            }
        }
        if let Some(a) = &self.askpass {
            cmd.env("SSH_ASKPASS", &a.program)
                .env("SSH_ASKPASS_REQUIRE", "force")
                .envs(a.env.iter().map(|(k, v)| (k, v)));
        }
        cmd.kill_on_drop(true);
        cmd
    }

    /// `ssh <options> <target> -- <remote>`, joining the master.
    pub fn command(&self, t: &SshTarget, remote: &str) -> Command {
        let mut cmd = self.base(&self.ssh);
        cmd.args(self.options(t, false))
            .arg("-T")
            .arg("--")
            .arg(&t.target)
            .arg(remote);
        cmd
    }

    pub async fn resolve(&self, t: &SshTarget) -> Result<Resolved, SshError> {
        let mut cmd = self.base(&self.ssh);
        cmd.arg("-G")
            .args(self.options(t, false))
            .arg("--")
            .arg(&t.target);
        let out =
            cmd.stdin(Stdio::null()).output().await.map_err(|e| {
                SshError::new(Category::Unreachable, format!("Couldn't run ssh: {e}"))
            })?;
        let g = String::from_utf8_lossy(&out.stdout);
        parse_g(&g).ok_or_else(|| fail(&String::from_utf8_lossy(&out.stderr), &t.label()))
    }

    /// The host's keys, for approval before the first connection:
    /// `ssh-keyscan` for a host reached directly, else ssh itself through
    /// the user's `ProxyJump` or `ProxyCommand` (D-1603). That connection
    /// reaches the jump host as the user's config says and offers the host
    /// no sign-in at all (every method off, no agent, no forwarding), so all
    /// it learns is the key the host showed, which `accept-new` writes to a
    /// throwaway `known_hosts`. Nothing is trusted until the user approves.
    pub async fn host_keys(&self, t: &SshTarget, r: &Resolved) -> Result<Vec<HostKey>, SshError> {
        let Some(via) = r.via else {
            return self.scan(r).await;
        };
        std::fs::create_dir_all(&self.run_dir)
            .map_err(|e| SshError::new(Category::Unreachable, e.to_string()))?;
        let file = self.run_dir.join(format!(
            "probe-{}-{}",
            std::process::id(),
            PROBES.fetch_add(1, Ordering::Relaxed)
        ));
        let mut cmd = self.base(&self.ssh);
        if let Some(c) = &self.config {
            cmd.arg("-F").arg(c);
        }
        for kv in [
            "ControlMaster=no".to_string(),
            "ControlPath=none".into(),
            "StrictHostKeyChecking=accept-new".into(),
            format!("UserKnownHostsFile={}", file.display()),
            "GlobalKnownHostsFile=/dev/null".into(),
            "HashKnownHosts=no".into(),
            "UpdateHostKeys=no".into(),
            "CheckHostIP=no".into(),
            "BatchMode=yes".into(),
            "PubkeyAuthentication=no".into(),
            "PasswordAuthentication=no".into(),
            "KbdInteractiveAuthentication=no".into(),
            "HostbasedAuthentication=no".into(),
            "GSSAPIAuthentication=no".into(),
            "ForwardAgent=no".into(),
            "ForwardX11=no".into(),
            "ClearAllForwardings=yes".into(),
            "PermitLocalCommand=no".into(),
            "LogLevel=ERROR".into(),
            format!("ConnectTimeout={CONNECT_TIMEOUT_SECS}"),
        ] {
            cmd.arg("-o").arg(kv);
        }
        if let Some(p) = t.port {
            cmd.arg("-p").arg(p.to_string());
        }
        if let Some(u) = &t.user {
            cmd.arg("-l").arg(u);
        }
        cmd.args(["-T", "--", &t.target, "true"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let out = tokio::time::timeout(EXEC_TIMEOUT, cmd.output()).await;
        let seen = std::fs::read_to_string(&file).unwrap_or_default();
        let _ = std::fs::remove_file(&file);
        let name = known_name(r);
        let keys: Vec<HostKey> = parse_keyscan(&seen)
            .into_iter()
            .map(|k| HostKey {
                line: match k.line.split_once(' ') {
                    Some((_, rest)) => format!("{name} {rest}"),
                    None => k.line,
                },
                ..k
            })
            .collect();
        if !keys.is_empty() {
            return Ok(keys);
        }
        let stderr = match out {
            Ok(Ok(o)) => String::from_utf8_lossy(&o.stderr).into_owned(),
            Ok(Err(e)) => format!("Couldn't run ssh: {e}"),
            Err(_) => String::new(),
        };
        Err(SshError::new(
            Category::Unreachable,
            format!(
                "Couldn't reach {} through {via} in your SSH config. {}",
                t.label(),
                last_lines(&stderr)
            )
            .trim_end()
            .to_string(),
        ))
    }

    /// `ssh-keyscan`: the keys of a host reached directly.
    pub async fn scan(&self, r: &Resolved) -> Result<Vec<HostKey>, SshError> {
        let out = Command::new(&self.keyscan)
            .args([
                "-T",
                "10",
                "-p",
                &r.port.to_string(),
                "-t",
                "ed25519,ecdsa,rsa",
                "--",
                &r.hostname,
            ])
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|e| {
                SshError::new(
                    Category::Unreachable,
                    format!("Couldn't run ssh-keyscan: {e}"),
                )
            })?;
        let keys = parse_keyscan(&String::from_utf8_lossy(&out.stdout));
        if keys.is_empty() {
            return Err(SshError::new(
                Category::Unreachable,
                message(Category::Unreachable, &r.hostname, ""),
            ));
        }
        Ok(keys)
    }

    /// The keys pinned for `r`.
    pub fn known(&self, r: &Resolved) -> Vec<String> {
        std::fs::read_to_string(&self.known_hosts)
            .map(|k| known_keys(&k, r))
            .unwrap_or_default()
    }

    /// Pins the approved key: only it is trusted for this host afterwards.
    pub fn trust(&self, r: &Resolved, key: &HostKey) -> std::io::Result<()> {
        if let Some(dir) = self.known_hosts.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let name = known_name(r);
        let existing = std::fs::read_to_string(&self.known_hosts).unwrap_or_default();
        let mut kept: Vec<&str> = existing
            .lines()
            .filter(|l| {
                l.split_whitespace()
                    .next()
                    .is_none_or(|h| !h.split(',').any(|x| x == name))
            })
            .collect();
        kept.push(&key.line);
        std::fs::write(&self.known_hosts, kept.join("\n") + "\n")
    }

    /// Forgets the keys pinned for `r` (the user chose to trust a new key;
    /// the next connection asks again).
    pub fn forget(&self, r: &Resolved) -> std::io::Result<()> {
        let name = known_name(r);
        let existing = match std::fs::read_to_string(&self.known_hosts) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        let kept: Vec<&str> = existing
            .lines()
            .filter(|l| {
                l.split_whitespace()
                    .next()
                    .is_none_or(|h| !h.split(',').any(|x| x == name))
            })
            .collect();
        std::fs::write(&self.known_hosts, kept.join("\n") + "\n")
    }

    pub async fn master_running(&self, t: &SshTarget) -> bool {
        if !self.multiplex {
            return false;
        }
        let mut cmd = self.base(&self.ssh);
        cmd.args(self.options(t, false))
            .args(["-O", "check", "--", &t.target])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        matches!(cmd.status().await, Ok(s) if s.success())
    }

    /// Starts the host's master connection (asks for a password through
    /// the askpass helper when the key isn't enough).
    pub async fn master_up(&self, t: &SshTarget) -> Result<(), SshError> {
        if self.master_running(t).await {
            return Ok(());
        }
        std::fs::create_dir_all(&self.run_dir)
            .map_err(|e| SshError::new(Category::Unreachable, e.to_string()))?;
        let log = self
            .run_dir
            .join(format!("ssh-master-{}.log", std::process::id()));
        let err_file = std::fs::File::create(&log)
            .map_err(|e| SshError::new(Category::Unreachable, e.to_string()))?;
        let mut cmd = self.base(&self.ssh);
        if self.multiplex {
            cmd.args(self.options(t, true))
                .args(["-N", "-f", "--", &t.target]);
        } else {
            // No master to start: one connection proves the host answers
            // and the key (or password) works.
            cmd.args(self.options(t, false))
                .args(["-T", "--", &t.target, "true"]);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(err_file)
            // The master outlives this call by design.
            .kill_on_drop(false);
        let status = cmd
            .status()
            .await
            .map_err(|e| SshError::new(Category::Unreachable, format!("Couldn't run ssh: {e}")))?;
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        let _ = std::fs::remove_file(&log);
        if status.success() {
            Ok(())
        } else {
            Err(fail(&text, &t.label()))
        }
    }

    /// `ssh -O exit`: ends the master (E2E-M14-4 drops the connection so).
    pub async fn master_exit(&self, t: &SshTarget) {
        if !self.multiplex {
            return;
        }
        let mut cmd = self.base(&self.ssh);
        cmd.args(self.options(t, false))
            .args(["-O", "exit", "--", &t.target])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let _ = cmd.status().await;
    }

    /// Runs `remote` on the host and returns stdout; a non-zero exit is an
    /// error with ssh's or the command's stderr.
    pub async fn exec(
        &self,
        t: &SshTarget,
        remote: &str,
        stdin: Option<&[u8]>,
    ) -> Result<String, SshError> {
        let mut cmd = self.command(t, remote);
        cmd.stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| SshError::new(Category::Unreachable, format!("Couldn't run ssh: {e}")))?;
        if let (Some(data), Some(mut input)) = (stdin, child.stdin.take()) {
            let data = data.to_vec();
            tokio::spawn(async move {
                let _ = input.write_all(&data).await;
                let _ = input.shutdown().await;
            });
        }
        let out = tokio::time::timeout(EXEC_TIMEOUT, child.wait_with_output())
            .await
            .map_err(|_| {
                SshError::new(
                    Category::Unreachable,
                    message(Category::Unreachable, &t.label(), ""),
                )
            })?
            .map_err(|e| SshError::new(Category::Unreachable, e.to_string()))?;
        let stderr = String::from_utf8_lossy(&out.stderr);
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else if out.status.code() == Some(255) {
            Err(fail(&stderr, &t.label()))
        } else {
            Err(SshError::new(Category::InstallFailed, last_lines(&stderr)))
        }
    }

    /// Makes sure `~/.tandem-host/<protocol>/tandem-host` is this build's,
    /// uploading it when it isn't, and checking the upload's SHA-256 on the
    /// host before it's used.
    pub async fn install(&self, t: &SshTarget, bin_dir: &Path) -> Result<Installed, SshError> {
        let host = t.label();
        let uname = self.exec(t, "uname -sm", None).await?;
        let plat = platform(uname.trim()).ok_or_else(|| {
            SshError::new(
                Category::UnsupportedArch,
                message(Category::UnsupportedArch, &host, uname.trim()),
            )
        })?;
        let bin = remote_bin();
        let install_err = |d: &str| {
            SshError::new(
                Category::InstallFailed,
                message(Category::InstallFailed, &host, d),
            )
        };
        let manifest = Manifest::load(bin_dir).map_err(|e| install_err(&e.to_string()))?;
        let (path, entry) = manifest
            .binary(bin_dir, plat)
            .map_err(|e| install_err(&e.to_string()))?;
        // The copy on the host is this build's only if its hash matches.
        let have = self
            .exec(
                t,
                &format!("( sha256sum {bin} 2>/dev/null || shasum -a 256 {bin} 2>/dev/null ) | cut -d' ' -f1"),
                None,
            )
            .await?;
        if have.trim() == entry.sha256 {
            return Ok(Installed {
                platform: plat.into(),
                uploaded: false,
            });
        }
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|e| install_err(&e.to_string()))?;
        let dir = format!("\"$HOME\"/{}", remote_dir());
        let tmp = format!("{dir}/tandem-host.tmp");
        self.exec(
            t,
            &format!("umask 077 && mkdir -p {dir} && cat > {tmp} && chmod 700 {tmp}"),
            Some(&bytes),
        )
        .await?;
        let check = format!(
            "h=$( (sha256sum {tmp} 2>/dev/null || shasum -a 256 {tmp}) | cut -d' ' -f1 ); \
             if [ \"$h\" = \"{sum}\" ]; then mv -f {tmp} {bin} && echo verified; else rm -f {tmp}; echo \"mismatch $h\"; fi",
            sum = entry.sha256
        );
        let verdict = self.exec(t, &check, None).await?;
        if verdict.trim() != "verified" {
            return Err(install_err("The uploaded copy didn't match its checksum."));
        }
        Ok(Installed {
            platform: plat.into(),
            uploaded: true,
        })
    }

    /// `tandem-host serve` on the host, its stdio piped.
    pub fn serve(&self, t: &SshTarget) -> std::io::Result<Child> {
        let mut cmd = self.command(t, &format!("exec {} serve", remote_bin()));
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    }

    /// The command line that runs `argv` on the host through `tandem-host
    /// run` (chat engines): its stdin and stdout are the engine's.
    pub fn run_args(
        &self,
        t: &SshTarget,
        cwd: &str,
        env: &std::collections::BTreeMap<String, String>,
        argv: &[String],
    ) -> (PathBuf, Vec<String>) {
        let mut remote = vec![
            remote_bin(),
            "run".into(),
            "--cwd".into(),
            crate::quote(cwd),
        ];
        for (k, v) in crate::env::allowed(env) {
            remote.push("--env".into());
            remote.push(crate::quote(&format!("{k}={v}")));
        }
        remote.push("--".into());
        remote.extend(argv.iter().map(|a| crate::quote(a)));
        let mut args = self.options(t, false);
        args.extend([
            "-T".into(),
            "--".into(),
            t.target.clone(),
            format!("exec {}", remote.join(" ")),
        ]);
        (self.ssh.clone(), args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_rm_01_targets_parse_from_aliases_and_user_host_port() {
        let t = SshTarget::parse("tandem@127.0.0.1:2222").expect("t");
        assert_eq!(
            (t.user.as_deref(), t.target.as_str(), t.port),
            (Some("tandem"), "127.0.0.1", Some(2222))
        );
        let t = SshTarget::parse("devbox").expect("alias");
        assert_eq!((t.user, t.target.as_str(), t.port), (None, "devbox", None));
        let t = SshTarget::parse("[::1]:2200").expect("v6");
        assert_eq!((t.target.as_str(), t.port), ("::1", Some(2200)));
        assert!(SshTarget::parse("-oProxyCommand=x").is_none());
        assert!(SshTarget::parse("a b").is_none());
        assert!(SshTarget::parse("host:notaport").is_none());
        assert_eq!(SshTarget::parse("u@h:22").expect("t").label(), "u@h");
    }

    #[test]
    fn f_rm_01_2_a_changed_key_is_the_host_key_category() {
        let changed = "@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\n\
@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\n\
Host key verification failed.";
        assert_eq!(classify(changed), Category::HostKey);
        assert_eq!(
            classify("ssh: connect to host x port 22: Connection refused"),
            Category::Unreachable
        );
        assert_eq!(
            classify("ssh: Could not resolve hostname nope"),
            Category::Unreachable
        );
        assert_eq!(
            classify("tandem@h: Permission denied (publickey)."),
            Category::SignIn
        );
        assert_eq!(
            classify("Warning: Identity file /x not accessible: No such file"),
            Category::KeyFile
        );
        assert_eq!(
            classify("Load key \"/k\": invalid format"),
            Category::KeyFile
        );
        assert!(message(Category::HostKey, "h", "").contains("won't connect"));
    }

    #[test]
    fn f_rm_01_options_are_strict_and_multiplexed() {
        let mut s = Ssh::new(PathBuf::from("/d/run"), PathBuf::from("/d/ssh/known_hosts"));
        s.multiplex = true;
        let t = SshTarget {
            target: "h".into(),
            port: Some(2222),
            user: Some("u".into()),
            identity_file: Some("/k".into()),
        };
        let o = s.options(&t, false).join(" ");
        let control = format!("ControlPath={}", s.control_path().display());
        for want in [
            "ControlMaster=no",
            &control,
            "ControlPersist=10m",
            "StrictHostKeyChecking=yes",
            "UserKnownHostsFile=/d/ssh/known_hosts",
            "GlobalKnownHostsFile=/dev/null",
            "BatchMode=yes",
            "ForwardAgent=no",
            "-i /k",
            "-p 2222",
            "-l u",
        ] {
            assert!(o.contains(want), "{want} missing from {o}");
        }
        assert!(s.options(&t, true).join(" ").contains("ControlMaster=yes"));
        // Windows' OpenSSH can't share a connection: no Control* options,
        // the rest unchanged.
        s.multiplex = false;
        let o = s.options(&t, true).join(" ");
        assert!(!o.contains("Control"), "{o}");
        assert!(o.contains("StrictHostKeyChecking=yes") && o.contains("BatchMode=yes"));
    }

    #[test]
    fn f_rm_01_multiplexing_is_off_only_on_windows() {
        let s = Ssh::new(PathBuf::from("/d/run"), PathBuf::from("/d/kh"));
        assert_eq!(s.multiplex, !cfg!(windows));
    }

    #[test]
    fn f_rm_01_host_keys_scan_fingerprint_and_pin() {
        // The fingerprint `ssh-keygen -lf` prints for this key.
        let key = "AAAAC3NzaC1lZDI1NTE5AAAAIAqOoL6VNQ1OPUC8g5wRGWLOehgFrQ090B4ElYdPHpN1";
        let out = format!(
            "# 127.0.0.1:2222 SSH-2.0-OpenSSH\n[127.0.0.1]:2222 ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQ==\n[127.0.0.1]:2222 ssh-ed25519 {key}\n"
        );
        let keys = parse_keyscan(&out);
        assert_eq!(keys[0].key_type, "ssh-ed25519");
        assert!(keys[0].fingerprint.starts_with("SHA256:"));
        assert_eq!(
            keys[0].fingerprint,
            "SHA256:stJujlycZu7KX3xkUhjnpN9DhoZl3Ebuc37lTVtPPFY"
        );
        let r = Resolved {
            hostname: "127.0.0.1".into(),
            port: 2222,
            user: "tandem".into(),
            via: None,
        };
        let dir = tempfile::tempdir().expect("tmp");
        let s = Ssh::new(dir.path().join("run"), dir.path().join("ssh/known_hosts"));
        assert!(s.known(&r).is_empty());
        s.trust(&r, &keys[0]).expect("trust");
        assert_eq!(s.known(&r), [keys[0].fingerprint.clone()]);
        // Trusting a new key replaces the old one; other hosts stay.
        let other = Resolved {
            hostname: "h2".into(),
            port: 22,
            user: String::new(),
            via: None,
        };
        s.trust(
            &other,
            &HostKey {
                line: format!("h2 ssh-ed25519 {key}"),
                ..keys[0].clone()
            },
        )
        .expect("t2");
        s.trust(&r, &keys[1]).expect("retrust");
        assert_eq!(s.known(&r), [keys[1].fingerprint.clone()]);
        assert_eq!(s.known(&other).len(), 1);
        s.forget(&r).expect("forget");
        assert!(s.known(&r).is_empty());
        assert_eq!(s.known(&other).len(), 1);
        assert_eq!(
            parse_g("user tandem\nhostname 10.0.0.5\nport 2222\n"),
            Some(Resolved {
                hostname: "10.0.0.5".into(),
                port: 2222,
                user: "tandem".into(),
                via: None,
            })
        );
    }

    #[test]
    fn f_rm_01_hosts_behind_a_proxy_are_named_not_scanned() {
        let direct = "user tandem\nhostname 10.0.0.5\nport 22\nproxycommand none\n";
        assert_eq!(proxied(direct), None);
        assert_eq!(
            proxied("hostname inner\nproxyjump bastion.example.com\n"),
            Some("ProxyJump")
        );
        assert_eq!(
            proxied("hostname inner\nproxycommand ssh -W %h:%p bastion\n"),
            Some("ProxyCommand")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn f_rm_01_a_proxyjump_hosts_key_is_read_through_ssh_offering_no_sign_in() {
        use std::os::unix::fs::PermissionsExt;
        // An ssh whose config sends `inner` through a jump host: `-G` says
        // so, and a connection writes the key the host showed to the
        // `known_hosts` it was given, then fails to sign in (none offered).
        let dir = tempfile::tempdir().expect("tmp");
        let fake = dir.path().join("ssh");
        let args = dir.path().join("args");
        std::fs::write(
            &fake,
            format!(
                r#"#!/bin/sh
if [ "$1" = -G ]; then printf 'user me\nhostname 10.9.9.9\nport 22\nproxyjump bastion\n'; exit 0; fi
printf '%s\n' "$@" > {args}
for a in "$@"; do case "$a" in UserKnownHostsFile=*) kh="${{a#UserKnownHostsFile=}}";; esac; done
echo "10.9.9.9 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl" > "$kh"
echo "me@10.9.9.9: Permission denied (publickey)." >&2
exit 255
"#,
                args = args.display()
            ),
        )
        .expect("w");
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let mut s = Ssh::new(dir.path().join("run"), dir.path().join("kh"));
        s.ssh = fake;
        // `ssh-keyscan` would connect directly; it must never run.
        s.keyscan = dir.path().join("no-keyscan");
        s.config = Some(dir.path().join("config"));
        let t = SshTarget::parse("me@inner").expect("t");
        let r = s.resolve(&t).await.expect("resolved");
        assert_eq!(r.via, Some("ProxyJump"));
        let keys = s.host_keys(&t, &r).await.expect("keys");
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key_type, "ssh-ed25519");
        assert!(
            keys[0].line.starts_with("10.9.9.9 ssh-ed25519 "),
            "{}",
            keys[0].line
        );
        let sent = std::fs::read_to_string(&args).expect("args");
        let sent: Vec<&str> = sent.lines().collect();
        for must in [
            "PubkeyAuthentication=no",
            "PasswordAuthentication=no",
            "KbdInteractiveAuthentication=no",
            "ForwardAgent=no",
            "StrictHostKeyChecking=accept-new",
            "ControlPath=none",
        ] {
            assert!(sent.contains(&must), "{must} in {sent:?}");
        }
        assert_eq!(
            sent[..2],
            ["-F", dir.path().join("config").to_str().expect("utf8")]
        );
        assert!(!sent.contains(&"-i"), "no key is offered: {sent:?}");
        assert!(
            !sent
                .iter()
                .any(|a| a.contains(&*s.known_hosts.to_string_lossy())),
            "the probe never touches the pinned keys: {sent:?}"
        );
        // Nothing is trusted, and the throwaway file is gone.
        assert!(s.known(&r).is_empty());
        let left: Vec<_> = std::fs::read_dir(dir.path().join("run"))
            .expect("run")
            .flatten()
            .collect();
        assert!(left.is_empty(), "{left:?}");
        // A jump that fails leaves nothing to approve, and says through what.
        std::fs::write(
            &s.ssh,
            "#!/bin/sh\nif [ \"$1\" = -G ]; then printf 'hostname inner\\nport 22\\nproxycommand nc %h %p\\n'; exit 0; fi\necho 'ssh: connect to host bastion port 22: Connection refused' >&2\nexit 255\n",
        )
        .expect("w");
        let r = s.resolve(&t).await.expect("resolved");
        let e = s.host_keys(&t, &r).await.expect_err("unreachable");
        assert_eq!(e.category, Category::Unreachable);
        assert!(e.message.contains("through ProxyCommand"), "{}", e.message);
        assert!(e.message.contains("Connection refused"), "{}", e.message);
    }

    #[test]
    fn f_rm_01_remote_engines_run_through_tandem_host_run() {
        let s = Ssh::new(PathBuf::from("/d/run"), PathBuf::from("/d/kh"));
        let t = SshTarget::parse("u@h").expect("t");
        let env = [
            ("TERM".to_string(), "dumb".to_string()),
            (
                "CLAUDE_CONFIG_DIR".to_string(),
                "/l/.tandem/providers/claude/a".to_string(),
            ),
        ]
        .into_iter()
        .collect();
        let (prog, args) = s.run_args(
            &t,
            "/home/u/my project",
            &env,
            &["claude".into(), "-p".into()],
        );
        assert_eq!(prog, PathBuf::from("ssh"));
        let remote = args.last().expect("remote");
        assert_eq!(
            remote,
            "exec \"$HOME\"/.tandem-host/1/tandem-host run --cwd '/home/u/my project' --env TERM=dumb -- claude -p"
        );
        assert!(!args.join(" ").contains("providers"));
    }
}
