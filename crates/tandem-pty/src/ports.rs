//! The Ports panel's probe (F-EX-03): TCP ports listening on this machine,
//! kept only when the listening process is in a pane's process tree.
//!
//! One [`PortProbe`] per platform, each a parser over what the OS reports,
//! unit-tested on recorded output:
//! - Linux: `/proc/net/tcp` and `/proc/net/tcp6` (state `0A`, LISTEN) and
//!   each candidate process's `/proc/<pid>/fd` links (`socket:[inode]`);
//! - macOS: `lsof -nP -iTCP -sTCP:LISTEN -Fpcn`;
//! - Windows: `netstat -ano -p tcp` and `-p tcpv6`.
//!
//! Process trees, names and start times come from `sysinfo` everywhere.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A listening socket and the process that owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listener {
    pub port: u16,
    /// As shown: `127.0.0.1`, `::1`, `0.0.0.0`, `::`, or an interface address.
    pub address: String,
    /// Bound to every interface (`0.0.0.0`, `::`, `*`): reachable from the network.
    pub all_interfaces: bool,
    pub pid: u32,
}

/// One process in a pane's tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
    /// Seconds since the epoch, as the OS reports it (pid reuse check).
    #[ts(type = "number")]
    pub start_time: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum PortsError {
    #[error("couldn't list ports: {0}")]
    Probe(String),
}

/// Lists listening sockets owned by `pids`.
pub trait PortProbe: Send + Sync {
    fn listeners(&self, pids: &HashSet<u32>) -> Result<Vec<Listener>, PortsError>;
}

/// The platform's probe.
pub fn system_probe() -> Box<dyn PortProbe> {
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::ProcProbe::default())
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::LsofProbe)
    }
    #[cfg(windows)]
    {
        Box::new(windows::NetstatProbe)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Box::new(NoProbe)
    }
}

/// Platforms without a probe list nothing.
pub struct NoProbe;

impl PortProbe for NoProbe {
    fn listeners(&self, _: &HashSet<u32>) -> Result<Vec<Listener>, PortsError> {
        Ok(Vec::new())
    }
}

fn is_all(addr: &str) -> bool {
    matches!(addr, "0.0.0.0" | "::" | "*" | "[::]")
}

/// Merges duplicates (a server on both `0.0.0.0` and `::`): one row per
/// pid and port, "all interfaces" when any socket is.
pub fn merge(listeners: Vec<Listener>) -> Vec<Listener> {
    let mut by: BTreeMap<(u16, u32), Listener> = BTreeMap::new();
    for l in listeners {
        by.entry((l.port, l.pid))
            .and_modify(|cur| {
                if l.all_interfaces && !cur.all_interfaces {
                    *cur = l.clone();
                }
            })
            .or_insert(l);
    }
    by.into_values().collect()
}

/// Every process in the trees rooted at `roots` (root pid → owner), with
/// names and start times, from one process-table read.
pub fn trees(roots: &[(String, u32)]) -> HashMap<u32, (String, ProcInfo)> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for (pid, p) in sys.processes() {
        if let Some(parent) = p.parent() {
            children
                .entry(parent.as_u32())
                .or_default()
                .push(pid.as_u32());
        }
    }
    let info = |pid: u32| -> Option<ProcInfo> {
        let p = sys.process(sysinfo::Pid::from_u32(pid))?;
        Some(ProcInfo {
            pid,
            name: p.name().to_string_lossy().into_owned(),
            start_time: p.start_time(),
        })
    };
    let mut out = HashMap::new();
    for (owner, root) in roots {
        let mut stack = vec![*root];
        while let Some(pid) = stack.pop() {
            if out.contains_key(&pid) {
                continue;
            }
            if let Some(i) = info(pid) {
                out.insert(pid, (owner.clone(), i));
            }
            if let Some(kids) = children.get(&pid) {
                stack.extend(kids.iter().copied());
            }
        }
    }
    out
}

