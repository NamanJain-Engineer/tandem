//! Thread attachments and `@` mentions (F-TH-07): files and folders the user
//! attaches, pasted screenshots saved as PNGs in `$DATA/attachments/<thread>/`,
//! and the `.gitignore`-aware path picker.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tandem_core::ids::{AttachmentId, ThreadId};
use tandem_core::{ErrorCode, IpcError};
use tandem_store::repo::attachments::{Attachment, AttachmentKind, MAX_FILE_BYTES};
use ts_rs::TS;

/// Per message (F-TH-07): 20 attachments and 20 MB in all.
pub const MAX_ATTACHMENTS: usize = 20;
pub const MAX_BYTES: u64 = MAX_FILE_BYTES;
/// The picker answers with at most this many rows.
pub const MAX_MENTIONS: usize = 50;
/// Files walked per search, so a huge tree answers quickly.
const WALK_LIMIT: usize = 20_000;

/// One `@` picker row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PathEntry {
    /// Relative to the thread's folder, `/`-separated; folders end in `/`.
    pub path: String,
    pub is_dir: bool,
}

fn too_large() -> String {
    "Too large (max 20 MB)".into()
}

fn image_mime(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => Some("image/png"),
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        Some("gif") => Some("image/gif"),
        Some("webp") => Some("image/webp"),
        _ => None,
    }
}

/// An attachment for a dropped or picked path. Files over the limit come
/// back as error chips rather than failing the whole drop.
pub fn for_path(thread: &ThreadId, path: &Path) -> Result<Attachment, IpcError> {
    let meta = std::fs::metadata(path).map_err(|e| {
        IpcError::new(
            ErrorCode::NotFound,
            format!("Couldn't read {}: {e}", path.display()),
        )
    })?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let (kind, mime) = if meta.is_dir() {
        (AttachmentKind::Folder, None)
    } else if let Some(m) = image_mime(path) {
        (AttachmentKind::Image, Some(m.to_string()))
    } else {
        (AttachmentKind::File, None)
    };
    let bytes = if meta.is_dir() { 0 } else { meta.len() };
    Ok(Attachment {
        id: AttachmentId::new(),
        thread_id: thread.clone(),
        kind,
        path: path.to_string_lossy().into_owned(),
        name,
        mime,
        bytes,
        sent: false,
        error: (bytes > MAX_BYTES).then(too_large),
    })
}

/// Checks one message's attachments against the limits.
pub fn check_limits(list: &[Attachment]) -> Result<(), IpcError> {
    if list.len() > MAX_ATTACHMENTS {
        return Err(IpcError::new(
            ErrorCode::LimitReached,
            "A message can carry up to 20 attachments",
        ));
    }
    let total: u64 = list.iter().map(|a| a.bytes).sum();
    if total > MAX_BYTES || list.iter().any(|a| a.error.is_some()) {
        return Err(IpcError::new(
            ErrorCode::LimitReached,
            "Attachments are limited to 20 MB per message",
        ));
    }
    Ok(())
}

/// Saves RGBA pixels as `$DATA/attachments/<thread>/<id>.png` (mode 0600).
pub fn save_png(
    dir: &Path,
    thread: &ThreadId,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<Attachment, IpcError> {
    let io =
        |e: std::io::Error| IpcError::new(ErrorCode::Io, format!("Couldn't save the image: {e}"));
    if width == 0 || height == 0 || rgba.len() != (width as usize) * (height as usize) * 4 {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "The clipboard image is empty",
        ));
    }
    std::fs::create_dir_all(dir).map_err(io)?;
    let id = AttachmentId::new();
    let path = dir.join(format!("{}.png", id.as_str()));
    let mut bytes = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut bytes, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc
            .write_header()
            .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
        w.write_image_data(rgba)
            .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
    }
    tandem_core::atomic::atomic_write(&path, &bytes, Some(0o600)).map_err(io)?;
    let len = bytes.len() as u64;
    Ok(Attachment {
        id: id.clone(),
        thread_id: thread.clone(),
        kind: AttachmentKind::Image,
        path: path.to_string_lossy().into_owned(),
        name: "Pasted image.png".into(),
        mime: Some("image/png".into()),
        bytes: len,
        sent: false,
        error: (len > MAX_BYTES).then(too_large),
    })
}

/// Subsequence match: every query character in order, case-insensitive.
/// Lower is better: earlier and tighter matches, then shorter paths.
fn score(path: &str, query: &str) -> Option<(usize, usize, usize)> {
    if query.is_empty() {
        return Some((0, 0, path.len()));
    }
    let hay: Vec<char> = path.to_lowercase().chars().collect();
    let mut first = None;
    let mut last = 0;
    let mut at = 0;
    for q in query.to_lowercase().chars() {
        let found = hay[at..].iter().position(|c| *c == q)? + at;
        first.get_or_insert(found);
        last = found;
        at = found + 1;
    }
    let first = first.unwrap_or(0);
    Some((last - first, first, path.len()))
}

