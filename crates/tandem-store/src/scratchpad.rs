//! The scratchpad's notes (F-SH-12): Markdown files in `$DATA/scratchpad`,
//! one per note, named by its id. Every save replaces the file atomically,
//! so a crash leaves the last saved version whole.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use tandem_core::atomic::atomic_write;
use tandem_core::ids::{has_prefix, new_id};
use tandem_core::{ErrorCode, IpcError};
use thiserror::Error;
use ts_rs::TS;

/// A note's size limit: far more than notes need, small enough to save on
/// every pause in typing.
pub const MAX_NOTE_BYTES: usize = 1024 * 1024;
const PREFIX: &str = "note";
/// Titles are shown on one line.
const TITLE_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScratchNote {
    pub id: String,
    /// The first line with words, without its Markdown marks ("" for an
    /// empty note).
    pub title: String,
    /// When it was last saved, in ms since the epoch.
    pub updated_at: i64,
    pub bytes: u64,
}

#[derive(Debug, Error)]
pub enum ScratchpadError {
    #[error("That note doesn't exist")]
    NotFound,
    #[error("Notes are limited to 1 MB")]
    TooLarge,
    #[error("Couldn't save the note: {0}")]
    Io(#[from] io::Error),
}

impl From<ScratchpadError> for IpcError {
    fn from(err: ScratchpadError) -> Self {
        let code = match err {
            ScratchpadError::NotFound => ErrorCode::NotFound,
            ScratchpadError::TooLarge => ErrorCode::Invalid,
            ScratchpadError::Io(_) => ErrorCode::Internal,
        };
        IpcError::new(code, err.to_string()).with_stage("scratchpad")
    }
}

/// The notes in `dir` (`$DATA/scratchpad`).
pub struct ScratchpadStore {
    dir: PathBuf,
}

/// Strips one leading mark (heading, quote, list, task box) from `line`.
fn strip_lead(line: &str) -> Option<&str> {
    let line = line.trim_start();
    for mark in [
        "- [ ] ", "- [x] ", "- [X] ", "* [ ] ", "* [x] ", "* [X] ", "[ ] ", "[x] ", "[X] ", "- ",
        "* ", "+ ", "> ",
    ] {
        if let Some(rest) = line.strip_prefix(mark) {
            return Some(rest);
        }
    }
    if line.starts_with('#') {
        let rest = line.trim_start_matches('#');
        if rest.is_empty() || rest.starts_with(' ') {
            return Some(rest);
        }
    }
    // `1. ` and `1) `
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 {
        let rest = &line[digits..];
        if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some(rest);
        }
    }
    None
}

/// A note's title: its first line with words, without heading, quote, list
/// or task marks and without emphasis (a lone `_` stays: it's more often
/// part of a name than a mark), cut at 80 characters.
pub fn title_of(markdown: &str) -> String {
    for raw in markdown.lines() {
        let mut line = raw;
        while let Some(rest) = strip_lead(line) {
            line = rest;
        }
        let mut text = line.to_string();
        for mark in ["<u>", "</u>", "**", "__", "~~", "`", "*"] {
            text = text.replace(mark, "");
        }
        let words = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !words.is_empty() {
            return words.chars().take(TITLE_CHARS).collect();
        }
    }
    String::new()
}

