//! Memory files (F-AG-07, ARCHITECTURE §8.3): `<home>/memory/NOTES.md`
//! (capped by the teammate's setting) and `ABOUT_YOU.md` (2k). Writes are
//! checked first: over the cap is refused with a request to condense, and a
//! credential-shaped line is refused outright. A write keeps a copy of what
//! it replaced under `memory/.history/<change>.md` for Undo.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tandem_core::teammates::ABOUT_CAP;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MemoryFile {
    Notes,
    About,
}

impl MemoryFile {
    pub fn file_name(self) -> &'static str {
        match self {
            MemoryFile::Notes => "NOTES.md",
            MemoryFile::About => "ABOUT_YOU.md",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            MemoryFile::Notes => "notes",
            MemoryFile::About => "about",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "notes" => Some(MemoryFile::Notes),
            "about" => Some(MemoryFile::About),
            _ => None,
        }
    }
}

/// `memory_get`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Memory {
    pub notes: String,
    pub about: String,
    /// NOTES.md's budget in characters.
    pub cap: u32,
    /// NOTES.md's length in characters.
    pub used: u32,
    pub about_cap: u32,
    pub about_used: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MemoryError {
    #[error("Memory is over its {cap} character budget. Condense it and try again.")]
    OverCap { cap: u32 },
    #[error("Memory can't store credentials")]
    Credentials,
    #[error("{0}")]
    Io(String),
}

pub fn path(home: &Path, file: MemoryFile) -> PathBuf {
    home.join("memory").join(file.file_name())
}

fn read_file(home: &Path, file: MemoryFile) -> String {
    std::fs::read_to_string(path(home, file)).unwrap_or_default()
}

pub fn chars(s: &str) -> u32 {
    s.chars().count() as u32
}

pub fn read(home: &Path, notes_cap: u32) -> Memory {
    let notes = read_file(home, MemoryFile::Notes);
    let about = read_file(home, MemoryFile::About);
    Memory {
        used: chars(&notes),
        about_used: chars(&about),
        notes,
        about,
        cap: notes_cap,
        about_cap: ABOUT_CAP,
    }
}

/// The cap for a file.
pub fn cap_for(file: MemoryFile, notes_cap: u32) -> u32 {
    match file {
        MemoryFile::Notes => notes_cap,
        MemoryFile::About => ABOUT_CAP,
    }
}

/// Whether a text holds something credential-shaped (the log redactor's
/// rules: vendor tokens, key=value secrets, private keys, …), a line at a
/// time, as written or as a briefing reads it, past what's removable
/// (D-3031, D-3036).
pub fn has_credentials(text: &str) -> bool {
    text.lines().any(tandem_core::untrusted::holds_credential)
}

/// The pre-write check (`tandem__memory_write`, `memory_write`).
pub fn check(markdown: &str, cap: u32) -> Result<(), MemoryError> {
    if has_credentials(markdown) {
        return Err(MemoryError::Credentials);
    }
    if chars(markdown) > cap {
        return Err(MemoryError::OverCap { cap });
    }
    Ok(())
}

/// What a write replaced, for Undo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// The copy of the previous content.
    pub backup: PathBuf,
    pub before_hash: String,
    pub after_hash: String,
}

fn hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(text.as_bytes()))
}

/// Checks against the file's own cap (`notes_cap` for NOTES.md, 2k for
/// ABOUT_YOU.md), keeps the old content under `.history/<change>.md`, then
/// replaces the file atomically. A refused write leaves the file as it was.
pub fn write(
    home: &Path,
    file: MemoryFile,
    markdown: &str,
    notes_cap: u32,
    change_id: &str,
) -> Result<Written, MemoryError> {
    check(markdown, cap_for(file, notes_cap))?;
    let io = |e: std::io::Error| MemoryError::Io(e.to_string());
    let before = read_file(home, file);
    let history = home.join("memory").join(".history");
    std::fs::create_dir_all(&history).map_err(io)?;
    let backup = history.join(format!("{change_id}.md"));
    tandem_core::atomic::atomic_write(&backup, before.as_bytes(), Some(0o600)).map_err(io)?;
    tandem_core::atomic::atomic_write(&path(home, file), markdown.as_bytes(), Some(0o600))
        .map_err(io)?;
    Ok(Written {
        backup,
        before_hash: hash(&before),
        after_hash: hash(markdown),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("dir");
        std::fs::create_dir_all(dir.path().join("memory")).expect("memory");
        dir
    }

    #[test]
    fn f_ag_07_1_a_write_over_the_cap_is_refused_and_the_file_is_unchanged() {
        let h = home();
        write(h.path(), MemoryFile::Notes, "- short\n", 20, "c1").expect("fits");
        let err = write(h.path(), MemoryFile::Notes, &"x".repeat(21), 20, "c2").expect_err("over");
        assert_eq!(
            err.to_string(),
            "Memory is over its 20 character budget. Condense it and try again."
        );
        assert_eq!(read(h.path(), 20).notes, "- short\n");
        // About has its own 2k cap.
        assert!(write(h.path(), MemoryFile::About, &"y".repeat(2049), 16_384, "c3").is_err());
    }

    #[test]
    fn f_ag_07_2_a_credential_is_refused() {
        let h = home();
        let token = format!("- token=ghp_{}", "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8");
        let err = write(h.path(), MemoryFile::Notes, &token, 4096, "c1").expect_err("secret");
        assert_eq!(err.to_string(), "Memory can't store credentials");
        assert_eq!(read(h.path(), 4096).notes, "");
        assert!(!has_credentials(
            "- Release branch is cut on Thursdays.\n- Prefers pnpm."
        ));
    }

    #[test]
    fn f_tk_08_1_a_credential_what_is_removable_splits_or_sets_apart_is_refused() {
        // A briefing reads memory past what's removable (D-3031), so a
        // write is checked that way too, and as written: NOTES.md never
        // keeps one (D-3036).
        let h = home();
        for note in [
            "- pushed with ghp_0123456789\u{200b}abcdefghijABCDEFGHIJ012345\n",
            "- pushed with ghp_0123456789\u{2028}abcdefghijABCDEFGHIJ012345\n",
            "- pushed with ghp_0123456789\u{fff9}abcdefghijABCDEFGHIJ012345\n",
            "- pushed with abc\u{200b}ghp_0123456789abcdefghijABCDEFGHIJ012345\n",
        ] {
            assert!(has_credentials(note), "{note:?}");
            let err = write(h.path(), MemoryFile::Notes, note, 4096, "c1").expect_err("secret");
            assert_eq!(err.to_string(), "Memory can't store credentials");
        }
        assert_eq!(read(h.path(), 4096).notes, "");
    }

    #[test]
    fn f_ag_07_a_write_keeps_what_it_replaced_for_undo() {
        let h = home();
        write(h.path(), MemoryFile::Notes, "- one\n", 4096, "c1").expect("one");
        let w = write(h.path(), MemoryFile::Notes, "- two\n", 4096, "c2").expect("two");
        assert_eq!(
            std::fs::read_to_string(&w.backup).expect("backup"),
            "- one\n"
        );
        assert_ne!(w.before_hash, w.after_hash);
        let m = read(h.path(), 4096);
        assert_eq!((m.notes.as_str(), m.used, m.cap), ("- two\n", 6, 4096));
    }
}
