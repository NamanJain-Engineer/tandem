//! F-RM-01 against the `sshd` compose service (MILESTONES M14):
//!
//! ```sh
//! bash scripts/gen-ssh-fixture.sh && docker compose -f infra/docker-compose.yml --profile remote up -d --wait sshd
//! sh scripts/build-tandem-host.sh
//! cargo test -p tandem-host --features e2e-ssh -- --include-ignored f_rm_01
//! ```
//!
//! Each test has its own data folder (control sockets, known_hosts); they
//! run one at a time because they share the host's install folder.

#![cfg(all(feature = "e2e-ssh", unix))]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{json, Value};
use tandem_host::client::{ClientError, HostClient};
use tandem_host::protocol::{
    method, ErrorKind, Hello, HelloReply, Message, PtyAttached, PtyOutput, PtySpawned, PROTOCOL,
    VERSION,
};
use tandem_host::ssh::{Category, HostKey, Ssh, SshTarget};
use tokio::sync::mpsc::UnboundedReceiver;

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bin_dir() -> PathBuf {
    std::env::var_os("TANDEM_HOST_BIN_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo().join("apps/desktop/src-tauri/resources/tandem-host"))
}

fn target() -> SshTarget {
    SshTarget {
        target: "127.0.0.1".into(),
        port: Some(2222),
        user: Some("tandem".into()),
        identity_file: Some(
            repo()
                .join("infra/ssh/id_ed25519")
                .to_string_lossy()
                .into_owned(),
        ),
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    ssh: Ssh,
    t: SshTarget,
}

/// A fresh data folder, the host's key approved (as the user would after
/// seeing its fingerprint), and the master connection up.
async fn connected() -> Fixture {
    connected_sharing(true).await
}

/// [`connected`], with or without one shared connection per host (Windows'
/// OpenSSH has none).
async fn connected_sharing(multiplex: bool) -> Fixture {
    let dir = tempfile::tempdir().expect("tmp");
    // Control sockets need a short path.
    let mut ssh = Ssh::new(dir.path().join("run"), dir.path().join("ssh/known_hosts"));
    ssh.multiplex = multiplex;
    let t = target();
    let r = ssh.resolve(&t).await.expect("ssh -G");
    assert_eq!(
        (r.hostname.as_str(), r.port, r.user.as_str()),
        ("127.0.0.1", 2222, "tandem")
    );
    // Unknown host: nothing pinned, so a connection is refused until approved.
    assert!(ssh.known(&r).is_empty());
    let refused = ssh.master_up(&t).await.expect_err("unknown host");
    assert_eq!(refused.category, Category::HostKey);
    let keys = ssh.scan(&r).await.expect("keyscan");
    assert!(keys[0].fingerprint.starts_with("SHA256:"));
    ssh.trust(&r, &keys[0]).expect("trust");
    ssh.master_up(&t).await.expect("master");
    Fixture { _dir: dir, ssh, t }
}

async fn open(
    f: &Fixture,
) -> (
    std::sync::Arc<HostClient>,
    UnboundedReceiver<Message>,
    tokio::process::Child,
) {
    let mut child = f.ssh.serve(&f.t).expect("serve");
    let (r, w) = (
        child.stdout.take().expect("out"),
        child.stdin.take().expect("in"),
    );
    let (c, notes) = HostClient::start(r, w);
    let hello: HelloReply = c
        .call(
            method::HELLO,
            Hello {
                protocol: PROTOCOL,
                client_version: VERSION.into(),
            },
        )
        .await
        .expect("hello");
    assert_eq!(hello.host_version, VERSION);
    assert_eq!(hello.home, "/home/tandem");
    (c, notes, child)
}

async fn output_until(
    notes: &mut UnboundedReceiver<Message>,
    id: &str,
    want: &str,
) -> (String, u64) {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut text = String::new();
    let mut end = 0;
    loop {
        if text.contains(want) {
            return (text, end);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        let msg = tokio::time::timeout(left, notes.recv())
            .await
            .unwrap_or_else(|_| panic!("no {want:?} in {text:?}"))
            .expect("open");
        if msg.method.as_deref() == Some(method::PTY_OUTPUT) {
            let o: PtyOutput = serde_json::from_value(msg.params.expect("p")).expect("o");
            if o.id == id {
                let d = base64::engine::general_purpose::STANDARD
                    .decode(o.data_b64)
                    .expect("b64");
                text.push_str(&String::from_utf8_lossy(&d));
                end = o.seq + d.len() as u64;
            }
        }
    }
}

fn b64(s: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(s)
}

#[tokio::test]
#[ignore = "needs the sshd compose service (profile remote)"]
async fn f_rm_01_1_connect_upload_host_open_terminal_echo_hi() {
    let _g = SERIAL.lock().await;
    let f = connected().await;
    // A clean host: the helper is uploaded and checked, then found.
    f.ssh
        .exec(&f.t, "rm -rf \"$HOME\"/.tandem-host/1", None)
        .await
        .expect("clean");
    let first = f.ssh.install(&f.t, &bin_dir()).await.expect("install");
    assert_eq!(first.platform, "linux-x86_64");
    assert!(first.uploaded);
    assert!(
        !f.ssh
            .install(&f.t, &bin_dir())
            .await
            .expect("again")
            .uploaded
    );
    let listed = f
        .ssh
        .exec(&f.t, "ls ~/.tandem-host", None)
        .await
        .expect("ls");
    // The protocol's folder (and the daemon's `run`, once it has started).
    assert!(listed.lines().any(|l| l == "1"), "{listed}");

    let (c, mut notes, _child) = open(&f).await;
    let spawned: PtySpawned = c
        .call(
            method::PTY_SPAWN,
            json!({"id": "pn_echo", "cwd": "/home/tandem/project", "cols": 80, "rows": 24,
                   "env": {"TERM": "xterm-256color", "TANDEM_PANE_ID": "pn_echo"}}),
        )
        .await
        .expect("spawn");
    assert!(spawned.pid > 0);
    let _: Value = c
        .call(
            method::PTY_WRITE,
            json!({"id": "pn_echo", "data_b64": b64("echo hi; pwd\n")}),
        )
        .await
        .expect("write");
    let (text, _) = output_until(&mut notes, "pn_echo", "/home/tandem/project").await;
    assert!(text.contains("\nhi") || text.contains("hi\r\n"), "{text:?}");
    let _: Value = c
        .call(method::PTY_KILL, json!({"id": "pn_echo"}))
        .await
        .expect("kill");
}

/// The Windows path (no ControlMaster): the host key is still checked,
/// "connecting" only proves the host answers, and every command, the
/// helper's session included, opens its own connection.
#[tokio::test]
#[ignore = "needs the sshd compose service (profile remote)"]
async fn f_rm_01_without_connection_sharing_as_on_windows() {
    let _g = SERIAL.lock().await;
    let f = connected_sharing(false).await;
    assert!(!f.ssh.master_running(&f.t).await);
    let sockets = std::fs::read_dir(f.ssh.run_dir.as_path())
        .map(|d| d.flatten().count())
        .unwrap_or(0);
    assert_eq!(sockets, 0, "no control socket");
    let who = f.ssh.exec(&f.t, "whoami", None).await.expect("exec");
    assert_eq!(who.trim(), "tandem");
    f.ssh.install(&f.t, &bin_dir()).await.expect("install");
    let (c, mut notes, _child) = open(&f).await;
    let _: PtySpawned = c
        .call(
            method::PTY_SPAWN,
            json!({"id": "pn_win", "cwd": "/home/tandem/project", "cols": 80, "rows": 24,
                   "env": {"TERM": "xterm-256color", "TANDEM_PANE_ID": "pn_win"}}),
        )
        .await
        .expect("spawn");
    let _: Value = c
        .call(
            method::PTY_WRITE,
            json!({"id": "pn_win", "data_b64": b64("echo win-ok\n")}),
        )
        .await
        .expect("write");
    output_until(&mut notes, "pn_win", "win-ok").await;
    let _: Value = c
        .call(method::PTY_KILL, json!({"id": "pn_win"}))
        .await
        .expect("kill");
    // Nothing to end.
    f.ssh.master_exit(&f.t).await;
}

#[tokio::test]
#[ignore = "needs the sshd compose service (profile remote)"]
async fn f_rm_01_2_a_changed_host_key_refuses_the_connection() {
    let _g = SERIAL.lock().await;
    let f = connected().await;
    f.ssh.master_exit(&f.t).await;
    // Another key for the same host, as after the host's key changed.
    let r = f.ssh.resolve(&f.t).await.expect("r");
    let other = HostKey {
        line: "[127.0.0.1]:2222 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAqOoL6VNQ1OPUC8g5wRGWLOehgFrQ090B4ElYdPHpN1".into(),
        key_type: "ssh-ed25519".into(),
        fingerprint: String::new(),
    };
    f.ssh.trust(&r, &other).expect("pin the old key");
    let err = f.ssh.master_up(&f.t).await.expect_err("refused");
    assert_eq!(err.category, Category::HostKey);
    assert!(err.message.contains("won't connect"), "{}", err.message);
    let err = f
        .ssh
        .exec(&f.t, "true", None)
        .await
        .expect_err("refused too");
    assert_eq!(err.category, Category::HostKey);
}

#[tokio::test]
#[ignore = "needs the sshd compose service (profile remote)"]
async fn f_rm_01_3_reading_outside_the_root_is_refused() {
    let _g = SERIAL.lock().await;
    let f = connected().await;
    f.ssh.install(&f.t, &bin_dir()).await.expect("install");
    // The fixture's repository as the image made it (other tests edit it).
    f.ssh
        .exec(
            &f.t,
            // The fixture's project as made (one commit on main): E2E specs
            // commit and push in it (E2E-M14-7, `resetProject`).
            "cd ~/project && git checkout -q -f main && git reset -q --hard \"$(git rev-list --max-parents=0 HEAD)\" && git clean -fdxq",
            None,
        )
        .await
        .expect("reset");
    let (c, _notes, _child) = open(&f).await;
    let err = c
        .call::<_, Value>(
            method::FS_READ,
            json!({"root": "/home/tandem/project", "path": "../../etc/passwd"}),
        )
        .await
        .expect_err("refused");
    assert!(
        matches!(err, ClientError::Rpc(ref e) if e.kind == ErrorKind::PathEscape),
        "{err:?}"
    );
    let ok: Value = c
        .call(
            method::FS_READ,
            json!({"root": "/home/tandem/project", "path": "README.md"}),
        )
        .await
        .expect("inside");
    assert_eq!(ok["text"], "# project\n");
}

#[tokio::test]
#[ignore = "needs the sshd compose service (profile remote)"]
async fn f_rm_01_4_the_remote_spawn_env_has_no_local_provider_home() {
    let _g = SERIAL.lock().await;
    let f = connected().await;
    f.ssh.install(&f.t, &bin_dir()).await.expect("install");
    let (c, mut notes, _child) = open(&f).await;
    let _: PtySpawned = c
        .call(
            method::PTY_SPAWN,
            json!({"id": "pn_env", "argv": ["/usr/bin/env"], "cwd": "/home/tandem/project", "cols": 200, "rows": 50,
                   "env": {"TANDEM_PANE_ID": "pn_env", "CLAUDE_CONFIG_DIR": "/home/me/.tandem/providers/claude/acct",
                           "CODEX_HOME": "/home/me/.tandem/providers/codex/acct", "TERM": "xterm-256color"}}),
        )
        .await
        .expect("spawn");
    let (text, _) = output_until(&mut notes, "pn_env", "TANDEM_PANE_ID=pn_env").await;
    // Wait for the rest of `env`.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut all = text;
    while let Ok(m) = notes.try_recv() {
        if let Some(p) = m
            .params
            .filter(|_| m.method.as_deref() == Some(method::PTY_OUTPUT))
        {
            let o: PtyOutput = serde_json::from_value(p).expect("o");
            all.push_str(&String::from_utf8_lossy(
                &base64::engine::general_purpose::STANDARD
                    .decode(o.data_b64)
                    .expect("b64"),
            ));
        }
    }
    assert!(!all.contains(".tandem/providers"), "{all}");
    assert!(!all.contains("CLAUDE_CONFIG_DIR"), "{all}");
    assert!(all.contains("HOME=/home/tandem"), "{all}");
}

#[tokio::test]
#[ignore = "needs the sshd compose service (profile remote)"]
async fn f_rm_01_5_dropping_the_connection_reattaches_the_same_pid() {
    let _g = SERIAL.lock().await;
    let f = connected().await;
    f.ssh.install(&f.t, &bin_dir()).await.expect("install");
    let (c, mut notes, _child) = open(&f).await;
    let spawned: PtySpawned = c
        .call(
            method::PTY_SPAWN,
            json!({"id": "pn_keep", "cwd": "/home/tandem/project", "cols": 80, "rows": 24, "env": {}}),
        )
        .await
        .expect("spawn");
    let _: Value = c
        .call(
            method::PTY_WRITE,
            json!({"id": "pn_keep", "data_b64": b64("echo before-$$\n")}),
        )
        .await
        .expect("write");
    let (_, end) = output_until(&mut notes, "pn_keep", &format!("before-{}", spawned.pid)).await;
    // `ssh -O exit`: the master and every session on it go.
    f.ssh.master_exit(&f.t).await;
    tokio::time::timeout(Duration::from_secs(10), c.closed())
        .await
        .expect("client sees the drop");

    f.ssh.master_up(&f.t).await.expect("reconnect");
    let (c2, mut notes2, _child2) = open(&f).await;
    let att: PtyAttached = c2
        .call(
            method::PTY_ATTACH,
            json!({"id": "pn_keep", "from_seq": end}),
        )
        .await
        .expect("attach");
    assert_eq!(att.pid, spawned.pid, "the same remote PTY re-attached");
    assert!(att.alive);
    let _: Value = c2
        .call(
            method::PTY_WRITE,
            json!({"id": "pn_keep", "data_b64": b64("echo after-$$\n")}),
        )
        .await
        .expect("write");
    output_until(&mut notes2, "pn_keep", &format!("after-{}", spawned.pid)).await;
    let _: Value = c2
        .call(method::PTY_KILL, json!({"id": "pn_keep"}))
        .await
        .expect("kill");
}

#[tokio::test]
#[ignore = "needs the sshd compose service (profile remote)"]
async fn f_rm_01_remote_engines_are_detected_and_run_through_tandem_host() {
    let _g = SERIAL.lock().await;
    let f = connected().await;
    f.ssh.install(&f.t, &bin_dir()).await.expect("install");
    let (c, _notes, _child) = open(&f).await;
    let found: Value = c
        .call(
            method::ENGINES_DETECT,
            json!({"engines": [{"id": "git", "commands": ["git"], "version_args": ["--version"]},
                               {"id": "nope", "commands": ["no-such-engine"]}]}),
        )
        .await
        .expect("detect");
    assert_eq!(found.as_array().map(Vec::len), Some(1));
    assert!(found[0]["version"]
        .as_str()
        .unwrap_or_default()
        .starts_with("git version"));
    // A chat engine's process: stdin and stdout are the engine's.
    let env = [("TERM".to_string(), "dumb".to_string())]
        .into_iter()
        .collect();
    let (prog, args) = f.ssh.run_args(
        &f.t,
        "/home/tandem/project",
        &env,
        &[
            "sh".into(),
            "-c".into(),
            "read x; echo got-$x-$(pwd)".into(),
        ],
    );
    let mut cmd = tokio::process::Command::new(prog);
    cmd.args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    let mut child = cmd.spawn().expect("run");
    use tokio::io::AsyncWriteExt;
    let mut stdin = child.stdin.take().expect("in");
    stdin.write_all(b"ping\n").await.expect("w");
    drop(stdin);
    let out = tokio::time::timeout(Duration::from_secs(20), child.wait_with_output())
        .await
        .expect("in time")
        .expect("out");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "got-ping-/home/tandem/project"
    );
}

/// D-1603: a host behind `ProxyJump` and one behind `ProxyCommand`, both
/// the fixture's own `localhost:22` reached through the fixture: the key is
/// read through the jump (nothing listens on this machine's :22, so a direct
/// scan couldn't), it's the fixture's, and once pinned the master connects
/// through the jump and checks it.
#[tokio::test]
#[ignore = "needs the sshd compose service (profile remote)"]
async fn f_rm_01_hosts_behind_proxyjump_and_proxycommand_connect_through_the_users_config() {
    let _g = SERIAL.lock().await;
    let dir = tempfile::tempdir().expect("tmp");
    let key = repo().join("infra/ssh/id_ed25519");
    let config = dir.path().join("config");
    let inner = |alias: &str, how: &str| {
        format!(
            "Host {alias}\n  HostName localhost\n  Port 22\n  User tandem\n  IdentityFile {}\n  IdentitiesOnly yes\n  {how}\n",
            key.display()
        )
    };
    std::fs::write(
        &config,
        format!(
            "Host tandem-jump\n  HostName 127.0.0.1\n  Port 2222\n  User tandem\n  IdentityFile {}\n  IdentitiesOnly yes\n  StrictHostKeyChecking no\n  UserKnownHostsFile /dev/null\n  LogLevel ERROR\n{}{}",
            key.display(),
            inner("via-jump", "ProxyJump tandem-jump"),
            inner(
                "via-command",
                &format!("ProxyCommand ssh -F {} -W %h:%p tandem-jump", config.display())
            ),
        ),
    )
    .expect("config");
    // The fixture's own keys, scanned directly on its published port.
    let direct = Ssh::new(dir.path().join("d"), dir.path().join("d/kh"));
    let fixture = direct
        .scan(&direct.resolve(&target()).await.expect("resolve"))
        .await
        .expect("scan");
    for (alias, how) in [("via-jump", "ProxyJump"), ("via-command", "ProxyCommand")] {
        let data = dir.path().join(alias);
        let mut ssh = Ssh::new(data.join("run"), data.join("ssh/known_hosts"));
        ssh.config = Some(config.clone());
        let t = SshTarget::parse(alias).expect("target");
        let r = ssh.resolve(&t).await.expect("ssh -G");
        assert_eq!(r.via, Some(how));
        assert_eq!((r.hostname.as_str(), r.port), ("localhost", 22));
        // Unknown: refused until approved, as for a direct host.
        let refused = ssh.master_up(&t).await.expect_err("unknown host");
        assert_eq!(refused.category, Category::HostKey, "{}", refused.message);
        let keys = ssh.host_keys(&t, &r).await.expect("keys through the jump");
        assert!(
            fixture.iter().any(|k| k.fingerprint == keys[0].fingerprint),
            "{how}: {keys:?} vs {fixture:?}"
        );
        assert!(ssh.known(&r).is_empty(), "nothing pinned before approval");
        ssh.trust(&r, &keys[0]).expect("trust");
        ssh.master_up(&t).await.expect("master through the jump");
        let out = ssh.exec(&t, "echo via-$USER", None).await.expect("exec");
        assert_eq!(out.trim(), "via-tandem");
        ssh.master_exit(&t).await;
        // A different pinned key refuses, through the jump too.
        let wrong = HostKey {
            line: format!(
                "localhost ssh-ed25519 {}",
                "AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl"
            ),
            key_type: "ssh-ed25519".into(),
            fingerprint: String::new(),
        };
        ssh.trust(&r, &wrong).expect("pin another key");
        let e = ssh.master_up(&t).await.expect_err("changed key");
        assert_eq!(e.category, Category::HostKey, "{}", e.message);
    }
}