/// Asks `pid` to stop (SIGTERM; `force`: SIGKILL). False when it's gone.
pub fn signal(pid: u32, force: bool) -> bool {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, Signal, System};
    let mut sys = System::new();
    let target = Pid::from_u32(pid);
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[target]),
        true,
        ProcessRefreshKind::nothing(),
    );
    let Some(p) = sys.process(target) else {
        return false;
    };
    if force {
        p.kill()
    } else {
        p.kill_with(Signal::Term).unwrap_or_else(|| p.kill())
    }
}

/// `pid` still runs with this start time.
pub fn alive(pid: u32, start_time: u64) -> bool {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = System::new();
    let target = Pid::from_u32(pid);
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[target]),
        true,
        ProcessRefreshKind::nothing(),
    );
    sys.process(target)
        .is_some_and(|p| p.start_time() == start_time)
}

/// `/proc/net/tcp{,6}`: (address, port, inode) of each LISTEN socket.
pub fn parse_proc_net(text: &str, v6: bool) -> Vec<(String, u16, u64)> {
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 10 || f[3] != "0A" {
            continue;
        }
        let Some((addr, port)) = f[1].split_once(':') else {
            continue;
        };
        let (Ok(port), Ok(inode)) = (u16::from_str_radix(port, 16), f[9].parse::<u64>()) else {
            continue;
        };
        let Some(address) = (if v6 { hex_v6(addr) } else { hex_v4(addr) }) else {
            continue;
        };
        out.push((address, port, inode));
    }
    out
}

/// `0100007F` → `127.0.0.1` (the kernel prints the u32 in host order).
fn hex_v4(hex: &str) -> Option<String> {
    let n = u32::from_str_radix(hex, 16).ok()?;
    Some(std::net::Ipv4Addr::from(n.to_le_bytes()).to_string())
}

/// Four host-order u32 words; v4-mapped addresses print as v4.
fn hex_v6(hex: &str) -> Option<String> {
    if hex.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for (i, chunk) in bytes.chunks_mut(4).enumerate() {
        let word = u32::from_str_radix(&hex[i * 8..i * 8 + 8], 16).ok()?;
        chunk.copy_from_slice(&word.to_le_bytes());
    }
    let ip = std::net::Ipv6Addr::from(bytes);
    Some(match ip.to_ipv4_mapped() {
        Some(v4) => v4.to_string(),
        None => ip.to_string(),
    })
}

/// `lsof -Fpcn` records: `p<pid>`, `c<command>`, `n<addr>:<port>`.
pub fn parse_lsof(text: &str) -> Vec<Listener> {
    let mut out = Vec::new();
    let mut pid = None;
    for line in text.lines() {
        let (tag, rest) = line.split_at(line.len().min(1));
        match tag {
            "p" => pid = rest.parse::<u32>().ok(),
            "n" => {
                let (Some(pid), Some((addr, port))) = (pid, rest.rsplit_once(':')) else {
                    continue;
                };
                let Ok(port) = port.parse::<u16>() else {
                    continue;
                };
                let addr = addr
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .to_string();
                out.push(Listener {
                    port,
                    all_interfaces: is_all(&addr),
                    address: if addr == "*" { "0.0.0.0".into() } else { addr },
                    pid,
                });
            }
            _ => {}
        }
    }
    out
}

/// `netstat -ano` rows: `TCP <local> <remote> LISTENING <pid>`.
pub fn parse_netstat(text: &str) -> Vec<Listener> {
    let mut out = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 5 || !f[0].eq_ignore_ascii_case("TCP") || f[3] != "LISTENING" {
            continue;
        }
        let (Some((addr, port)), Ok(pid)) = (f[1].rsplit_once(':'), f[4].parse::<u32>()) else {
            continue;
        };
        let Ok(port) = port.parse::<u16>() else {
            continue;
        };
        let addr = addr
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
        out.push(Listener {
            port,
            all_interfaces: is_all(&addr),
            address: addr,
            pid,
        });
    }
    out
}

#[cfg(target_os = "linux")]
pub mod linux {
    use super::*;

