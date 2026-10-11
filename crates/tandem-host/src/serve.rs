//! `tandem-host serve` (one per SSH connection), `tandem-host run` (one
//! per remote chat engine) and `tandem-host mcp` (one per MCP server an
//! engine on the host starts, D-1604).
//!
//! `serve` connects to the session daemon of this version, starting it
//! when none runs, and pipes stdin and stdout to it: the daemon does the
//! work and keeps the PTYs when the connection goes.
//!
//! `run --cwd <dir> [--env K=V]… -- <argv>` starts a thread's engine with
//! the host's environment plus the allowed variables, in its own process
//! group, and relays stdin to it. When the SSH channel closes (the thread
//! stopped, or the connection dropped) stdin ends: the engine's stdin is
//! closed, and the group is ended if it hasn't exited 3 s later.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::daemon::{ensure_run_dir, run_dir, socket_path};
use crate::env;
use crate::protocol::{method, McpLine, Message};

fn connect_or_start(exe: &Path, home: &Path) -> std::io::Result<UnixStream> {
    let run = run_dir(home);
    ensure_run_dir(&run)?;
    let sock = socket_path(&run);
    if let Ok(s) = UnixStream::connect(&sock) {
        return Ok(s);
    }
    let mut cmd = Command::new(exe);
    cmd.arg("daemon")
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: setsid(2) between fork and exec; it touches no memory.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd.spawn()?;
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        match UnixStream::connect(&sock) {
            Ok(s) => return Ok(s),
            Err(e) if Instant::now() >= until => return Err(e),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

pub fn serve(exe: &Path, home: &Path) -> std::io::Result<()> {
    let stream = connect_or_start(exe, home)?;
    let mut to_daemon = stream.try_clone()?;
    let closer = stream.try_clone()?;
    std::thread::Builder::new()
        .name("stdin".into())
        .spawn(move || {
            let mut stdin = std::io::stdin().lock();
            let _ = std::io::copy(&mut stdin, &mut to_daemon);
            // The connection is gone: the daemon sees this client leave.
            let _ = closer.shutdown(std::net::Shutdown::Both);
        })?;
    let mut from_daemon = stream;
    let mut stdout = std::io::stdout().lock();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = from_daemon.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        stdout.write_all(&buf[..n])?;
        stdout.flush()?;
    }
}

/// `mcp --channel <id>`: the stdio MCP server an engine on the host starts.
/// It serves nothing itself: it attaches to the channel the desktop opened
/// for this engine session and passes each JSON-RPC line between the
/// engine and the desktop, where the server runs (D-1604). Without a daemon
/// (the desktop isn't connected) it says so and exits.
pub fn mcp(home: &Path, channel: &str) -> std::io::Result<()> {
    let sock = UnixStream::connect(socket_path(&run_dir(home))).map_err(|e| {
        std::io::Error::new(
            e.kind(),
            format!("Vibezeo isn't connected to this host ({e})"),
        )
    })?;
    relay(
        channel,
        sock,
        BufReader::new(std::io::stdin()),
        std::io::stdout().lock(),
    )
}

/// Attaches to `channel` over `sock`, then passes lines from `input` to
/// the daemon and the daemon's lines to `output`, until either side ends.
pub fn relay<R, W>(
    channel: &str,
    sock: UnixStream,
    mut input: R,
    mut output: W,
) -> std::io::Result<()>
where
    R: BufRead + Send + 'static,
    W: Write,
{
    let mut to = sock.try_clone()?;
    to.write_all(
        &Message::request(1, method::MCP_ATTACH, json!({ "channel": channel })).to_line(),
    )?;
    let mut from = BufReader::new(sock.try_clone()?);
    let mut line = Vec::new();
    // Nothing reaches a relay before its attach is answered.
    loop {
        line.clear();
        if from.read_until(b'\n', &mut line)? == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "the helper closed the channel",
            ));
        }
        let Ok(m) = serde_json::from_slice::<Message>(&line) else {
            continue;
        };
        if m.id == Some(1) {
            if let Some(e) = m.error {
                return Err(std::io::Error::other(e.message));
            }
            break;
        }
    }
    let closer = sock.try_clone()?;
    let chan = channel.to_string();
    std::thread::Builder::new()
        .name("mcp-in".into())
        .spawn(move || {
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match input.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let text = String::from_utf8_lossy(&buf);
                let text = text.trim_end_matches(['\r', '\n']);
                if text.is_empty() {
                    continue;
                }
                let note = Message::notification(
                    method::MCP_LINE,
                    json!({ "channel": chan, "line": text }),
                );
                if to.write_all(&note.to_line()).is_err() {
                    break;
                }
            }
            // The engine stopped its server: the daemon tells the desktop.
            let _ = closer.shutdown(std::net::Shutdown::Both);
        })?;
    loop {
        line.clear();
        if from.read_until(b'\n', &mut line)? == 0 {
            return Ok(());
        }
        let Ok(m) = serde_json::from_slice::<Message>(&line) else {
            continue;
        };
        if m.method.as_deref() != Some(method::MCP_LINE) {
            continue;
        }
        let Some(l) = m
            .params
            .and_then(|p| serde_json::from_value::<McpLine>(p).ok())
        else {
            continue;
        };
        output.write_all(l.line.as_bytes())?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
}