/// `@` picker rows under `root` for `query` (F-TH-07 AC6): `.gitignore`d
/// and hidden entries are skipped; top-level folders come first when the
/// query is empty.
pub fn mention_search(root: &Path, query: &str) -> Vec<PathEntry> {
    let query = query.trim().trim_start_matches("./");
    // "src/" lists what's inside src; "src/au" matches inside it too.
    let (base, rest) = match query.rfind('/') {
        Some(i) => (&query[..=i], &query[i + 1..]),
        None => ("", query),
    };
    // Only plain names below the folder: no `..`, no absolute paths, and no
    // symlink out of it.
    let plain = Path::new(base)
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)));
    let start: PathBuf = root.join(base);
    let inside = match (start.canonicalize(), root.canonicalize()) {
        (Ok(s), Ok(r)) => s.starts_with(r),
        _ => false,
    };
    if !plain || !inside || !start.is_dir() {
        return Vec::new();
    }
    let walker = ignore::WalkBuilder::new(&start)
        .hidden(true)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .require_git(false)
        .parents(true)
        .max_depth(Some(if rest.is_empty() { 1 } else { 12 }))
        .build();
    let mut found: Vec<((usize, usize, usize), PathEntry)> = Vec::new();
    for entry in walker.flatten().take(WALK_LIMIT) {
        let path = entry.path();
        if path == start {
            continue;
        }
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
        let mut text = rel.to_string_lossy().replace('\\', "/");
        if is_dir {
            text.push('/');
        }
        let within = &text[base.len().min(text.len())..];
        if let Some(s) = score(within, rest) {
            // Folders first in a listing; fuzzy results by score.
            let key = if rest.is_empty() {
                (usize::from(!is_dir), 0, text.len())
            } else {
                s
            };
            found.push((key, PathEntry { path: text, is_dir }));
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.path.cmp(&b.1.path)));
    found
        .into_iter()
        .take(MAX_MENTIONS)
        .map(|(_, e)| e)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        for f in [
            "src/auth.ts",
            "src/checkout/redirect.ts",
            "src/generated/big.ts",
            "test/checkout.test.ts",
            "README.md",
            ".env",
        ] {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
            std::fs::write(&p, "x").expect("write");
        }
        std::fs::write(root.join(".gitignore"), "src/generated/\n").expect("ignore");
        dir
    }

    #[test]
    fn f_th_07_6_at_src_lists_files_under_src_without_ignored_ones() {
        let dir = tree();
        let rows = mention_search(dir.path(), "src/");
        let paths: Vec<_> = rows.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["src/checkout/", "src/auth.ts"]);
        let fuzzy = mention_search(dir.path(), "src/redir");
        assert_eq!(fuzzy[0].path, "src/checkout/redirect.ts");
        assert!(mention_search(dir.path(), "big").is_empty());
        // Hidden files and escapes never list.
        assert!(mention_search(dir.path(), ".env").is_empty());
        assert!(mention_search(dir.path(), "../").is_empty());
        assert!(mention_search(dir.path(), "src/../../").is_empty());
        assert!(mention_search(dir.path(), "/etc/").is_empty());
    }

    #[test]
    fn f_th_07_empty_query_lists_top_level_folders_first() {
        let dir = tree();
        let rows = mention_search(dir.path(), "");
        let paths: Vec<_> = rows.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["src/", "test/", "README.md"]);
    }

    #[test]
    fn f_th_07_5_a_pasted_image_is_a_0600_png_in_the_threads_folder() {
        let dir = tempfile::tempdir().expect("tmp");
        let thread = ThreadId::new();
        let att =
            save_png(dir.path(), &thread, 2, 1, &[255, 0, 0, 255, 0, 255, 0, 255]).expect("saved");
        assert_eq!(att.kind, AttachmentKind::Image);
        let bytes = std::fs::read(&att.path).expect("file exists");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&att.path)
                .expect("meta")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(save_png(dir.path(), &thread, 2, 2, &[0; 4]).is_err());
    }

    #[test]
    fn f_th_07_limits_20_attachments_and_20_mb() {
        let dir = tempfile::tempdir().expect("tmp");
        let thread = ThreadId::new();
        let small = dir.path().join("a.txt");
        std::fs::write(&small, "hi").expect("write");
        let one = for_path(&thread, &small).expect("att");
        assert_eq!(one.kind, AttachmentKind::File);
        assert!(check_limits(&vec![one.clone(); 20]).is_ok());
        assert_eq!(
            check_limits(&vec![one.clone(); 21]).map_err(|e| e.code),
            Err(ErrorCode::LimitReached)
        );
        let big = Attachment {
            bytes: MAX_BYTES + 1,
            error: Some(too_large()),
            ..one
        };
        assert!(check_limits(&[big]).is_err());
        let folder = for_path(&thread, dir.path()).expect("folder");
        assert_eq!(folder.kind, AttachmentKind::Folder);
    }
}
