//! The Files sidebar's file system side (F-CD-19, ARCHITECTURE §3.2 "Files,
//! editor, browser"): a lazy tree that honours `.gitignore`, reads that tell
//! text, binary and image files apart, and writes that land on a symlink's
//! target only while the file is still what the editor opened, keeping its
//! line endings, BOM and mode.
//!
//! Whether a path is editable (inside some workspace) is the caller's call:
//! these functions only see one root. The local UI may open any path; every
//! other origin resolves its path with [`crate::contain::contain`] first
//! ([`resolve`]).

pub mod watch;

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::contain::{contain, ContainError, ContainPolicy};
use crate::error::{ErrorCode, IpcError};
use crate::origin::Origin;

/// Files larger than this open read-only with a warning.
pub const LARGE_FILE_BYTES: u64 = 5 * 1024 * 1024;
/// A large text file is shown up to here.
pub const MAX_TEXT_BYTES: u64 = 20 * 1024 * 1024;
/// Images up to this size are previewed.
pub const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
/// A binary file's placeholder shows this many bytes as hex.
pub const HEX_PREVIEW_BYTES: usize = 256;
/// Bytes looked at to tell text from binary.
const SNIFF_BYTES: usize = 8_000;

#[derive(Debug, thiserror::Error)]
pub enum FilesError {
    #[error("That file or folder doesn't exist")]
    NotFound,
    /// The file isn't what the editor opened any more.
    #[error("File changed on disk")]
    Conflict,
    #[error("{0} already exists")]
    Exists(String),
    #[error("{}", .0.message())]
    ReadOnly(ReadOnlyReason),
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Contain(#[from] ContainError),
    #[error("couldn't use that path: {0}")]
    Io(#[from] std::io::Error),
}

impl From<FilesError> for IpcError {
    fn from(err: FilesError) -> Self {
        let code = match &err {
            FilesError::NotFound => ErrorCode::NotFound,
            FilesError::Conflict | FilesError::Exists(_) => ErrorCode::Conflict,
            FilesError::ReadOnly(_) => ErrorCode::PermissionDenied,
            FilesError::Invalid(_) => ErrorCode::Invalid,
            FilesError::Contain(e) => return e.clone().into(),
            FilesError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => ErrorCode::NotFound,
            FilesError::Io(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                ErrorCode::PermissionDenied
            }
            FilesError::Io(_) => ErrorCode::Io,
        };
        IpcError::new(code, err.to_string()).with_stage("files")
    }
}

/// Why a file opened read-only (the editor's banner says so).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ReadOnlyReason {
    /// Outside every workspace (F-CD-09 AC2).
    Outside,
    /// Over [`LARGE_FILE_BYTES`].
    Large,
    /// The file system won't let us write it.
    Permission,
}

impl ReadOnlyReason {
    pub fn message(self) -> &'static str {
        match self {
            ReadOnlyReason::Outside => "This file is outside your workspaces, so it's read-only",
            ReadOnlyReason::Large => "This file is larger than 5 MB, so it's read-only",
            ReadOnlyReason::Permission => "You don't have permission to change this file",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FsKind {
    File,
    Dir,
}

/// One row of a folder listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    /// Relative to the listing's root, `/`-separated.
    pub path: String,
    /// A symlink's target kind (a broken link is a file).
    pub kind: FsKind,
    pub symlink: bool,
    /// Matched by `.gitignore` (or inside an ignored folder).
    pub ignored: bool,
}

/// Folder names never listed or watched: git's own data.
fn is_git_dir(name: &str) -> bool {
    name == ".git"
}

/// `rel` as a relative path inside the root: no absolute paths, no `..`.
fn relative(rel: &str) -> Result<PathBuf, FilesError> {
    let path = Path::new(rel);
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => {
                return Err(FilesError::Invalid(
                    "That folder isn't inside this one".into(),
                ))
            }
        }
    }
    Ok(out)
}