/// `run`'s arguments.
#[derive(Debug, PartialEq, Eq)]
pub struct RunArgs {
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub argv: Vec<String>,
}

pub fn parse_run(args: &[String]) -> Result<RunArgs, String> {
    let mut cwd = None;
    let mut env = BTreeMap::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--cwd" => cwd = it.next().map(PathBuf::from),
            "--env" => {
                let kv = it.next().ok_or("--env needs K=V")?;
                let (k, v) = kv.split_once('=').ok_or("--env needs K=V")?;
                env.insert(k.to_string(), v.to_string());
            }
            "--" => {
                let argv: Vec<String> = it.cloned().collect();
                if argv.is_empty() {
                    return Err("nothing to run".into());
                }
                return Ok(RunArgs {
                    cwd: cwd.ok_or("--cwd is required")?,
                    env,
                    argv,
                });
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Err("missing -- before the command".into())
}

pub fn run(args: RunArgs, home: &Path) -> std::io::Result<i32> {
    if !args.cwd.is_dir() {
        eprintln!("tandem-host: the remote folder doesn't exist");
        return Ok(127);
    }
    let status_sock = crate::daemon::status_socket_path(&run_dir(home));
    let status_url = status_sock
        .exists()
        .then(|| format!("unix://{}", status_sock.display()));
    let mut cmd = Command::new(&args.argv[0]);
    cmd.args(&args.argv[1..])
        .current_dir(&args.cwd)
        .env_clear()
        .envs(env::compose(
            std::env::vars(),
            &args.env,
            status_url.as_deref(),
        ))
        .stdin(Stdio::piped())
        .process_group(0);
    let mut child = cmd.spawn()?;
    let pid = child.id() as i32;
    let mut child_in = child.stdin.take();
    std::thread::Builder::new()
        .name("relay".into())
        .spawn(move || {
            let mut stdin = std::io::stdin().lock();
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match stdin.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let ok = child_in
                            .as_mut()
                            .map(|c| c.write_all(&buf[..n]).and_then(|_| c.flush()).is_ok())
                            .unwrap_or(false);
                        if !ok {
                            break;
                        }
                    }
                }
            }
            drop(child_in.take());
            std::thread::sleep(Duration::from_secs(3));
            // SAFETY: kill(2) on the group this process started.
            unsafe {
                libc::kill(-pid, libc::SIGTERM);
            }
            std::thread::sleep(Duration::from_secs(2));
            // SAFETY: as above.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        })?;
    let status = child.wait()?;
    Ok(status.code().unwrap_or(128 + 9))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn f_rm_01_run_arguments() {
        let a = parse_run(&s(&[
            "--cwd",
            "/w",
            "--env",
            "TERM=dumb",
            "--",
            "claude",
            "-p",
            "--x",
        ]))
        .expect("parsed");
        assert_eq!(a.cwd, PathBuf::from("/w"));
        assert_eq!(a.env.get("TERM").map(String::as_str), Some("dumb"));
        assert_eq!(a.argv, ["claude", "-p", "--x"]);
        assert!(parse_run(&s(&["--cwd", "/w", "claude"])).is_err());
        assert!(parse_run(&s(&["--", "claude"])).is_err());
    }
}
