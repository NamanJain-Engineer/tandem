//! Skills (F-AG-09, ARCHITECTURE §8.5): `<home>/skills/<slug>/SKILL.md` with
//! front matter `name` and `description`. A skill whose front matter doesn't
//! parse, or whose name isn't kebab-case (≤48, the folder's name) or whose
//! description is empty or over 280 characters, is listed with its error and
//! left out of the briefing's index. The starter library is original text
//! bundled with the app.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const NAME_MAX: usize = 48;
pub const DESCRIPTION_MAX: usize = 280;

/// The starter library (F-AG-09): slug and SKILL.md.
pub const LIBRARY: &[(&str, &str)] = &[
    (
        "concise-diffs",
        include_str!("../assets/skills/concise-diffs/SKILL.md"),
    ),
    (
        "test-first",
        include_str!("../assets/skills/test-first/SKILL.md"),
    ),
    (
        "release-notes",
        include_str!("../assets/skills/release-notes/SKILL.md"),
    ),
    (
        "code-review-checklist",
        include_str!("../assets/skills/code-review-checklist/SKILL.md"),
    ),
    (
        "dependency-audit",
        include_str!("../assets/skills/dependency-audit/SKILL.md"),
    ),
    (
        "commit-messages",
        include_str!("../assets/skills/commit-messages/SKILL.md"),
    ),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SkillEntry {
    /// The folder's name.
    pub slug: String,
    pub name: String,
    pub description: String,
    pub valid: bool,
    pub error: Option<String>,
    /// Written by the teammate and not opened yet.
    pub is_new: bool,
    /// The engine accounts (ids) whose own skills folder has Vibezeo's copy
    /// of it (F-AG-10).
    pub synced_to: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Skill {
    pub entry: SkillEntry,
    pub markdown: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkillError {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Exists(String),
}

fn invalid(s: impl Into<String>) -> SkillError {
    SkillError::Invalid(s.into())
}

pub fn is_kebab(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= NAME_MAX
        && s.split('-').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// A scalar value from the front matter: plain, quoted, or a `>`/`|` block.
fn scalar(
    first: &str,
    rest: &mut std::iter::Peekable<std::slice::Iter<'_, &str>>,
) -> Result<String, SkillError> {
    let v = first.trim();
    if v == ">" || v == "|" || v == ">-" || v == "|-" {
        let folded = v.starts_with('>');
        let mut parts = Vec::new();
        while let Some(next) = rest.peek() {
            if next.starts_with(' ') || next.starts_with('\t') || next.trim().is_empty() {
                parts.push(next.trim().to_string());
                rest.next();
            } else {
                break;
            }
        }
        let joined = if folded {
            parts.join(" ")
        } else {
            parts.join("\n")
        };
        return Ok(joined.trim().to_string());
    }
    if let Some(inner) = v.strip_prefix('"') {
        let inner = inner
            .strip_suffix('"')
            .ok_or_else(|| invalid("A quoted value in the front matter isn't closed"))?;
        return Ok(inner.replace("\\\"", "\"").replace("\\\\", "\\"));
    }
    if let Some(inner) = v.strip_prefix('\'') {
        let inner = inner
            .strip_suffix('\'')
            .ok_or_else(|| invalid("A quoted value in the front matter isn't closed"))?;
        return Ok(inner.replace("''", "'"));
    }
    if v.starts_with(['[', '{', '&', '*', '!', '|', '>', '@', '`']) {
        return Err(invalid(
            "The front matter's values must be plain text (lists and objects aren't supported)",
        ));
    }
    // A comment after a plain value.
    let v = v.split(" #").next().unwrap_or(v).trim();
    Ok(v.to_string())
}

/// The front matter's `name` and `description` (YAML key: value lines,
/// quoted strings and folded or literal blocks).
pub fn parse_front_matter(md: &str) -> Result<(SkillMeta, &str), SkillError> {
    let md = md.strip_prefix('\u{feff}').unwrap_or(md);
    let body = md
        .strip_prefix("---\n")
        .or_else(|| md.strip_prefix("---\r\n"))
        .ok_or_else(|| invalid("SKILL.md must start with front matter (a line with ---)"))?;
    let end = body
        .find("\n---")
        .ok_or_else(|| invalid("The front matter isn't closed with a line of ---"))?;
    let head = &body[..end];
    let rest = body[end + 4..].trim_start_matches(['\r']);
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    let lines: Vec<&str> = head.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut it = lines.iter().peekable();
    let (mut name, mut description) = (None, None);
    while let Some(line) = it.next() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            return Err(invalid(
                "The front matter has an indented line where a key should be",
            ));
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| invalid(format!("This front matter line isn't key: value: {line}")))?;
        let value = scalar(value, &mut it)?;
        match key.trim() {
            "name" => name = Some(value),
            "description" => description = Some(value),
            _ => {}
        }
    }
    let name = name.ok_or_else(|| invalid("The front matter needs a name"))?;
    let description = description.ok_or_else(|| invalid("The front matter needs a description"))?;
    Ok((SkillMeta { name, description }, rest))
}

/// Checks a SKILL.md for the folder `slug`.
pub fn validate(md: &str, slug: &str) -> Result<SkillMeta, SkillError> {
    let (meta, _) = parse_front_matter(md)?;
    if !is_kebab(&meta.name) {
        return Err(invalid(
            "The name must be lowercase letters, numbers and dashes, at most 48 characters",
        ));
    }
    if meta.name != slug {
        return Err(invalid(format!(
            "The name ({}) must match the skill's folder ({slug})",
            meta.name
        )));
    }
    let n = meta.description.chars().count();
    if n == 0 {
        return Err(invalid("The description can't be empty"));
    }
    if n > DESCRIPTION_MAX {
        return Err(invalid("The description is at most 280 characters"));
    }
    Ok(meta)
}

fn skills_dir(home: &Path) -> PathBuf {
    home.join("skills")
}

fn entry(
    slug: &str,
    md: Result<String, std::io::Error>,
    seen: &[String],
    allow_new: bool,
) -> SkillEntry {
    let is_new = !seen.iter().any(|s| s == slug);
    let checked = md
        .map_err(|e| SkillError::Io(format!("Couldn't read SKILL.md: {e}")))
        .and_then(|md| validate(&md, slug));
    let (name, description, error) = match checked {
        Ok(m) => (m.name, m.description, None),
        Err(e) => (slug.to_string(), String::new(), Some(e.to_string())),
    };
    let error = error.or_else(|| {
        (is_new && !allow_new).then(|| {
            "Teammates can't write skills. Turn on \"Allow teammates to write skills\" in Settings › Teammates, or open it to keep it.".to_string()
        })
    });
    SkillEntry {
        slug: slug.to_string(),
        name,
        description,
        valid: error.is_none(),
        error,
        is_new,
        synced_to: Vec::new(),
    }
}

/// Every skill folder under `<home>/skills`, sorted by slug. `seen`: the
/// slugs the user has made or opened; any other is the teammate's own and
/// "New" (and, while teammates may not write skills, left out).
pub fn list(home: &Path, seen: &[String], allow_new: bool) -> Vec<SkillEntry> {
    let Ok(dir) = std::fs::read_dir(skills_dir(home)) else {
        return Vec::new();
    };
    let mut out: Vec<SkillEntry> = dir
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|slug| !slug.starts_with('.'))
        .map(|slug| {
            let md = std::fs::read_to_string(skills_dir(home).join(&slug).join("SKILL.md"));
            entry(&slug, md, seen, allow_new)
        })
        .collect();
    out.sort_by(|a, b| a.slug.cmp(&b.slug));
    out
}

fn slug_path(home: &Path, slug: &str) -> Result<PathBuf, SkillError> {
    if !is_kebab(slug) {
        return Err(invalid(
            "Use lowercase letters, numbers and dashes, at most 48 characters",
        ));
    }
    Ok(skills_dir(home).join(slug))
}

pub fn read(
    home: &Path,
    slug: &str,
    seen: &[String],
    allow_new: bool,
) -> Result<Skill, SkillError> {
    let path = slug_path(home, slug)?.join("SKILL.md");
    let markdown = std::fs::read_to_string(&path)
        .map_err(|_| SkillError::NotFound(format!("There's no skill called {slug}")))?;
    Ok(Skill {
        entry: entry(slug, Ok(markdown.clone()), seen, allow_new),
        markdown,
    })
}

/// Saves a SKILL.md after checking it (the user's editor, the library).
pub fn write(home: &Path, slug: &str, markdown: &str) -> Result<(), SkillError> {
    validate(markdown, slug)?;
    let dir = slug_path(home, slug)?;
    std::fs::create_dir_all(&dir).map_err(|e| SkillError::Io(e.to_string()))?;
    tandem_core::atomic::atomic_write(&dir.join("SKILL.md"), markdown.as_bytes(), Some(0o600))
        .map_err(|e| SkillError::Io(e.to_string()))
}

/// Removes a skill's folder (into the teammate's own `.trash`).
pub fn remove(home: &Path, slug: &str) -> Result<(), SkillError> {
    let dir = slug_path(home, slug)?;
    if !dir.is_dir() {
        return Err(SkillError::NotFound(format!(
            "There's no skill called {slug}"
        )));
    }
    let trash = home.join(".trash").join("skills");
    std::fs::create_dir_all(&trash).map_err(|e| SkillError::Io(e.to_string()))?;
    let to = trash.join(format!(
        "{slug}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    std::fs::rename(&dir, &to).map_err(|e| SkillError::Io(e.to_string()))
}

/// The starter library's entries.
pub fn library() -> Vec<SkillEntry> {
    LIBRARY
        .iter()
        .map(|(slug, md)| entry(slug, Ok(md.to_string()), &[slug.to_string()], true))
        .collect()
}

/// Copies a library skill into a teammate's skills.
pub fn add_from_library(home: &Path, slug: &str) -> Result<(), SkillError> {
    let (_, md) = LIBRARY
        .iter()
        .find(|(s, _)| *s == slug)
        .ok_or_else(|| SkillError::NotFound(format!("The library has no skill called {slug}")))?;
    if slug_path(home, slug)?.exists() {
        return Err(SkillError::Exists(format!(
            "{slug} is already one of its skills"
        )));
    }
    write(home, slug, md)
}

/// The file that marks a synced copy as Vibezeo's (F-AG-10): it holds the
/// path of the skill it was copied from.
pub const SYNC_MARKER: &str = ".tandem-sync";

/// One engine account's own skills folder (registry `skills_dir` under the
/// account's provider home).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncTarget {
    pub account_id: String,
    pub dir: PathBuf,
}

/// What a sync did (`skill_sync`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SyncState {
    /// Vibezeo's copy is in the engine's skills folder.
    Synced,
    /// Vibezeo's copy is gone (or was never there).
    Removed,
    /// The engine already has a skill of that name that isn't Vibezeo's; it
    /// was left as it is.
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SyncOutcome {
    pub state: SyncState,
}

/// What the engine's folder holds at a skill's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Found {
    Nothing,
    Ours,
    Theirs,
}

fn found(source: &Path, target: &Path) -> Found {
    let Ok(meta) = std::fs::symlink_metadata(target) else {
        return Found::Nothing;
    };
    if meta.file_type().is_symlink() {
        // A link Vibezeo made points at the skill itself.
        return match std::fs::read_link(target) {
            Ok(to) if to == source => Found::Ours,
            _ => Found::Theirs,
        };
    }
    if meta.is_dir() {
        let marker = std::fs::read_to_string(target.join(SYNC_MARKER)).ok();
        if marker.as_deref().map(str::trim) == source.to_str() {
            return Found::Ours;
        }
    }
    Found::Theirs
}

fn io(e: std::io::Error) -> SkillError {
    SkillError::Io(e.to_string())
}

/// Copies a skill's files (not links) into `to`, then the marker.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let dest = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &dest)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

/// Puts Vibezeo's copy of `slug` in the engine's skills folder (F-AG-10):
/// a copy with the marker, so the engine reads what the user kept. A
/// same-named folder that isn't Vibezeo's is a conflict and stays as it is;
/// Vibezeo's own copy is refreshed.
pub fn sync_on(home: &Path, slug: &str, target: &SyncTarget) -> Result<SyncState, SkillError> {
    let source = slug_path(home, slug)?;
    let md = std::fs::read_to_string(source.join("SKILL.md"))
        .map_err(|_| SkillError::NotFound(format!("There's no skill called {slug}")))?;
    validate(&md, slug)?;
    let dest = target.dir.join(slug);
    match found(&source, &dest) {
        Found::Theirs => return Ok(SyncState::Conflict),
        Found::Ours => remove_ours(&dest).map_err(io)?,
        Found::Nothing => {}
    }
    std::fs::create_dir_all(&target.dir).map_err(io)?;
    // Built beside the destination, then renamed in, so the engine never
    // reads half a skill.
    let staging = target.dir.join(format!(".{slug}.tandem-sync-tmp"));
    let _ = std::fs::remove_dir_all(&staging);
    let built = copy_tree(&source, &staging).and_then(|()| {
        std::fs::write(
            staging.join(SYNC_MARKER),
            source.to_string_lossy().as_bytes(),
        )
    });
    if let Err(e) = built.and_then(|()| std::fs::rename(&staging, &dest)) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(io(e));
    }
    Ok(SyncState::Synced)
}