/// `/`-separated, whatever the platform.
pub fn slash_path(path: &Path) -> String {
    let parts: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(p) => Some(p.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    parts.join("/")
}

/// One level of `root/dir` (F-CD-19 "lazy-loaded, respects .gitignore"):
/// folders first, then files, by name; `.git` is never listed. Ignored
/// entries are left out unless `show_ignored`, and then they're flagged;
/// everything inside an ignored folder is ignored too.
pub fn list_dir(root: &Path, dir: &str, show_ignored: bool) -> Result<Vec<DirEntry>, FilesError> {
    let rel = relative(dir)?;
    let abs = root.join(&rel);
    let meta = fs::metadata(&abs).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => FilesError::NotFound,
        _ => FilesError::Io(e),
    })?;
    if !meta.is_dir() {
        return Err(FilesError::Invalid("That isn't a folder".into()));
    }
    let kept = not_ignored(root, &rel);
    let mut out = Vec::new();
    for entry in fs::read_dir(&abs)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            tracing::debug!(
                code = "files.non_utf8_name",
                "skipped a file name that isn't UTF-8"
            );
            continue;
        };
        if is_git_dir(&name) {
            continue;
        }
        let ignored = !kept.contains(&name);
        if ignored && !show_ignored {
            continue;
        }
        let file_type = entry.file_type()?;
        let symlink = file_type.is_symlink();
        let is_dir = if symlink {
            fs::metadata(entry.path())
                .map(|m| m.is_dir())
                .unwrap_or(false)
        } else {
            file_type.is_dir()
        };
        out.push(DirEntry {
            path: slash_path(&rel.join(&name)),
            name,
            kind: if is_dir { FsKind::Dir } else { FsKind::File },
            symlink,
            ignored,
        });
    }
    out.sort_by(|a, b| {
        (a.kind != FsKind::Dir)
            .cmp(&(b.kind != FsKind::Dir))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(out)
}

/// The names in `root/rel` that `.gitignore` keeps. The walk starts at the
/// root and only goes down `rel`'s own path, so every `.gitignore` on the
/// way applies, and a folder inside an ignored one yields nothing.
fn not_ignored(root: &Path, rel: &Path) -> std::collections::HashSet<String> {
    let target = rel.to_path_buf();
    let depth = rel.components().count() + 1;
    let base = root.to_path_buf();
    let walker = walker(root)
        .max_depth(Some(depth))
        .filter_entry(move |e| {
            let Ok(r) = e.path().strip_prefix(&base) else {
                return false;
            };
            target.starts_with(r) || r.parent() == Some(target.as_path())
        })
        .build();
    let mut names = std::collections::HashSet::new();
    for entry in walker.flatten() {
        let Ok(r) = entry.path().strip_prefix(root) else {
            continue;
        };
        if r.parent() == Some(rel) {
            if let Some(name) = r.file_name().and_then(|n| n.to_str()) {
                names.insert(name.to_string());
            }
        }
    }
    names
}

/// The `.gitignore` rules the tree and the watcher share: the repository's
/// ignore files (also outside a git repository), its `info/exclude` and the
/// user's global excludes; hidden files are shown.
pub(crate) fn walker(root: &Path) -> ignore::WalkBuilder {
    let mut b = ignore::WalkBuilder::new(root);
    b.hidden(false)
        .ignore(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .parents(true)
        .require_git(false)
        .follow_links(false);
    b
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FileKind {
    Text,
    Binary,
    Image,
}

/// What [`read_file`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRead {
    pub kind: FileKind,
    /// Text files, without a BOM; cut at [`MAX_TEXT_BYTES`].
    pub text: Option<String>,
    pub size: u64,
    pub truncated: bool,
    /// The content hash `write_file` checks; a large file's is its size
    /// and mtime (it can't be written).
    pub hash: String,
    /// Unix milliseconds.
    pub mtime: i64,
    /// Images up to [`MAX_IMAGE_BYTES`].
    pub data_url: Option<String>,
    /// Binary files: the first bytes as a hex dump.
    pub hex: Option<String>,
    /// Over [`LARGE_FILE_BYTES`].
    pub large: bool,
    /// The file system lets us write it.
    pub writable: bool,
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn mtime_ms(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn image_mime(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        _ => return None,
    })
}

const BOM: &[u8] = b"\xEF\xBB\xBF";

/// Text if it has no NUL byte and is UTF-8 (a character cut off at the end
/// of a partial read doesn't count against it).
fn decode_text(bytes: &[u8], partial: bool) -> Option<String> {
    let sniff = &bytes[..bytes.len().min(SNIFF_BYTES)];
    if sniff.contains(&0) {
        return None;
    }
    let body = bytes.strip_prefix(BOM).unwrap_or(bytes);
    match std::str::from_utf8(body) {
        Ok(text) => Some(text.to_string()),
        Err(e) if partial && e.error_len().is_none() => {
            std::str::from_utf8(&body[..e.valid_up_to()])
                .ok()
                .map(str::to_string)
        }
        Err(_) => None,
    }
}

/// `00000000  89 50 4e 47 0d 0a 1a 0a  00 00 00 0d 49 48 44 52  |.PNG........IHDR|`
pub fn hex_dump(bytes: &[u8]) -> String {
    let mut out = String::new();
    for (row, chunk) in bytes.chunks(16).enumerate() {
        out.push_str(&format!("{:08x} ", row * 16));
        for i in 0..16 {
            if i == 8 {
                out.push(' ');
            }
            match chunk.get(i) {
                Some(b) => out.push_str(&format!(" {b:02x}")),
                None => out.push_str("   "),
            }
        }
        out.push_str("  |");
        for &b in chunk {
            out.push(if (0x20..0x7f).contains(&b) {
                b as char
            } else {
                '.'
            });
        }
        out.push_str("|\n");
    }
    out
}

fn can_write(path: &Path) -> bool {
    // Append mode opens without truncating or changing anything.
    OpenOptions::new().append(true).open(path).is_ok()
}

/// Reads `path` for the editor (F-CD-19): text, a binary placeholder or an
/// image preview, with the hash and mtime a later write checks against.
pub fn read_file(path: &Path) -> Result<FileRead, FilesError> {
    let meta = fs::metadata(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => FilesError::NotFound,
        _ => FilesError::Io(e),
    })?;
    if meta.is_dir() {
        return Err(FilesError::Invalid("That's a folder".into()));
    }
    let size = meta.len();
    let mtime = mtime_ms(&meta);
    let large = size > LARGE_FILE_BYTES;
    let writable = can_write(path);
    let mut file = fs::File::open(path)?;

    if let Some(mime) = image_mime(path) {
        let mut out = FileRead {
            kind: FileKind::Image,
            text: None,
            size,
            truncated: false,
            hash: format!("meta:{size}:{mtime}"),
            mtime,
            data_url: None,
            hex: None,
            large,
            writable,
        };
        if size <= MAX_IMAGE_BYTES {
            let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
            file.read_to_end(&mut bytes)?;
            out.hash = hash_bytes(&bytes);
            out.data_url = Some(format!(
                "data:{mime};base64,{}",
                base64::engine::general_purpose::STANDARD.encode(&bytes)
            ));
        }
        return Ok(out);
    }

    let cap = if large {
        MAX_TEXT_BYTES
    } else {
        LARGE_FILE_BYTES
    };
    let mut bytes = Vec::new();
    (&mut file).take(cap).read_to_end(&mut bytes)?;
    let truncated = (bytes.len() as u64) < size;
    let hash = if large {
        format!("meta:{size}:{mtime}")
    } else {
        hash_bytes(&bytes)
    };
    match decode_text(&bytes, truncated) {
        Some(mut text) => {
            if truncated {
                // Don't end on half a line.
                if let Some(cut) = text.rfind('\n') {
                    text.truncate(cut + 1);
                }
            }
            Ok(FileRead {
                kind: FileKind::Text,
                text: Some(text),
                size,
                truncated,
                hash,
                mtime,
                data_url: None,
                hex: None,
                large,
                writable,
            })
        }
        None => Ok(FileRead {
            kind: FileKind::Binary,
            text: None,
            size,
            truncated: false,
            hash,
            mtime,
            data_url: None,
            hex: Some(hex_dump(&bytes[..bytes.len().min(HEX_PREVIEW_BYTES)])),
            large,
            writable,
        }),
    }
}

/// What a write left on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub hash: String,
    pub mtime: i64,
    /// The file actually written (a symlink's target).
    pub target: PathBuf,
}

