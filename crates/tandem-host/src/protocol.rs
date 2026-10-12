//! The `tandem-host` wire protocol (ARCHITECTURE §13.1): newline-delimited
//! JSON-RPC 2.0 over the SSH channel's stdio, binary data as base64. The
//! desktop and the host share these types; the desktop reads the git and
//! file shapes straight into its own (same JSON).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bumped whenever a method or a shape changes incompatibly. The host is
/// installed under `~/.tandem-host/<PROTOCOL>/`.
pub const PROTOCOL: u32 = 1;
/// This build's version (the desktop's, since they ship together).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Longest line either side accepts (a 10 MB file read is ~14 MB of base64).
pub const MAX_LINE: usize = 32 * 1024 * 1024;

/// One line on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub jsonrpc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Message {
    pub fn request(id: u64, method: &str, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: Some(id),
            method: Some(method.into()),
            params: Some(params),
            result: None,
            error: None,
        }
    }

    pub fn notification(method: &str, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: None,
            method: Some(method.into()),
            params: Some(params),
            result: None,
            error: None,
        }
    }

    pub fn reply(id: u64, outcome: Result<Value, RpcError>) -> Self {
        let (result, error) = match outcome {
            Ok(v) => (Some(v), None),
            Err(e) => (None, Some(e)),
        };
        Self {
            jsonrpc: "2.0".into(),
            id: Some(id),
            method: None,
            params: None,
            result,
            error,
        }
    }

    /// The line to write, newline included.
    pub fn to_line(&self) -> Vec<u8> {
        let mut out = serde_json::to_vec(self).unwrap_or_else(|_| b"{}".to_vec());
        out.push(b'\n');
        out
    }
}

/// What went wrong, in a form both sides map to words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorKind {
    /// `..` or a symlink left the workspace root (F-RM-01 AC3).
    PathEscape,
    NotFound,
    Invalid,
    /// The file changed since it was read.
    Conflict,
    Exists,
    ReadOnly,
    Io,
    Unsupported,
    /// A malformed message or an unknown method.
    Protocol,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{message}")]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub kind: ErrorKind,
}

impl RpcError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        let code = match kind {
            ErrorKind::Protocol => -32600,
            _ => -32000,
        };
        Self {
            code,
            message: message.into(),
            kind,
        }
    }
}

// ── handshake ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    pub client_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HelloReply {
    pub protocol: u32,
    pub host_version: String,
    /// `linux` or `macos`.
    pub os: String,
    pub arch: String,
    pub home: String,
    /// The session daemon's PID (it outlives connections).
    pub daemon_pid: u32,
}

// ── PTYs ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PtySpawn {
    pub id: String,
    /// Run this exactly. `None` runs the user's login shell, with
    /// `command` first when given (then the shell stays).
    #[serde(default)]
    pub argv: Option<Vec<String>>,
    #[serde(default)]
    pub command: Option<String>,
    pub cwd: String,
    /// Filtered by [`crate::env::allowed`] on both sides.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PtySpawned {
    pub pid: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PtyId {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PtyWrite {
    pub id: String,
    pub data_b64: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PtyResize {
    pub id: String,
    pub cols: u16,
    pub rows: u16,
}

/// Re-attach after a reconnect: output from `from_seq` (a byte offset into
/// the PTY's stream) is sent again, as far as the 1 MB buffer reaches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PtyAttach {
    pub id: String,
    pub from_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PtyAttached {
    pub pid: u32,
    /// Where the replay starts (later than asked when the buffer moved on).
    pub seq: u64,
    pub alive: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PtyEntry {
    pub id: String,
    pub pid: u32,
    pub alive: bool,
    pub exit_code: Option<i32>,
}

/// Notification `pty.output`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PtyOutput {
    pub id: String,
    /// Offset of the first byte in the PTY's stream.
    pub seq: u64,
    pub data_b64: String,
}

/// Notification `pty.exit`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PtyExit {
    pub id: String,
    pub code: Option<i32>,
}

// ── files ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Dir,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FsList {
    pub root: String,
    /// Relative to `root`; empty for the root itself.
    pub path: String,
    #[serde(default)]
    pub show_ignored: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub kind: EntryKind,
    pub symlink: bool,
    pub ignored: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FsPath {
    pub root: String,
    pub path: String,
}

/// `fs.watch`: a workspace folder on the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsWatch {
    pub root: String,
}

/// `fs.watch`'s answer, and what `fs.unwatch` takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsWatching {
    pub id: String,
}

