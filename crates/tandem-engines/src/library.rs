//! The session library (F-TH-17, ARCHITECTURE §2.5): the Claude Code and
//! Codex sessions on this computer, read from the engines' own session files.
//!
//! Only `projects/<folder>/*.jsonl` (Claude Code) and `sessions/**/*.jsonl`
//! (Codex) under a home are ever opened, through [`SessionFs`]; symbolic
//! links are never followed, so nothing else in a home, a credential file
//! least of all, is read (F-TH-17 AC5). Text read here is never logged.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// Files past this are skipped, not read.
pub const MAX_FILE: u64 = 64 * 1024 * 1024;
/// Lines past this are skipped.
pub const MAX_LINE: usize = 4 * 1024 * 1024;
/// A transcript keeps its last this many turns.
pub const MAX_ITEMS: usize = 2000;
/// Titles are cut to this many characters.
pub const TITLE_CHARS: usize = 80;
/// Codex's dated folders: `sessions/YYYY/MM/DD/`, and a little room.
const CODEX_DEPTH: usize = 5;

/// What a folder holds, as [`SessionFs::entries`] sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    /// A symbolic link, never followed.
    Link,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsEntry {
    pub path: PathBuf,
    pub kind: EntryKind,
    pub len: u64,
    /// Last modified, Unix ms.
    pub modified_ms: i64,
}

/// The file system as the library sees it: the real one, or a recording
/// fake in tests.
pub trait SessionFs: Send + Sync {
    fn entries(&self, dir: &Path) -> Vec<FsEntry>;
    fn open(&self, path: &Path) -> std::io::Result<Box<dyn Read + Send>>;
    /// A folder, not a link to one.
    fn is_real_dir(&self, path: &Path) -> bool;
}

/// The real file system.
pub struct RealFs;

impl SessionFs for RealFs {
    fn entries(&self, dir: &Path) -> Vec<FsEntry> {
        let Ok(read) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        read.flatten()
            .filter_map(|e| {
                let kind = e.file_type().ok()?;
                let kind = if kind.is_symlink() {
                    EntryKind::Link
                } else if kind.is_dir() {
                    EntryKind::Dir
                } else if kind.is_file() {
                    EntryKind::File
                } else {
                    return None;
                };
                // `DirEntry::metadata` doesn't follow links.
                let meta = e.metadata().ok()?;
                let modified_ms = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                Some(FsEntry {
                    path: e.path(),
                    kind,
                    len: meta.len(),
                    modified_ms,
                })
            })
            .collect()
    }

    fn open(&self, path: &Path) -> std::io::Result<Box<dyn Read + Send>> {
        let mut opts = std::fs::OpenOptions::new();
        opts.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // Not through a link, and a FIFO swapped in can't block.
            opts.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = opts.open(path)?;
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(std::io::Error::other("not a regular file"));
        }
        if meta.len() > MAX_FILE {
            return Err(std::io::Error::other("too large"));
        }
        Ok(Box::new(file.take(MAX_FILE)))
    }

    fn is_real_dir(&self, path: &Path) -> bool {
        std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
    }
}

/// Which engine's format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LibraryEngine {
    Claude,
    Codex,
}

impl LibraryEngine {
    pub fn of(engine_id: &str) -> Option<Self> {
        match engine_id {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    /// The engine's own home outside Vibezeo: its env var in the login
    /// environment (`env`), else its folder in the user's home.
    pub fn own_home(self, user_home: &Path, env: impl Fn(&str) -> Option<String>) -> PathBuf {
        let (var, dir) = match self {
            Self::Claude => ("CLAUDE_CONFIG_DIR", ".claude"),
            Self::Codex => ("CODEX_HOME", ".codex"),
        };
        env(var)
            .filter(|v| Path::new(v).is_absolute())
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home.join(dir))
    }
}

/// One home to read: an account's provider home, or the engine's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibrarySource {
    pub engine: LibraryEngine,
    /// `None`: the engine's own home, outside Vibezeo.
    pub account_id: Option<String>,
    pub account_label: Option<String>,
    pub home: PathBuf,
}

/// A session file, as listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub engine: LibraryEngine,
    pub account_id: Option<String>,
    pub account_label: Option<String>,
    pub home: PathBuf,
    pub file: PathBuf,
    pub session_id: String,
    pub folder: Option<String>,
    pub title: Option<String>,
    pub size: u64,
    pub modified_ms: i64,
}