/// CRLF when most of the file's line breaks are.
fn uses_crlf(bytes: &[u8]) -> bool {
    let crlf = bytes.windows(2).filter(|w| w == b"\r\n").count();
    let lf = bytes.iter().filter(|&&b| b == b'\n').count();
    crlf > 0 && crlf * 2 >= lf
}

/// Saves `text` to `path` (F-CD-19 "Safety").
///
/// - Through a symlink the link's target is written; the link stays.
/// - `expected` is the hash from when the editor read the file. If the file
///   changed since, nothing is written: [`FilesError::Conflict`]. `None`
///   creates a new file, which mustn't exist yet.
/// - The file keeps its line endings (CRLF stays CRLF), BOM and mode, and
///   is replaced atomically.
pub fn write_file(path: &Path, text: &str, expected: Option<&str>) -> Result<Written, FilesError> {
    let (target, current) = match fs::canonicalize(path) {
        Ok(target) => {
            let meta = fs::metadata(&target)?;
            if meta.is_dir() {
                return Err(FilesError::Invalid("That's a folder".into()));
            }
            if meta.len() > LARGE_FILE_BYTES {
                return Err(FilesError::ReadOnly(ReadOnlyReason::Large));
            }
            let bytes = fs::read(&target)?;
            (target, Some((bytes, meta)))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if fs::symlink_metadata(path).is_ok() {
                // A link to nothing: there's no target to write.
                return Err(FilesError::NotFound);
            }
            let parent = path.parent().ok_or(FilesError::NotFound)?;
            let name = path.file_name().ok_or(FilesError::NotFound)?;
            (fs::canonicalize(parent)?.join(name), None)
        }
        Err(e) => return Err(e.into()),
    };
    match (&current, expected) {
        (None, Some(_)) => return Err(FilesError::Conflict),
        (Some(_), None) => {
            return Err(FilesError::Exists(
                target
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ))
        }
        (Some((bytes, _)), Some(hash)) if hash_bytes(bytes) != hash => {
            return Err(FilesError::Conflict)
        }
        _ => {}
    }

    let (crlf, bom, mode) = match &current {
        Some((bytes, meta)) => (uses_crlf(bytes), bytes.starts_with(BOM), file_mode(meta)),
        None => (false, false, None),
    };
    let mut out = Vec::with_capacity(text.len() + 3);
    if bom {
        out.extend_from_slice(BOM);
    }
    if crlf {
        let normalized = text.replace("\r\n", "\n");
        out.extend_from_slice(normalized.replace('\n', "\r\n").as_bytes());
    } else {
        out.extend_from_slice(text.as_bytes());
    }
    if current.is_some() && !can_write(&target) {
        return Err(FilesError::ReadOnly(ReadOnlyReason::Permission));
    }
    match crate::atomic::atomic_write(&target, &out, mode) {
        Ok(()) => {}
        // A folder we can't create files in, but a file we may change:
        // write it in place.
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && current.is_some() => {
            let mut f = OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&target)?;
            f.write_all(&out)?;
            f.sync_all()?;
        }
        Err(e) => return Err(e.into()),
    }
    let meta = fs::metadata(&target)?;
    Ok(Written {
        hash: hash_bytes(&out),
        mtime: mtime_ms(&meta),
        target,
    })
}