fn remove_ours(dest: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(dest)?;
    if meta.file_type().is_symlink() {
        std::fs::remove_file(dest)
    } else {
        std::fs::remove_dir_all(dest)
    }
}

/// Takes Vibezeo's copy of `slug` out of the engine's skills folder. A
/// same-named skill that isn't Vibezeo's is never touched (a conflict).
pub fn sync_off(home: &Path, slug: &str, target: &SyncTarget) -> Result<SyncState, SkillError> {
    let source = slug_path(home, slug)?;
    let dest = target.dir.join(slug);
    match found(&source, &dest) {
        Found::Nothing => Ok(SyncState::Removed),
        Found::Theirs => Ok(SyncState::Conflict),
        Found::Ours => {
            remove_ours(&dest).map_err(io)?;
            Ok(SyncState::Removed)
        }
    }
}

/// The targets that hold Vibezeo's copy of `slug`.
pub fn synced_to(home: &Path, slug: &str, targets: &[SyncTarget]) -> Vec<String> {
    let Ok(source) = slug_path(home, slug) else {
        return Vec::new();
    };
    targets
        .iter()
        .filter(|t| found(&source, &t.dir.join(slug)) == Found::Ours)
        .map(|t| t.account_id.clone())
        .collect()
}

/// `list`, with each skill's synced targets filled in.
pub fn list_synced(
    home: &Path,
    seen: &[String],
    allow_new: bool,
    targets: &[SyncTarget],
) -> Vec<SkillEntry> {
    let mut out = list(home, seen, allow_new);
    for e in &mut out {
        e.synced_to = synced_to(home, &e.slug, targets);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: &str = "---\nname: weekly-deps\ndescription: Checks for outdated packages every week.\n---\n# Steps\n1. Run the audit.\n";

    #[test]
    fn f_ag_09_front_matter_rules() {
        assert_eq!(
            validate(OK, "weekly-deps").expect("ok").description,
            "Checks for outdated packages every week."
        );
        let quoted = "---\nname: \"a-b\"\ndescription: 'It''s quoted'\n---\n";
        assert_eq!(
            validate(quoted, "a-b").expect("ok").description,
            "It's quoted"
        );
        let folded = "---\nname: a-b\ndescription: >\n  Two lines\n  folded.\n---\nbody";
        assert_eq!(
            validate(folded, "a-b").expect("ok").description,
            "Two lines folded."
        );
        for (md, slug) in [
            ("no front matter", "x"),
            ("---\nname: x\n", "x"),
            (
                "---\nname: Weekly Deps\ndescription: d\n---\n",
                "weekly-deps",
            ),
            ("---\nname: weekly-deps\ndescription: d\n---\n", "other"),
            ("---\nname: a\ndescription: \n---\n", "a"),
            ("---\nname: a\n---\n", "a"),
            ("---\nname: [a]\ndescription: d\n---\n", "a"),
            ("---\nname: \"a\ndescription: d\n---\n", "a"),
        ] {
            assert!(validate(md, slug).is_err(), "{md:?} should be invalid");
        }
        let long = format!("---\nname: a\ndescription: {}\n---\n", "d".repeat(281));
        assert!(validate(&long, "a").is_err());
        assert!(is_kebab(&"a".repeat(48)));
        assert!(!is_kebab(&"a".repeat(49)));
        assert!(!is_kebab("a--b") && !is_kebab("-a") && !is_kebab("a_b"));
    }

    #[test]
    fn f_ag_09_1_release_notes_from_the_library_makes_a_valid_folder() {
        let dir = tempfile::tempdir().expect("dir");
        add_from_library(dir.path(), "release-notes").expect("add");
        let md =
            std::fs::read_to_string(dir.path().join("skills/release-notes/SKILL.md")).expect("md");
        validate(&md, "release-notes").expect("valid");
        assert!(matches!(
            add_from_library(dir.path(), "release-notes"),
            Err(SkillError::Exists(_))
        ));
        let listed = list(dir.path(), &["release-notes".into()], false);
        assert_eq!(listed.len(), 1);
        assert!(listed[0].valid && !listed[0].is_new);
    }

    #[test]
    fn f_ag_09_every_library_skill_is_valid_and_original() {
        assert_eq!(LIBRARY.len(), 6);
        for (slug, md) in LIBRARY {
            validate(md, slug).unwrap_or_else(|e| panic!("{slug}: {e}"));
        }
        assert_eq!(library().iter().filter(|e| e.valid).count(), 6);
    }

    #[test]
    fn f_ag_09_2_invalid_front_matter_is_listed_with_its_error_and_new_skills_wait_for_the_setting()
    {
        let dir = tempfile::tempdir().expect("dir");
        let home = dir.path();
        std::fs::create_dir_all(home.join("skills/broken")).expect("dir");
        std::fs::write(
            home.join("skills/broken/SKILL.md"),
            "---\nname: Broken!\n---\n",
        )
        .expect("w");
        write(home, "weekly-deps", OK).expect("write");
        // A folder the teammate made itself.
        std::fs::create_dir_all(home.join("skills/its-own")).expect("dir");
        std::fs::write(
            home.join("skills/its-own/SKILL.md"),
            "---\nname: its-own\ndescription: Its own.\n---\n",
        )
        .expect("w");
        let seen = vec!["broken".to_string(), "weekly-deps".to_string()];
        let listed = list(home, &seen, false);
        let get = |s: &str| listed.iter().find(|e| e.slug == s).expect(s).clone();
        assert!(!get("broken").valid);
        assert!(get("broken").error.is_some());
        assert!(get("weekly-deps").valid);
        assert!(get("its-own").is_new);
        assert!(!get("its-own").valid, "teammates may not write skills yet");
        let allowed = list(home, &seen, true);
        let own = allowed.iter().find(|e| e.slug == "its-own").expect("own");
        assert!(own.valid && own.is_new);
        assert!(write(home, "bad", "---\nname: bad\n---\n").is_err());
        remove(home, "weekly-deps").expect("remove");
        assert!(!home.join("skills/weekly-deps").exists());
    }

    fn target(dir: &Path) -> SyncTarget {
        SyncTarget {
            account_id: "acc_claude".into(),
            dir: dir.join("provider/skills"),
        }
    }

    #[test]
    fn f_ag_10_1_syncing_to_claude_creates_the_skill_md_and_removing_it_deletes_it() {
        let dir = tempfile::tempdir().expect("dir");
        let home = dir.path().join("teammate");
        write(&home, "weekly-deps", OK).expect("write");
        let t = target(dir.path());
        assert_eq!(sync_on(&home, "weekly-deps", &t), Ok(SyncState::Synced));
        let copy = dir.path().join("provider/skills/weekly-deps/SKILL.md");
        assert_eq!(std::fs::read_to_string(&copy).expect("copy"), OK);
        assert_eq!(
            synced_to(&home, "weekly-deps", std::slice::from_ref(&t)),
            vec!["acc_claude"]
        );
        let listed = list_synced(
            &home,
            &["weekly-deps".into()],
            false,
            std::slice::from_ref(&t),
        );
        assert_eq!(listed[0].synced_to, vec!["acc_claude"]);
        // Syncing again refreshes Vibezeo's own copy.
        let edited = OK.replace("every week", "every day");
        write(&home, "weekly-deps", &edited).expect("edit");
        assert_eq!(sync_on(&home, "weekly-deps", &t), Ok(SyncState::Synced));
        assert_eq!(std::fs::read_to_string(&copy).expect("copy"), edited);
        assert_eq!(sync_off(&home, "weekly-deps", &t), Ok(SyncState::Removed));
        assert!(!dir.path().join("provider/skills/weekly-deps").exists());
        assert!(synced_to(&home, "weekly-deps", std::slice::from_ref(&t)).is_empty());
        // Off again is a no-op; nothing else in the folder moved.
        assert_eq!(sync_off(&home, "weekly-deps", &t), Ok(SyncState::Removed));
        assert!(dir.path().join("provider/skills").is_dir());
    }

    #[test]
    fn f_ag_10_2_a_same_name_folder_that_isnt_ours_is_a_conflict_and_left_untouched() {
        let dir = tempfile::tempdir().expect("dir");
        let home = dir.path().join("teammate");
        write(&home, "weekly-deps", OK).expect("write");
        let t = target(dir.path());
        let theirs = t.dir.join("weekly-deps");
        std::fs::create_dir_all(&theirs).expect("dir");
        std::fs::write(theirs.join("SKILL.md"), "the user's own").expect("w");
        assert_eq!(sync_on(&home, "weekly-deps", &t), Ok(SyncState::Conflict));
        assert_eq!(sync_off(&home, "weekly-deps", &t), Ok(SyncState::Conflict));
        assert_eq!(
            std::fs::read_to_string(theirs.join("SKILL.md")).expect("r"),
            "the user's own"
        );
        assert_eq!(std::fs::read_dir(&theirs).expect("ls").count(), 1);
        assert!(synced_to(&home, "weekly-deps", std::slice::from_ref(&t)).is_empty());
        // Another teammate's copy of the same name isn't ours either.
        let other = dir.path().join("other");
        write(&other, "weekly-deps", OK).expect("write");
        std::fs::remove_dir_all(&theirs).expect("rm");
        assert_eq!(sync_on(&other, "weekly-deps", &t), Ok(SyncState::Synced));
        assert_eq!(sync_on(&home, "weekly-deps", &t), Ok(SyncState::Conflict));
        assert_eq!(sync_off(&home, "weekly-deps", &t), Ok(SyncState::Conflict));
        assert!(theirs.join("SKILL.md").is_file());
        // A file of that name, or a link Vibezeo didn't make, counts too.
        let t2 = SyncTarget {
            account_id: "acc_2".into(),
            dir: dir.path().join("p2/skills"),
        };
        std::fs::create_dir_all(&t2.dir).expect("dir");
        std::fs::write(t2.dir.join("weekly-deps"), "a file").expect("w");
        assert_eq!(sync_on(&home, "weekly-deps", &t2), Ok(SyncState::Conflict));
        assert_eq!(
            std::fs::read_to_string(t2.dir.join("weekly-deps")).expect("r"),
            "a file"
        );
    }

    #[test]
    fn f_ag_10_only_a_valid_existing_skill_is_synced() {
        let dir = tempfile::tempdir().expect("dir");
        let home = dir.path().join("teammate");
        let t = target(dir.path());
        assert!(matches!(
            sync_on(&home, "missing", &t),
            Err(SkillError::NotFound(_))
        ));
        std::fs::create_dir_all(home.join("skills/broken")).expect("dir");
        std::fs::write(home.join("skills/broken/SKILL.md"), "no front matter").expect("w");
        assert!(matches!(
            sync_on(&home, "broken", &t),
            Err(SkillError::Invalid(_))
        ));
        assert!(!t.dir.join("broken").exists());
        assert!(sync_on(&home, "../escape", &t).is_err());
    }
}