    /// Reads `/proc` (a different root in tests).
    pub struct ProcProbe {
        pub root: std::path::PathBuf,
    }

    impl Default for ProcProbe {
        fn default() -> Self {
            Self {
                root: "/proc".into(),
            }
        }
    }

    impl PortProbe for ProcProbe {
        fn listeners(&self, pids: &HashSet<u32>) -> Result<Vec<Listener>, PortsError> {
            let mut sockets: HashMap<u64, (String, u16)> = HashMap::new();
            for (file, v6) in [("net/tcp", false), ("net/tcp6", true)] {
                if let Ok(text) = std::fs::read_to_string(self.root.join(file)) {
                    for (addr, port, inode) in parse_proc_net(&text, v6) {
                        sockets.insert(inode, (addr, port));
                    }
                }
            }
            if sockets.is_empty() {
                return Ok(Vec::new());
            }
            let mut out = Vec::new();
            for pid in pids {
                let Ok(fds) = std::fs::read_dir(self.root.join(pid.to_string()).join("fd")) else {
                    continue;
                };
                for fd in fds.flatten() {
                    let Ok(link) = std::fs::read_link(fd.path()) else {
                        continue;
                    };
                    let link = link.to_string_lossy();
                    let Some(inode) = link
                        .strip_prefix("socket:[")
                        .and_then(|s| s.strip_suffix(']'))
                        .and_then(|s| s.parse::<u64>().ok())
                    else {
                        continue;
                    };
                    if let Some((addr, port)) = sockets.get(&inode) {
                        out.push(Listener {
                            port: *port,
                            all_interfaces: is_all(addr),
                            address: addr.clone(),
                            pid: *pid,
                        });
                    }
                }
            }
            Ok(merge(out))
        }
    }
}

#[cfg(target_os = "macos")]
pub mod macos {
    use super::*;

    pub struct LsofProbe;

    impl PortProbe for LsofProbe {
        fn listeners(&self, pids: &HashSet<u32>) -> Result<Vec<Listener>, PortsError> {
            let out = std::process::Command::new("lsof")
                .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-Fpcn"])
                .output()
                .map_err(|e| PortsError::Probe(e.to_string()))?;
            let text = String::from_utf8_lossy(&out.stdout);
            Ok(merge(
                parse_lsof(&text)
                    .into_iter()
                    .filter(|l| pids.contains(&l.pid))
                    .collect(),
            ))
        }
    }
}

#[cfg(windows)]
pub mod windows {
    use super::*;

    pub struct NetstatProbe;