#[cfg(unix)]
fn file_mode(meta: &fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(meta.permissions().mode() & 0o7777)
}

#[cfg(not(unix))]
fn file_mode(_meta: &fs::Metadata) -> Option<u32> {
    None
}

/// New file or folder (the tree's context menu). The parent must exist.
pub fn create(path: &Path, kind: FsKind) -> Result<(), FilesError> {
    if fs::symlink_metadata(path).is_ok() {
        return Err(FilesError::Exists(display_name(path)));
    }
    match kind {
        FsKind::Dir => fs::create_dir(path)?,
        FsKind::File => {
            OpenOptions::new().write(true).create_new(true).open(path)?;
        }
    }
    Ok(())
}

/// Renames or moves within the file system; never over an existing entry.
pub fn rename(from: &Path, to: &Path) -> Result<(), FilesError> {
    if fs::symlink_metadata(from).is_err() {
        return Err(FilesError::NotFound);
    }
    if fs::symlink_metadata(to).is_ok() {
        return Err(FilesError::Exists(display_name(to)));
    }
    fs::rename(from, to)?;
    Ok(())
}

/// Deletes a file, a folder with what's in it, or a symlink itself (never
/// what it points to).
pub fn delete(path: &Path) -> Result<(), FilesError> {
    let meta = fs::symlink_metadata(path).map_err(|_| FilesError::NotFound)?;
    if meta.file_type().is_symlink() {
        #[cfg(windows)]
        if fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false) {
            fs::remove_dir(path)?;
            return Ok(());
        }
        fs::remove_file(path)?;
    } else if meta.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// The absolute path a request names (F-TL-03). The local UI may name any