/// `fs.changed`: one batch from a watch (`tandem_core::files::watch`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsChanged {
    pub id: String,
    pub change: tandem_core::files::watch::FsChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadKind {
    Text,
    Binary,
    Image,
}

/// `fs.read`: what [`tandem_core::files::read_file`] found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileData {
    /// Canonical, relative to the root.
    pub path: String,
    pub kind: ReadKind,
    pub text: Option<String>,
    pub size: u64,
    pub truncated: bool,
    pub hash: String,
    pub mtime: i64,
    pub data_url: Option<String>,
    pub hex: Option<String>,
    pub large: bool,
    pub writable: bool,
}

/// `fs.write`: text (keeps line endings and BOM) or raw bytes (new files
/// only: attachments).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FsWrite {
    pub root: String,
    pub path: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub data_b64: Option<String>,
    /// The hash from the read; `None` creates a new file.
    #[serde(default)]
    pub expected_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FsWritten {
    pub hash: String,
    pub mtime: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FsCreate {
    pub root: String,
    pub path: String,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FsRename {
    pub root: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FsStat {
    pub kind: EntryKind,
    pub size: u64,
    pub mtime: i64,
}

/// `fs.dirs`: the remote folder picker. Folders only, anywhere the user can
/// read (it's the local user choosing a workspace root).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FsDirs {
    /// Absolute, or empty / `~` for the home folder.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DirListing {
    /// Canonical.
    pub path: String,
    pub parent: Option<String>,
    pub dirs: Vec<String>,
}

// ── git ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitRoot {
    pub root: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitDiff {
    pub root: String,
    pub path: String,
    #[serde(default)]
    pub staged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitOpKind {
    Stage,
    Unstage,
    Discard,
    Commit,
    /// The branch's upstream (D-1601), with the host's own credentials.
    Push,
    /// The first push of a branch: `push -u <origin or the only remote>`.
    Publish,
    Pull,
    /// To `branch`: a local one, or a remote one by its tracking branch.
    Switch,
    /// A new branch `branch` from HEAD, switched to.
    CreateBranch,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitOp {
    pub root: String,
    pub op: GitOpKind,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub message: Option<String>,
    /// `switch` and `create_branch`.
    #[serde(default)]
    pub branch: Option<String>,
}

/// `lane.create`: a worktree from the HEAD of the checkout at `root`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneCreate {
    pub root: String,
    pub slug_source: String,
    /// Slugs reserved by other lanes of the same launch.
    #[serde(default)]
    pub also_taken: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneCreated {
    pub slug: String,
    pub branch: String,
    pub path: String,
    pub base: String,
}

/// `lane.remove`: the worktree at `path`, from the repository at `root`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneRemove {
    pub root: String,
    pub path: String,
    #[serde(default)]
    pub force: bool,
}

/// `lane.merge`: `branch` into the checkout at `root`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneMerge {
    pub root: String,
    pub branch: String,
}

/// The shape of `tandem_git::types::MergeReport`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaneMergeReport {
    pub merged: bool,
    pub conflicts: Vec<String>,
    pub commit: Option<String>,
}

/// `lane.diff`: the lane at `root` against the commit it started from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneDiff {
    pub root: String,
    pub base: String,
}

/// The shape of `tandem_git::types::StaleLane`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaneStale {
    pub branch: String,
    pub path: String,
    pub idle_days: u32,
}

/// `git.branches`'s rows: the shape of `tandem_git::types::Branch`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostBranch {
    pub name: String,
    pub current: bool,
    pub remote: bool,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
}

// ── engines ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetectWanted {
    pub id: String,
    /// Command names to look for on the login shell's PATH, in order.
    pub commands: Vec<String>,
    /// Arguments that print the version (`--version`).
    #[serde(default)]
    pub version_args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnginesDetect {
    pub engines: Vec<DetectWanted>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Found {
    pub id: String,
    pub path: String,
    /// The version command's first line, cut at 200 characters.
    pub version: Option<String>,
}

// ── status relay ───────────────────────────────────────────────────────

/// Notification `status.event`: a hook's report, exactly as posted to the
/// host's status socket. The desktop checks its signature with the pane's
/// secret, as it does for local hooks; the host never sees the secret's
/// use beyond passing it to the pane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusEvent {
    pub body_b64: String,
    pub signature: Option<String>,
}

/// A stdio MCP channel (D-1604): `mcp.open`/`mcp.close` from the desktop,
/// `mcp.attach` from `tandem-host mcp --channel <id>` on the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpChannel {
    pub channel: String,
    /// `mcp.open`: the channel outlives this connection (a terminal agent's
    /// server, which re-attaches with its PTY): the daemon holds its
    /// server's lines until the desktop opens it again by the same id.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep: bool,
}

