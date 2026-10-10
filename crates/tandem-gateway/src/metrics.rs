//! The gateway's own numbers (F-PL-01 "watchdog and memory metrics"): its
//! leases, MCP sessions, requests, restarts and the memory of the process it
//! runs in. Memory comes from a per-platform probe.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GatewayMetrics {
    /// The port every session's MCP config points at.
    pub port: u16,
    /// Listeners this gateway has open: one while it serves.
    pub listeners: u32,
    pub leases: u32,
    pub mcp_sessions: u32,
    pub requests: u64,
    /// Times the watchdog had to bring the listener back.
    pub restarts: u32,
    /// Resident memory of the process, where the platform reports it.
    #[ts(type = "number | null")]
    pub rss_bytes: Option<u64>,
}

/// Reads the process's resident memory.
pub trait MemoryProbe: Send + Sync {
    fn rss_bytes(&self) -> Option<u64>;
}

#[cfg(target_os = "linux")]
mod platform {
    use super::MemoryProbe;

    /// `VmRSS` from `/proc/self/status`.
    pub struct ProcStatus;

    impl MemoryProbe for ProcStatus {
        fn rss_bytes(&self) -> Option<u64> {
            let status = std::fs::read_to_string("/proc/self/status").ok()?;
            super::parse_vm_rss(&status)
        }
    }

    pub fn probe() -> Box<dyn MemoryProbe> {
        Box::new(ProcStatus)
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    use super::MemoryProbe;

    /// macOS and Windows report no figure yet (the metric says so: `null`).
    pub struct Unreported;

    impl MemoryProbe for Unreported {
        fn rss_bytes(&self) -> Option<u64> {
            None
        }
    }

    pub fn probe() -> Box<dyn MemoryProbe> {
        Box::new(Unreported)
    }
}

pub fn probe() -> Box<dyn MemoryProbe> {
    platform::probe()
}

/// `VmRSS:   12345 kB` → bytes.
#[allow(dead_code)]
pub(crate) fn parse_vm_rss(status: &str) -> Option<u64> {
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    let mut parts = line.split_whitespace().skip(1);
    let n: u64 = parts.next()?.parse().ok()?;
    match parts.next() {
        Some("kB") | None => Some(n * 1024),
        Some("mB") | Some("MB") => Some(n * 1024 * 1024),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_pl_01_the_memory_figure_is_read_from_vm_rss() {
        let fake = "Name:\ttandem\nVmPeak:\t  900 kB\nVmRSS:\t   51200 kB\nThreads:\t9\n";
        assert_eq!(parse_vm_rss(fake), Some(51200 * 1024));
        assert_eq!(parse_vm_rss("Name:\tx\n"), None);
        #[cfg(target_os = "linux")]
        assert!(probe().rss_bytes().is_some_and(|b| b > 0));
    }
}