/// path, relative to `root` or absolute (it's read-only outside every
/// workspace); every other origin is held to `root` by
/// [`crate::contain::contain`] with `policy`.
pub fn resolve(
    root: &Path,
    requested: &str,
    origin: &Origin,
    policy: ContainPolicy,
) -> Result<PathBuf, FilesError> {
    if requested.is_empty() {
        return Err(FilesError::Invalid("No path".into()));
    }
    let path = Path::new(requested);
    match origin {
        Origin::Local { .. } => Ok(if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.join(path)
        }),
        _ => Ok(contain(root, path, policy)?),
    }
}

/// Inside one of `roots` (all canonical), after resolving symlinks.
pub fn inside_any(path: &Path, roots: &[PathBuf]) -> bool {
    let canon = fs::canonicalize(path).or_else(|_| {
        // A new file: its folder decides.
        let parent = path.parent().ok_or(std::io::ErrorKind::NotFound)?;
        let name = path.file_name().ok_or(std::io::ErrorKind::NotFound)?;
        fs::canonicalize(parent).map(|p| p.join(name))
    });
    match canon {
        Ok(canon) => roots.iter().any(|r| canon.starts_with(r)),
        Err(_) => false,
    }
}

/// `path` relative to `root` when it's inside it (for tab and tree keys),
/// else the absolute path, both `/`-separated.
pub fn display_path(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(rel) => slash_path(rel),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tmp");
        let root = fs::canonicalize(dir.path()).expect("canon").join("ws");
        fs::create_dir_all(root.join("src/deep")).expect("mkdir");
        fs::create_dir_all(root.join("node_modules/react")).expect("mkdir");
        fs::create_dir_all(root.join(".git")).expect("mkdir");
        fs::write(root.join(".gitignore"), "node_modules/\n*.log\n").expect("write");
        fs::write(root.join("README.md"), "# hi\n").expect("write");
        fs::write(root.join("debug.log"), "noise\n").expect("write");
        fs::write(root.join(".env"), "KEY=1\n").expect("write");
        fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("write");
        fs::write(root.join("node_modules/react/index.js"), "x\n").expect("write");
        (dir, root)
    }

    fn names(entries: &[DirEntry]) -> Vec<(&str, bool)> {
        entries
            .iter()
            .map(|e| (e.name.as_str(), e.ignored))
            .collect()
    }

    #[test]
    fn f_cd_19_3_node_modules_is_hidden_when_gitignored_and_shown_with_show_ignored() {
        let (_d, root) = tree();
        let hidden = list_dir(&root, "", false).expect("list");
        assert_eq!(
            names(&hidden),
            vec![
                ("src", false),
                (".env", false),
                (".gitignore", false),
                ("README.md", false)
            ]
        );
        let shown = list_dir(&root, "", true).expect("list");
        assert_eq!(
            names(&shown),
            vec![
                ("node_modules", true),
                ("src", false),
                (".env", false),
                (".gitignore", false),
                ("debug.log", true),
                ("README.md", false)
            ]
        );
        assert_eq!(shown[0].kind, FsKind::Dir);
        assert_eq!(shown[0].path, "node_modules");
    }

    #[test]
    fn f_cd_19_everything_inside_an_ignored_folder_is_ignored() {
        let (_d, root) = tree();
        let inside = list_dir(&root, "node_modules", true).expect("list");
        assert_eq!(names(&inside), vec![("react", true)]);
        assert_eq!(inside[0].path, "node_modules/react");
        assert!(list_dir(&root, "node_modules", false)
            .expect("list")
            .is_empty());
        let deeper = list_dir(&root, "node_modules/react", true).expect("list");
        assert_eq!(names(&deeper), vec![("index.js", true)]);
    }

    #[test]
    fn f_cd_19_nested_gitignores_apply_to_their_folder() {
        let (_d, root) = tree();
        fs::write(root.join("src/.gitignore"), "generated.rs\n").expect("write");
        fs::write(root.join("src/generated.rs"), "//\n").expect("write");
        let src = list_dir(&root, "src", false).expect("list");
        assert_eq!(
            names(&src),
            vec![("deep", false), (".gitignore", false), ("main.rs", false)]
        );
        let all = list_dir(&root, "src", true).expect("list");
        assert!(all.iter().any(|e| e.name == "generated.rs" && e.ignored));
    }

    #[test]
    fn f_cd_19_git_data_is_never_listed_and_paths_stay_inside() {
        let (_d, root) = tree();
        let all = list_dir(&root, "", true).expect("list");
        assert!(all.iter().all(|e| e.name != ".git"));
        assert!(matches!(
            list_dir(&root, "../", false),
            Err(FilesError::Invalid(_))
        ));
        assert!(matches!(
            list_dir(&root, "missing", false),
            Err(FilesError::NotFound)
        ));
        assert!(matches!(
            list_dir(&root, "README.md", false),
            Err(FilesError::Invalid(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn f_cd_19_symlinks_are_listed_as_what_they_point_to() {
        let (_d, root) = tree();
        std::os::unix::fs::symlink(root.join("src"), root.join("src-link")).expect("symlink");
        std::os::unix::fs::symlink(root.join("gone"), root.join("dangling")).expect("symlink");
        let all = list_dir(&root, "", false).expect("list");
        let link = all.iter().find(|e| e.name == "src-link").expect("link");
        assert_eq!((link.kind, link.symlink), (FsKind::Dir, true));
        let dangling = all.iter().find(|e| e.name == "dangling").expect("link");
        assert_eq!((dangling.kind, dangling.symlink), (FsKind::File, true));
    }

    #[test]
    fn f_cd_19_1_a_file_changed_on_disk_since_it_was_opened_is_a_conflict_not_an_overwrite() {
        let (_d, root) = tree();
        let file = root.join("src/main.rs");
        let opened = read_file(&file).expect("read");
        assert_eq!(opened.text.as_deref(), Some("fn main() {}\n"));
        // An agent writes the file meanwhile.
        fs::write(&file, "fn main() { agent(); }\n").expect("agent write");
        let err = write_file(&file, "fn main() { mine(); }\n", Some(&opened.hash))
            .expect_err("stale hash");
        assert!(matches!(err, FilesError::Conflict));
        assert_eq!(IpcError::from(err).code, ErrorCode::Conflict);
        assert_eq!(
            fs::read_to_string(&file).expect("read"),
            "fn main() { agent(); }\n",
            "the agent's version stays"
        );
        // Overwrite: the dialog reads the current hash and writes with it.
        let now = read_file(&file).expect("read");
        let written = write_file(&file, "fn main() { mine(); }\n", Some(&now.hash)).expect("write");
        assert_eq!(
            fs::read_to_string(&file).expect("read"),
            "fn main() { mine(); }\n"
        );
        assert_eq!(written.hash, read_file(&file).expect("read").hash);
    }

    #[test]
    fn f_cd_19_a_deleted_file_is_a_conflict_and_new_files_must_not_exist() {
        let (_d, root) = tree();
        let file = root.join("README.md");
        let opened = read_file(&file).expect("read");
        fs::remove_file(&file).expect("rm");
        assert!(matches!(
            write_file(&file, "x", Some(&opened.hash)),
            Err(FilesError::Conflict)
        ));
        assert!(!file.exists(), "nothing was recreated");
        write_file(&root.join("new.txt"), "fresh\n", None).expect("create");
        assert_eq!(
            fs::read_to_string(root.join("new.txt")).expect("read"),
            "fresh\n"
        );
        assert!(matches!(
            write_file(&root.join("new.txt"), "again", None),
            Err(FilesError::Exists(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn f_cd_19_saving_through_a_symlink_writes_the_link_target() {
        let (_d, root) = tree();
        fs::create_dir_all(root.join("shared")).expect("mkdir");
        fs::write(root.join("shared/config.json"), "{}\n").expect("write");
        std::os::unix::fs::symlink("shared/config.json", root.join("config.json"))
            .expect("symlink");
        let link = root.join("config.json");
        let opened = read_file(&link).expect("read");
        let written =
            write_file(&link, "{\"a\": 1}\n", Some(&opened.hash)).expect("write through link");
        assert_eq!(
            fs::read_to_string(root.join("shared/config.json")).expect("read"),
            "{\"a\": 1}\n"
        );
        assert!(
            fs::symlink_metadata(&link)
                .expect("meta")
                .file_type()
                .is_symlink(),
            "the link is still a link"
        );
        assert_eq!(
            written.target,
            fs::canonicalize(root.join("shared/config.json")).expect("canon")
        );
        // A link to nothing has no target to write.
        std::os::unix::fs::symlink("nowhere.txt", root.join("dangling")).expect("symlink");
        assert!(matches!(
            write_file(&root.join("dangling"), "x", None),
            Err(FilesError::NotFound)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn f_cd_19_writes_keep_line_endings_bom_and_mode() {
        use std::os::unix::fs::PermissionsExt;
        let (_d, root) = tree();
        let file = root.join("win.txt");
        fs::write(&file, b"\xEF\xBB\xBFone\r\ntwo\r\n").expect("write");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o750)).expect("chmod");
        let opened = read_file(&file).expect("read");
        assert_eq!(opened.text.as_deref(), Some("one\r\ntwo\r\n"));
        // The editor hands back LF text.
        write_file(&file, "one\ntwo\nthree\n", Some(&opened.hash)).expect("write");
        assert_eq!(
            fs::read(&file).expect("read"),
            b"\xEF\xBB\xBFone\r\ntwo\r\nthree\r\n"
        );
        let mode = fs::metadata(&file).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o750);
        // LF files stay LF.
        let lf = root.join("src/main.rs");
        let opened = read_file(&lf).expect("read");
        write_file(&lf, "a\nb\n", Some(&opened.hash)).expect("write");
        assert_eq!(fs::read(&lf).expect("read"), b"a\nb\n");
    }

    #[test]
    fn f_cd_19_large_files_open_read_only_and_are_cut_on_a_line() {
        let (_d, root) = tree();
        let file = root.join("big.txt");
        let line = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde\n";
        let mut body = String::new();
        while (body.len() as u64) <= LARGE_FILE_BYTES {
            body.push_str(line);
        }
        fs::write(&file, &body).expect("write");
        let read = read_file(&file).expect("read");
        assert!(read.large);
        assert_eq!(read.kind, FileKind::Text);
        assert!(!read.truncated, "under the 20 MB display cap");
        assert!(matches!(
            write_file(&file, "x", Some(&read.hash)),
            Err(FilesError::ReadOnly(ReadOnlyReason::Large))
        ));
        let mut huge = body.repeat(4);
        huge.push_str("tail without a newline");
        fs::write(&file, &huge).expect("write");
        let read = read_file(&file).expect("read");
        assert!(read.truncated);
        let text = read.text.expect("text");
        assert!(text.len() as u64 <= MAX_TEXT_BYTES);
        assert!(text.ends_with('\n'), "cut at the end of a line");
    }

    #[test]
    fn f_cd_19_binary_files_get_a_hex_placeholder_and_images_a_preview() {
        let (_d, root) = tree();
        let bin = root.join("blob.bin");
        let mut bytes = vec![0x7f, b'E', b'L', b'F', 0, 1, 2];
        bytes.extend(std::iter::repeat_n(0xAB, 600));
        fs::write(&bin, &bytes).expect("write");
        let read = read_file(&bin).expect("read");
        assert_eq!(read.kind, FileKind::Binary);
        assert_eq!(read.text, None);
        assert_eq!(read.size, 607);
        let hex = read.hex.expect("hex");
        assert!(
            hex.starts_with("00000000  7f 45 4c 46 00 01 02 ab"),
            "{hex}"
        );
        assert!(hex.contains("|.ELF"), "{hex}");
        assert_eq!(hex.lines().count(), HEX_PREVIEW_BYTES / 16);

        // Latin-1 text isn't UTF-8: it's shown as binary, never mangled.
        fs::write(root.join("latin1.txt"), b"caf\xe9\n").expect("write");
        assert_eq!(
            read_file(&root.join("latin1.txt")).expect("read").kind,
            FileKind::Binary
        );

        let png = root.join("dot.png");
        fs::write(&png, b"\x89PNG\r\n\x1a\nfake").expect("write");
        let read = read_file(&png).expect("read");
        assert_eq!(read.kind, FileKind::Image);
        let url = read.data_url.expect("preview");
        // base64 of the bytes written above.
        assert_eq!(url, "data:image/png;base64,iVBORw0KGgpmYWtl");
    }

    #[test]
    fn f_cd_19_create_rename_and_delete_never_clobber() {
        let (_d, root) = tree();
        create(&root.join("notes"), FsKind::Dir).expect("mkdir");
        create(&root.join("notes/a.md"), FsKind::File).expect("touch");
        assert!(matches!(
            create(&root.join("notes/a.md"), FsKind::File),
            Err(FilesError::Exists(_))
        ));
        fs::write(root.join("notes/b.md"), "b").expect("write");
        assert!(matches!(
            rename(&root.join("notes/a.md"), &root.join("notes/b.md")),
            Err(FilesError::Exists(_))
        ));
        rename(&root.join("notes/a.md"), &root.join("notes/c.md")).expect("rename");
        assert!(root.join("notes/c.md").exists());
        delete(&root.join("notes")).expect("rm -r");
        assert!(!root.join("notes").exists());
        assert!(matches!(
            delete(&root.join("notes")),
            Err(FilesError::NotFound)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn f_cd_19_deleting_a_symlink_removes_the_link_not_its_target() {
        let (_d, root) = tree();
        std::os::unix::fs::symlink(root.join("src"), root.join("src-link")).expect("symlink");
        delete(&root.join("src-link")).expect("rm link");
        assert!(root.join("src/main.rs").exists());
        assert!(fs::symlink_metadata(root.join("src-link")).is_err());
    }

    #[test]
    fn f_tl_03_only_the_local_ui_may_name_paths_outside_the_folder() {
        let (_d, root) = tree();
        let ui = Origin::main_window();
        assert_eq!(
            resolve(&root, "src/main.rs", &ui, ContainPolicy::Read).expect("ui"),
            root.join("src/main.rs")
        );
        let outside = if cfg!(windows) {
            "C:\\Windows\\win.ini"
        } else {
            "/etc/hosts"
        };
        assert_eq!(
            resolve(&root, outside, &ui, ContainPolicy::Read).expect("ui"),
            PathBuf::from(outside)
        );
        for origin in [Origin::Conductor, Origin::Phone, Origin::Unattended] {
            assert!(matches!(
                resolve(&root, outside, &origin, ContainPolicy::Read),
                Err(FilesError::Contain(ContainError::Escape))
            ));
            assert!(matches!(
                resolve(&root, "../escape", &origin, ContainPolicy::Read),
                Err(FilesError::Contain(_))
            ));
            assert!(matches!(
                resolve(&root, ".env", &origin, ContainPolicy::Read),
                Err(FilesError::Contain(ContainError::Private))
            ));
            assert_eq!(
                resolve(&root, "src/main.rs", &origin, ContainPolicy::Write).expect("inside"),
                root.join("src/main.rs")
            );
        }
        assert_eq!(
            IpcError::from(FilesError::Contain(ContainError::Escape)).code,
            ErrorCode::PathEscape
        );
    }

    #[test]
    fn f_cd_09_2_a_path_outside_every_workspace_is_not_inside_any() {
        let (d, root) = tree();
        let roots = vec![root.clone()];
        assert!(inside_any(&root.join("src/main.rs"), &roots));
        assert!(
            inside_any(&root.join("src/new-file.rs"), &roots),
            "a new file's folder decides"
        );
        let elsewhere = fs::canonicalize(d.path())
            .expect("canon")
            .join("elsewhere.txt");
        fs::write(&elsewhere, "x").expect("write");
        assert!(!inside_any(&elsewhere, &roots));
        #[cfg(unix)]
        {
            // A link inside a workspace to a file outside it is outside.
            std::os::unix::fs::symlink(&elsewhere, root.join("out-link")).expect("symlink");
            assert!(!inside_any(&root.join("out-link"), &roots));
            assert!(!inside_any(Path::new("/etc/hosts"), &roots));
        }
    }

    #[test]
    fn display_paths_are_relative_inside_the_root() {
        let root = Path::new("/w/repo");
        assert_eq!(
            display_path(root, Path::new("/w/repo/src/a.rs")),
            "src/a.rs"
        );
        assert_eq!(display_path(root, Path::new("/etc/hosts")), "/etc/hosts");
        assert_eq!(slash_path(Path::new("a/./b")), "a/b");
    }
}