/// One JSON-RPC line on a channel, without its newline: `mcp.in` (desktop →
/// daemon), `mcp.out` (daemon → desktop) and `mcp.line` (daemon ↔ relay).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpLine {
    pub channel: String,
    pub line: String,
}

pub mod method {
    pub const HELLO: &str = "host.hello";
    pub const ENGINES_DETECT: &str = "engines.detect";
    pub const PTY_SPAWN: &str = "pty.spawn";
    pub const PTY_WRITE: &str = "pty.write";
    pub const PTY_RESIZE: &str = "pty.resize";
    pub const PTY_KILL: &str = "pty.kill";
    pub const PTY_ATTACH: &str = "pty.attach";
    pub const PTY_LIST: &str = "pty.list";
    pub const FS_LIST: &str = "fs.list";
    pub const FS_READ: &str = "fs.read";
    pub const FS_WRITE: &str = "fs.write";
    pub const FS_STAT: &str = "fs.stat";
    pub const FS_CREATE: &str = "fs.create";
    pub const FS_RENAME: &str = "fs.rename";
    pub const FS_DELETE: &str = "fs.delete";
    pub const FS_DIRS: &str = "fs.dirs";
    pub const GIT_STATUS: &str = "git.status";
    pub const GIT_DIFF: &str = "git.diff";
    pub const GIT_OP: &str = "git.op";
    /// Worktree lanes on the host (D-1602): `LaneCreate` → `LaneCreated`,
    /// `LaneRemove`, `LaneMerge` → `LaneMergeReport`, `LaneDiff` →
    /// `Vec<FileDiff>`, `GitRoot` → `Vec<LaneStale>`.
    pub const LANE_CREATE: &str = "lane.create";
    pub const LANE_REMOVE: &str = "lane.remove";
    pub const LANE_MERGE: &str = "lane.merge";
    pub const LANE_DIFF: &str = "lane.diff";
    pub const LANE_SWEEP: &str = "lane.sweep";
    /// `GitRoot` → `Vec<HostBranch>` (D-1601).
    pub const GIT_BRANCHES: &str = "git.branches";
    /// Watch a folder (`FsWatch`), answered with `FsWatching`; batches come
    /// as `fs.changed` notifications to this connection only, until
    /// `fs.unwatch` or the connection closes (D-1600).
    pub const FS_WATCH: &str = "fs.watch";
    pub const FS_UNWATCH: &str = "fs.unwatch";
    /// Stdio MCP channels (D-1604). The desktop opens a channel
    /// (`McpChannel`) for one engine session; the engine's
    /// `tandem-host mcp --channel <id>` attaches to it, and lines pass as
    /// notifications: `mcp.in` from the desktop, `mcp.out` to it, `mcp.line`
    /// between the daemon and the relay. `mcp.closed` tells the desktop the
    /// relay went (the engine stopped its server). A channel belongs to the
    /// connection that opened it and ends with it.
    pub const MCP_OPEN: &str = "mcp.open";
    pub const MCP_CLOSE: &str = "mcp.close";
    pub const MCP_ATTACH: &str = "mcp.attach";
    pub const MCP_IN: &str = "mcp.in";
    pub const MCP_LINE: &str = "mcp.line";
    // Host → client notifications.
    pub const PTY_OUTPUT: &str = "pty.output";
    pub const PTY_EXIT: &str = "pty.exit";
    pub const STATUS_EVENT: &str = "status.event";
    pub const FS_CHANGED: &str = "fs.changed";
    pub const MCP_OUT: &str = "mcp.out";
    pub const MCP_CLOSED: &str = "mcp.closed";
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn f_rm_01_messages_round_trip_as_one_line_each() {
        let req = Message::request(
            7,
            method::PTY_RESIZE,
            json!({"id": "p", "cols": 80, "rows": 24}),
        );
        let line = req.to_line();
        assert_eq!(line.last(), Some(&b'\n'));
        assert_eq!(line.iter().filter(|b| **b == b'\n').count(), 1);
        let back: Message = serde_json::from_slice(&line).expect("parse");
        assert_eq!(back, req);
        let err = Message::reply(7, Err(RpcError::new(ErrorKind::PathEscape, "outside")));
        let text = String::from_utf8(err.to_line()).expect("utf8");
        assert!(text.contains("\"PATH_ESCAPE\""), "{text}");
        assert!(!text.contains("\"result\""));
    }
}