/// The listing: sessions and how many files couldn't be read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scan {
    pub sessions: Vec<Found>,
    pub skipped: u32,
}

/// The session files under one home, by the engine's layout.
pub fn session_files(fs: &dyn SessionFs, source: &LibrarySource) -> Vec<FsEntry> {
    let mut out = Vec::new();
    let jsonl = |e: &FsEntry| {
        e.kind == EntryKind::File
            && e.path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".jsonl"))
    };
    match source.engine {
        // The home itself may be a link (a dotfiles manager's); the folders
        // in it never are followed.
        LibraryEngine::Claude => {
            let projects = source.home.join("projects");
            if !fs.is_real_dir(&projects) {
                return out;
            }
            for dir in fs.entries(&projects) {
                if dir.kind != EntryKind::Dir {
                    continue;
                }
                out.extend(fs.entries(&dir.path).into_iter().filter(|e| jsonl(e)));
            }
        }
        LibraryEngine::Codex => {
            let sessions = source.home.join("sessions");
            if !fs.is_real_dir(&sessions) {
                return out;
            }
            let mut stack = vec![(sessions, 0usize)];
            while let Some((dir, depth)) = stack.pop() {
                for e in fs.entries(&dir) {
                    if e.kind == EntryKind::Dir && depth < CODEX_DEPTH {
                        stack.push((e.path.clone(), depth + 1));
                    } else if jsonl(&e) {
                        out.push(e);
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// The JSON lines of a session file; `None` when it can't be opened.
fn lines(fs: &dyn SessionFs, path: &Path) -> Option<impl Iterator<Item = Option<Value>>> {
    let mut reader = BufReader::new(fs.open(path).ok()?);
    Some(
        std::iter::from_fn(move || bounded_line(&mut reader)).filter_map(|line| {
            let buf = line?;
            let trimmed = buf.trim_ascii();
            if trimmed.is_empty() {
                return None;
            }
            Some(serde_json::from_slice::<Value>(trimmed).ok())
        }),
    )
}

/// The next line, read at most [`MAX_LINE`] bytes at a time: a longer line
/// is skipped to its newline without being held, as `Some(None)`. `None` at
/// the end.
fn bounded_line(reader: &mut impl BufRead) -> Option<Option<Vec<u8>>> {
    let mut buf = Vec::new();
    let n = reader
        .by_ref()
        .take(MAX_LINE as u64 + 1)
        .read_until(b'\n', &mut buf)
        .ok()?;
    if n == 0 {
        return None;
    }
    if buf.len() > MAX_LINE && buf.last() != Some(&b'\n') {
        // Too long: drop the rest of it, a chunk at a time.
        loop {
            let chunk = reader.fill_buf().ok()?;
            if chunk.is_empty() {
                break;
            }
            match chunk.iter().position(|b| *b == b'\n') {
                Some(at) => {
                    reader.consume(at + 1);
                    break;
                }
                None => {
                    let len = chunk.len();
                    reader.consume(len);
                }
            }
        }
        return Some(None);
    }
    Some(Some(buf))
}

fn str_at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut at = v;
    for key in path {
        at = at.get(key)?;
    }
    at.as_str()
}

/// Text an engine added to a prompt itself (Codex's `<environment_context>`,
/// Claude Code's command wrappers): not the user's words.
fn injected(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with('<') || t.is_empty()
}

fn clip(text: &str, chars: usize) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match one_line.char_indices().nth(chars) {
        Some((cut, _)) => format!("{}…", &one_line[..cut]),
        None => one_line,
    }
}

/// One turn of a transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TranscriptItem {
    pub role: TranscriptRole,
    /// The text; a tool call's name only.
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TranscriptRole {
    User,
    Assistant,
    Tool,
}

/// A line's turns, by the engine's format. Secrets in the text are
/// redacted here, so no title, transcript or index entry ever holds one
/// (D-2416).
fn items_of(engine: LibraryEngine, v: &Value) -> Vec<TranscriptItem> {
    let item = |role, text: &str| TranscriptItem {
        role,
        text: tandem_core::redact::Redactor::global()
            .redact(text)
            .into_owned(),
    };
    match engine {
        LibraryEngine::Claude => {
            let role = match v.get("type").and_then(Value::as_str) {
                Some("user") => TranscriptRole::User,
                Some("assistant") => TranscriptRole::Assistant,
                _ => return Vec::new(),
            };
            if v.get("isMeta").and_then(Value::as_bool) == Some(true) {
                return Vec::new();
            }
            match v.pointer("/message/content") {
                Some(Value::String(s)) if !injected(s) => vec![item(role, s)],
                Some(Value::Array(blocks)) => blocks
                    .iter()
                    .filter_map(|b| match b.get("type").and_then(Value::as_str) {
                        Some("text") => b
                            .get("text")
                            .and_then(Value::as_str)
                            .filter(|t| !injected(t))
                            .map(|t| item(role, t)),
                        Some("tool_use") => b
                            .get("name")
                            .and_then(Value::as_str)
                            .map(|n| item(TranscriptRole::Tool, n)),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            }
        }
        LibraryEngine::Codex => {
            if v.get("type").and_then(Value::as_str) != Some("response_item") {
                return Vec::new();
            }
            let Some(p) = v.get("payload") else {
                return Vec::new();
            };
            match p.get("type").and_then(Value::as_str) {
                Some("message") => {
                    let role = match p.get("role").and_then(Value::as_str) {
                        Some("user") => TranscriptRole::User,
                        Some("assistant") => TranscriptRole::Assistant,
                        _ => return Vec::new(),
                    };
                    p.get("content")
                        .and_then(Value::as_array)
                        .map(|blocks| {
                            blocks
                                .iter()
                                .filter_map(|b| b.get("text").and_then(Value::as_str))
                                .filter(|t| !injected(t))
                                .map(|t| item(role, t))
                                .collect()
                        })
                        .unwrap_or_default()
                }
                Some("function_call" | "custom_tool_call" | "local_shell_call") => p
                    .get("name")
                    .and_then(Value::as_str)
                    .map(|n| vec![item(TranscriptRole::Tool, n)])
                    .unwrap_or_default(),
                _ => Vec::new(),
            }
        }
    }
}

/// What a session file says about itself: its id, folder and first prompt.
/// `None` when it doesn't read as the engine's session file.
pub fn summarize(fs: &dyn SessionFs, source: &LibrarySource, entry: &FsEntry) -> Option<Found> {
    if entry.len > MAX_FILE {
        return None;
    }
    let mut session_id: Option<String> = None;
    let mut folder: Option<String> = None;
    let mut title: Option<String> = None;
    let mut parsed = false;
    for v in lines(fs, &entry.path)? {
        let Some(v) = v else { continue };
        parsed = true;
        match source.engine {
            LibraryEngine::Claude => {
                if session_id.is_none() {
                    session_id = str_at(&v, &["sessionId"]).map(str::to_string);
                }
                if folder.is_none() {
                    folder = str_at(&v, &["cwd"]).map(str::to_string);
                }
            }
            LibraryEngine::Codex => {
                if str_at(&v, &["type"]) == Some("session_meta") {
                    session_id = str_at(&v, &["payload", "id"]).map(str::to_string);
                    folder = str_at(&v, &["payload", "cwd"]).map(str::to_string);
                }
            }
        }
        if title.is_none() {
            title = items_of(source.engine, &v)
                .into_iter()
                .find(|i| i.role == TranscriptRole::User)
                .map(|i| clip(&i.text, TITLE_CHARS));
        }
        if session_id.is_some() && folder.is_some() && title.is_some() {
            break;
        }
    }
    if !parsed {
        return None;
    }
    // Claude Code names the file after the session.
    let session_id = session_id.or_else(|| match source.engine {
        LibraryEngine::Claude => entry
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_string),
        LibraryEngine::Codex => None,
    })?;
    if !valid_session_id(&session_id) {
        return None;
    }
    Some(Found {
        engine: source.engine,
        account_id: source.account_id.clone(),
        account_label: source.account_label.clone(),
        home: source.home.clone(),
        file: entry.path.clone(),
        session_id,
        folder,
        title,
        size: entry.len,
        modified_ms: entry.modified_ms,
    })
}

/// A session id as the engines write them: letters, digits and dashes,
/// safe as a resume argument.
pub fn valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Every session under `sources`, newest first.
pub fn scan(fs: &dyn SessionFs, sources: &[LibrarySource]) -> Scan {
    let mut out = Scan::default();
    for source in sources {
        for entry in session_files(fs, source) {
            match summarize(fs, source, &entry) {
                Some(found) => out.sessions.push(found),
                None => out.skipped += 1,
            }
        }
    }
    out.sessions
        .sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms).then(a.file.cmp(&b.file)));
    out
}

/// A session's transcript, in order.
pub fn transcript(
    fs: &dyn SessionFs,
    engine: LibraryEngine,
    file: &Path,
) -> Option<Vec<TranscriptItem>> {
    let mut out = std::collections::VecDeque::new();
    for v in lines(fs, file)?.flatten() {
        for item in items_of(engine, &v) {
            if out.len() == MAX_ITEMS {
                out.pop_front();
            }
            out.push_back(item);
        }
    }
    Some(out.into())
}

/// A transcript as plain text, for a handoff brief or the search index.
pub fn as_text(items: &[TranscriptItem]) -> String {
    items
        .iter()
        .map(|i| match i.role {
            TranscriptRole::User => format!("User: {}", i.text),
            TranscriptRole::Assistant => format!("Assistant: {}", i.text),
            TranscriptRole::Tool => format!("(used {})", i.text),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The repository a folder belongs to: the nearest parent with `.git`
/// (`exists` checks a path), else the folder itself.
pub fn repository(folder: &str, exists: impl Fn(&Path) -> bool) -> String {
    let start = Path::new(folder);
    let mut at = Some(start);
    while let Some(dir) = at {
        if exists(&dir.join(".git")) {
            return dir.to_string_lossy().into_owned();
        }
        at = dir.parent();
    }
    folder.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// An in-memory file system that records every file opened.
    #[derive(Default)]
    pub struct FakeFs {
        files: BTreeMap<PathBuf, Vec<u8>>,
        links: Vec<PathBuf>,
        pub opened: Mutex<Vec<PathBuf>>,
    }

    impl FakeFs {
        fn file(&mut self, path: &str, body: &str) {
            self.files
                .insert(PathBuf::from(path), body.as_bytes().to_vec());
        }
    }

    impl SessionFs for FakeFs {
        fn entries(&self, dir: &Path) -> Vec<FsEntry> {
            let mut seen = BTreeMap::new();
            for (path, body) in &self.files {
                let Ok(rest) = path.strip_prefix(dir) else {
                    continue;
                };
                let mut parts = rest.components();
                let Some(first) = parts.next() else { continue };
                let child = dir.join(first);
                let kind = if parts.next().is_some() {
                    EntryKind::Dir
                } else if self.links.contains(&child) {
                    EntryKind::Link
                } else {
                    EntryKind::File
                };
                seen.entry(child.clone()).or_insert(FsEntry {
                    path: child,
                    kind,
                    len: body.len() as u64,
                    modified_ms: body.len() as i64,
                });
            }
            seen.into_values().collect()
        }

        fn is_real_dir(&self, path: &Path) -> bool {
            !self.links.contains(&path.to_path_buf())
                && self.files.keys().any(|f| f.starts_with(path) && f != path)
        }

        fn open(&self, path: &Path) -> std::io::Result<Box<dyn Read + Send>> {
            self.opened.lock().expect("lock").push(path.to_path_buf());
            let body = self
                .files
                .get(path)
                .cloned()
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
            Ok(Box::new(std::io::Cursor::new(body)))
        }
    }

    fn claude_line(kind: &str, session: &str, cwd: &str, text: &str) -> String {
        serde_json::json!({
            "type": kind, "sessionId": session, "cwd": cwd, "timestamp": "2026-10-14T09:00:00Z",
            "message": {"role": kind, "content": [{"type": "text", "text": text}]}
        })
        .to_string()
    }

    fn codex_file(id: &str, cwd: &str, prompt: &str) -> String {
        [
            serde_json::json!({"type": "session_meta", "payload": {"id": id, "cwd": cwd}}),
            serde_json::json!({"type": "response_item", "payload": {"type": "message", "role": "user",
                "content": [{"type": "input_text", "text": "<environment_context>cwd</environment_context>"}]}}),
            serde_json::json!({"type": "response_item", "payload": {"type": "message", "role": "user",
                "content": [{"type": "input_text", "text": prompt}]}}),
            serde_json::json!({"type": "response_item", "payload": {"type": "function_call", "name": "shell",
                "arguments": "{\"cmd\":\"cat secret\"}"}}),
            serde_json::json!({"type": "response_item", "payload": {"type": "message", "role": "assistant",
                "content": [{"type": "output_text", "text": "Done."}]}}),
        ]
        .map(|v| v.to_string())
        .join("\n")
    }

    /// Two Claude Code provider homes and Codex's own home, with credential
    /// files next to the sessions.
    pub fn fixture() -> (FakeFs, Vec<LibrarySource>) {
        let mut fs = FakeFs::default();
        let a = "/data/providers/claude/work";
        let b = "/data/providers/claude/home";
        fs.file(&format!("{a}/.credentials.json"), "{\"token\":\"x\"}");
        fs.file(
            &format!("{a}/projects/-src-shop/aaaa-1111.jsonl"),
            &[
                claude_line(
                    "user",
                    "aaaa-1111",
                    "/src/shop",
                    "Fix the checkout total rounding",
                ),
                claude_line("assistant", "aaaa-1111", "/src/shop", "Fixed."),
            ]
            .join("\n"),
        );
        fs.file(
            &format!("{a}/projects/-src-shop-api/bbbb-2222.jsonl"),
            &claude_line(
                "user",
                "bbbb-2222",
                "/src/shop/api",
                "Add a health endpoint",
            ),
        );
        fs.file(
            &format!("{a}/projects/-src-x/broken.jsonl"),
            "not json at all\n{oops",
        );
        fs.file(
            &format!("{b}/projects/-src-blog/cccc-3333.jsonl"),
            &claude_line("user", "cccc-3333", "/src/blog", "Write the release notes"),
        );
        fs.file(&format!("{b}/settings.json"), "{}");
        let codex = "/home/ana/.codex";
        fs.file(&format!("{codex}/auth.json"), "{\"OPENAI_API_KEY\":\"x\"}");
        fs.file(
            &format!("{codex}/sessions/2026/10/14/rollout-2026-10-14-dddd.jsonl"),
            &codex_file("dddd-4444", "/src/shop", "Speed up the product search"),
        );
        fs.links.push(PathBuf::from(format!(
            "{codex}/sessions/2026/10/14/linked.jsonl"
        )));
        fs.file(&format!("{codex}/sessions/2026/10/14/linked.jsonl"), "{}");
        let source = |engine, account: Option<&str>, home: &str| LibrarySource {
            engine,
            account_id: account.map(str::to_string),
            account_label: account.map(str::to_string),
            home: PathBuf::from(home),
        };
        let sources = vec![
            source(LibraryEngine::Claude, Some("work"), a),
            source(LibraryEngine::Claude, Some("home"), b),
            source(LibraryEngine::Codex, None, codex),
        ];
        (fs, sources)
    }

    #[test]
    fn f_th_17_1_four_sessions_list_with_engine_account_and_folder_and_a_bad_file_is_skipped() {
        let (fs, sources) = fixture();
        let scan = scan(&fs, &sources);
        assert_eq!(scan.sessions.len(), 4, "{:?}", scan.sessions);
        assert_eq!(scan.skipped, 1);
        let get = |id: &str| scan.sessions.iter().find(|s| s.session_id == id).expect(id);
        let shop = get("aaaa-1111");
        assert_eq!(shop.engine, LibraryEngine::Claude);
        assert_eq!(shop.account_id.as_deref(), Some("work"));
        assert_eq!(shop.folder.as_deref(), Some("/src/shop"));
        assert_eq!(
            shop.title.as_deref(),
            Some("Fix the checkout total rounding")
        );
        assert_eq!(get("cccc-3333").account_id.as_deref(), Some("home"));
        let codex = get("dddd-4444");
        assert_eq!(codex.engine, LibraryEngine::Codex);
        assert_eq!(codex.account_id, None, "Codex's own home: outside Vibezeo");
        assert_eq!(codex.folder.as_deref(), Some("/src/shop"));
        assert_eq!(codex.title.as_deref(), Some("Speed up the product search"));
        // Grouped by repository: /src/shop/api belongs to /src/shop.
        let git = |p: &Path| p == Path::new("/src/shop/.git");
        assert_eq!(repository("/src/shop/api", git), "/src/shop");
        assert_eq!(repository("/src/blog", git), "/src/blog");
    }

    #[test]
    fn f_th_17_5_the_library_never_opens_a_credential_file_or_follows_a_link() {
        let (fs, sources) = fixture();
        let scan = scan(&fs, &sources);
        for s in &scan.sessions {
            transcript(&fs, s.engine, &s.file).expect("transcript");
        }
        let opened = fs.opened.lock().expect("lock").clone();
        assert!(!opened.is_empty());
        for path in &opened {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            assert!(
                name != ".credentials.json" && name != "auth.json",
                "opened {path:?}"
            );
            assert!(name.ends_with(".jsonl"), "opened {path:?}");
            assert_ne!(name, "linked.jsonl", "followed a link");
        }
    }

    #[test]
    fn f_th_17_1_the_transcript_keeps_prompts_and_replies_and_names_tools_only() {
        let (fs, sources) = fixture();
        let scan = scan(&fs, &sources);
        let codex = scan
            .sessions
            .iter()
            .find(|s| s.session_id == "dddd-4444")
            .expect("codex");
        let items = transcript(&fs, codex.engine, &codex.file).expect("items");
        assert_eq!(
            items,
            vec![
                TranscriptItem {
                    role: TranscriptRole::User,
                    text: "Speed up the product search".into()
                },
                TranscriptItem {
                    role: TranscriptRole::Tool,
                    text: "shell".into()
                },
                TranscriptItem {
                    role: TranscriptRole::Assistant,
                    text: "Done.".into()
                },
            ]
        );
        let text = as_text(&items);
        assert!(text.contains("(used shell)") && !text.contains("cat secret"));
        assert!(
            !valid_session_id("--help")
                && !valid_session_id("a b")
                && valid_session_id("aaaa-1111")
        );
        assert_eq!(clip(&"word ".repeat(40), 10), "word word …");
    }

    #[test]
    fn f_th_17_4_a_pasted_token_never_reaches_a_title_or_a_transcript() {
        let token = "ghp_0123456789abcdefghijABCDEFGHIJ0123456789";
        let mut fs = FakeFs::default();
        fs.file(
            "/h/projects/-src/eeee-5555.jsonl",
            &claude_line(
                "user",
                "eeee-5555",
                "/src",
                &format!("Deploy with {token} please"),
            ),
        );
        let source = LibrarySource {
            engine: LibraryEngine::Claude,
            account_id: None,
            account_label: None,
            home: PathBuf::from("/h"),
        };
        let scan = scan(&fs, &[source]);
        let title = scan.sessions[0].title.clone().expect("title");
        assert!(
            !title.contains(token) && title.contains("Deploy with"),
            "{title}"
        );
        let items = transcript(&fs, LibraryEngine::Claude, &scan.sessions[0].file).expect("items");
        assert!(!as_text(&items).contains(token));
    }

    #[test]
    fn f_th_17_5_a_long_line_is_skipped_without_being_held_and_the_rest_is_read() {
        let mut reader =
            std::io::Cursor::new([vec![b'x'; MAX_LINE + 10], b"\n{\"a\":1}\n".to_vec()].concat());
        assert_eq!(bounded_line(&mut reader), Some(None));
        assert_eq!(
            bounded_line(&mut reader),
            Some(Some(b"{\"a\":1}\n".to_vec()))
        );
        assert_eq!(bounded_line(&mut reader), None);
    }

    #[cfg(unix)]
    #[test]
    fn f_th_17_5_real_files_only_never_a_link_a_fifo_or_a_linked_folder() {
        let dir = tempfile::tempdir().expect("dir");
        let home = dir.path().join("home");
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(elsewhere.join("-src")).expect("mkdir");
        std::fs::write(
            elsewhere.join("-src/aaaa-1111.jsonl"),
            claude_line("user", "aaaa-1111", "/src", "hi"),
        )
        .expect("write");
        std::fs::create_dir_all(&home).expect("home");
        // projects/ is a link to a folder elsewhere: not read.
        std::os::unix::fs::symlink(&elsewhere, home.join("projects")).expect("link");
        let source = LibrarySource {
            engine: LibraryEngine::Claude,
            account_id: None,
            account_label: None,
            home: home.clone(),
        };
        assert!(scan(&RealFs, std::slice::from_ref(&source))
            .sessions
            .is_empty());
        // A link to a file and a FIFO aren't opened.
        let file = elsewhere.join("-src/aaaa-1111.jsonl");
        let link = dir.path().join("link.jsonl");
        std::os::unix::fs::symlink(&file, &link).expect("link");
        assert!(RealFs.open(&link).is_err());
        let fifo = dir.path().join("fifo.jsonl");
        let c = std::ffi::CString::new(fifo.to_string_lossy().as_bytes()).expect("cstr");
        // SAFETY: a valid C string path; mkfifo has no other preconditions.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        assert!(
            RealFs.open(&fifo).is_err(),
            "a FIFO must not block or be read"
        );
        assert!(RealFs.open(&file).is_ok());
    }
}