    impl PortProbe for NetstatProbe {
        fn listeners(&self, pids: &HashSet<u32>) -> Result<Vec<Listener>, PortsError> {
            let mut all = Vec::new();
            for proto in ["tcp", "tcpv6"] {
                let out = std::process::Command::new("netstat")
                    .args(["-ano", "-p", proto])
                    .output()
                    .map_err(|e| PortsError::Probe(e.to_string()))?;
                all.extend(parse_netstat(&String::from_utf8_lossy(&out.stdout)));
            }
            Ok(merge(
                all.into_iter().filter(|l| pids.contains(&l.pid)).collect(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROC_TCP: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:1435 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 41001 1 0000000000000000 100 0 0 10 0
   1: 00000000:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 41002 1 0000000000000000 100 0 0 10 0
   2: 0100007F:1435 0100007F:C350 01 00000000:00000000 00:00000000 00000000  1000        0 41003 1 0000000000000000 20 4 30 10 -1
";
    const PROC_TCP6: &str = "  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000001000000:0BB8 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 41004 1 0000000000000000 100 0 0 10 0
   1: 00000000000000000000000000000000:1F90 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 41005 1 0000000000000000 100 0 0 10 0
   2: 0000000000000000FFFF00000100007F:2328 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 41006 1 0000000000000000 100 0 0 10 0
";

    #[test]
    fn f_ex_03_linux_listen_sockets_parse_with_their_addresses() {
        assert_eq!(
            parse_proc_net(PROC_TCP, false),
            vec![
                ("127.0.0.1".into(), 5173, 41001),
                ("0.0.0.0".into(), 8080, 41002)
            ]
        );
        assert_eq!(
            parse_proc_net(PROC_TCP6, true),
            vec![
                ("::1".into(), 3000, 41004),
                ("::".into(), 8080, 41005),
                ("127.0.0.1".into(), 9000, 41006)
            ]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn f_ex_03_1_linux_probe_keeps_only_the_given_processes() {
        let root = tempfile::tempdir().expect("tmp");
        std::fs::create_dir_all(root.path().join("net")).expect("mkdir");
        std::fs::write(root.path().join("net/tcp"), PROC_TCP).expect("write");
        std::fs::write(root.path().join("net/tcp6"), PROC_TCP6).expect("write");
        for (pid, inode) in [(100, 41001), (100, 41005), (200, 41002)] {
            let fd = root.path().join(format!("{pid}/fd"));
            std::fs::create_dir_all(&fd).expect("mkdir");
            std::os::unix::fs::symlink(format!("socket:[{inode}]"), fd.join(inode.to_string()))
                .expect("link");
        }
        let probe = linux::ProcProbe {
            root: root.path().to_path_buf(),
        };
        let got = probe.listeners(&HashSet::from([100])).expect("probe");
        assert_eq!(
            got,
            vec![
                Listener {
                    port: 5173,
                    address: "127.0.0.1".into(),
                    all_interfaces: false,
                    pid: 100
                },
                Listener {
                    port: 8080,
                    address: "::".into(),
                    all_interfaces: true,
                    pid: 100
                },
            ]
        );
    }

    #[test]
    fn f_ex_03_5_macos_lsof_output_parses_and_marks_all_interfaces() {
        let text = "p4120\ncnode\nn127.0.0.1:5173\nn[::1]:5173\np4300\ncpython3\nn*:8000\n";
        let got = merge(parse_lsof(text));
        assert_eq!(
            got,
            vec![
                Listener {
                    port: 5173,
                    address: "127.0.0.1".into(),
                    all_interfaces: false,
                    pid: 4120
                },
                Listener {
                    port: 8000,
                    address: "0.0.0.0".into(),
                    all_interfaces: true,
                    pid: 4300
                },
            ]
        );
    }

    #[test]
    fn f_ex_03_5_windows_netstat_output_parses() {
        let text = "\nActive Connections\n\n  Proto  Local Address          Foreign Address        State           PID\n  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1044\n  TCP    127.0.0.1:5173         0.0.0.0:0              LISTENING       8812\n  TCP    127.0.0.1:5173         127.0.0.1:50000        ESTABLISHED     8812\n  TCP    [::]:3000              [::]:0                 LISTENING       9001\n";
        assert_eq!(
            parse_netstat(text),
            vec![
                Listener {
                    port: 135,
                    address: "0.0.0.0".into(),
                    all_interfaces: true,
                    pid: 1044
                },
                Listener {
                    port: 5173,
                    address: "127.0.0.1".into(),
                    all_interfaces: false,
                    pid: 8812
                },
                Listener {
                    port: 3000,
                    address: "::".into(),
                    all_interfaces: true,
                    pid: 9001
                },
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn f_ex_03_1_trees_follow_children_and_name_them() {
        let mut child = std::process::Command::new("sh")
            .args(["-c", "sleep 30 & wait"])
            .spawn()
            .expect("spawn");
        std::thread::sleep(std::time::Duration::from_millis(300));
        let t = trees(&[("pane".into(), child.id())]);
        assert!(t.contains_key(&child.id()));
        assert!(
            t.values()
                .any(|(owner, p)| owner == "pane" && p.name == "sleep"),
            "{t:?}"
        );
        let (_, root) = &t[&child.id()];
        assert!(alive(child.id(), root.start_time));
        assert!(
            !alive(child.id(), root.start_time + 1),
            "a different start time is another process"
        );
        let _ = child.kill();
        let _ = child.wait();
    }
}