fn millis(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl ScratchpadStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The file for `id`; ids are only ever `note_<ULID>`, so no path can
    /// leave the folder.
    fn path(&self, id: &str) -> Result<PathBuf, ScratchpadError> {
        if !has_prefix(id, PREFIX) {
            return Err(ScratchpadError::NotFound);
        }
        Ok(self.dir.join(format!("{id}.md")))
    }

    fn note(&self, id: &str, path: &Path, markdown: &str) -> Result<ScratchNote, ScratchpadError> {
        let meta = fs::metadata(path)?;
        Ok(ScratchNote {
            id: id.to_string(),
            title: title_of(markdown),
            updated_at: millis(&meta),
            bytes: meta.len(),
        })
    }

    /// Every note, most recently saved first.
    pub fn list(&self) -> Result<Vec<ScratchNote>, ScratchpadError> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err.into()),
        };
        let mut notes = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(id) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".md"))
                .filter(|id| has_prefix(id, PREFIX))
            else {
                continue;
            };
            // A note removed or unreadable meanwhile is left out.
            if let Ok(markdown) = fs::read_to_string(&path) {
                if let Ok(note) = self.note(id, &path, &markdown) {
                    notes.push(note);
                }
            }
        }
        notes.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        Ok(notes)
    }

    pub fn read(&self, id: &str) -> Result<String, ScratchpadError> {
        fs::read_to_string(self.path(id)?).map_err(|err| match err.kind() {
            io::ErrorKind::NotFound => ScratchpadError::NotFound,
            _ => err.into(),
        })
    }

    /// Saves a note: a new one without `id`, else that note, which must
    /// still exist (a save arriving after a delete doesn't bring it back).
    pub fn write(&self, id: Option<&str>, markdown: &str) -> Result<ScratchNote, ScratchpadError> {
        if markdown.len() > MAX_NOTE_BYTES {
            return Err(ScratchpadError::TooLarge);
        }
        let id = match id {
            Some(id) => {
                if !self.path(id)?.is_file() {
                    return Err(ScratchpadError::NotFound);
                }
                id.to_string()
            }
            None => new_id(PREFIX),
        };
        let path = self.path(&id)?;
        atomic_write(&path, markdown.as_bytes(), Some(0o600))?;
        self.note(&id, &path, markdown)
    }

    pub fn delete(&self, id: &str) -> Result<(), ScratchpadError> {
        fs::remove_file(self.path(id)?).map_err(|err| match err.kind() {
            io::ErrorKind::NotFound => ScratchpadError::NotFound,
            _ => err.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, ScratchpadStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = ScratchpadStore::new(dir.path().join("scratchpad"));
        (dir, store)
    }

    #[test]
    fn f_sh_12_notes_are_markdown_files_saved_listed_read_and_deleted() {
        let (dir, store) = store();
        assert_eq!(
            store.list().expect("empty").len(),
            0,
            "no folder yet: no notes"
        );
        let first = store.write(None, "# Groceries\n- [ ] milk\n").expect("new");
        assert!(has_prefix(&first.id, "note"));
        assert_eq!(first.title, "Groceries");
        let file = dir
            .path()
            .join("scratchpad")
            .join(format!("{}.md", first.id));
        assert_eq!(
            fs::read_to_string(&file).expect("file"),
            "# Groceries\n- [ ] milk\n"
        );
        let second = store.write(None, "").expect("second");
        assert_eq!(second.title, "");
        // A save replaces the text; the newest save lists first.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let saved = store
            .write(Some(&first.id), "# Groceries\n- [x] milk\n- [ ] eggs\n")
            .expect("save");
        assert_eq!(saved.bytes, 34);
        assert_eq!(
            store.read(&first.id).expect("read"),
            "# Groceries\n- [x] milk\n- [ ] eggs\n"
        );
        let ids: Vec<String> = store
            .list()
            .expect("list")
            .into_iter()
            .map(|n| n.id)
            .collect();
        assert_eq!(ids, vec![first.id.clone(), second.id.clone()]);
        store.delete(&first.id).expect("delete");
        assert!(!file.exists());
        assert!(matches!(
            store.read(&first.id),
            Err(ScratchpadError::NotFound)
        ));
        // A save that lands after the delete doesn't bring the note back.
        assert!(matches!(
            store.write(Some(&first.id), "late"),
            Err(ScratchpadError::NotFound)
        ));
        assert!(!file.exists());
        assert!(matches!(
            store.delete(&first.id),
            Err(ScratchpadError::NotFound)
        ));
    }

    #[test]
    fn f_sh_12_ids_name_only_notes_inside_the_folder() {
        let (dir, store) = store();
        fs::write(dir.path().join("secret.md"), "x").expect("outside");
        for id in [
            "../secret",
            "note_../../secret",
            "note_x",
            "",
            "note_01ARZ3NDEKTSV4RRFFQ69G5FA/",
        ] {
            assert!(
                matches!(store.read(id), Err(ScratchpadError::NotFound)),
                "{id}"
            );
            assert!(
                matches!(store.write(Some(id), "x"), Err(ScratchpadError::NotFound)),
                "{id}"
            );
            assert!(
                matches!(store.delete(id), Err(ScratchpadError::NotFound)),
                "{id}"
            );
        }
        // Other files in the folder aren't notes.
        fs::create_dir_all(dir.path().join("scratchpad")).expect("dir");
        fs::write(dir.path().join("scratchpad").join("readme.md"), "x").expect("stray");
        assert!(store.list().expect("list").is_empty());
    }

    #[test]
    fn f_sh_12_a_note_over_1_mb_is_refused_and_the_saved_one_stays() {
        let (_dir, store) = store();
        let note = store.write(None, "keep").expect("new");
        let big = "x".repeat(MAX_NOTE_BYTES + 1);
        assert!(matches!(
            store.write(Some(&note.id), &big),
            Err(ScratchpadError::TooLarge)
        ));
        assert_eq!(store.read(&note.id).expect("read"), "keep");
    }

    #[test]
    fn f_sh_12_the_title_is_the_first_line_with_words_without_its_marks() {
        assert_eq!(title_of("# Plan for **Monday**\nmore"), "Plan for Monday");
        assert_eq!(title_of("\n\n   \n- [ ] call <u>Ana</u>\n"), "call Ana");
        assert_eq!(title_of("1. *first* step"), "first step");
        assert_eq!(title_of("> quoted `code`"), "quoted code");
        assert_eq!(title_of("#hashtag stays"), "#hashtag stays");
        assert_eq!(title_of("fix my_var in __main__"), "fix my_var in main");
        assert_eq!(title_of("- \n# \nreal title"), "real title");
        assert_eq!(title_of(""), "");
        assert_eq!(title_of(&"word ".repeat(40)).chars().count(), 80);
    }
}
