//! Claims (F-TK-05, ARCHITECTURE §9.5): `claims.json`, the files each
//! working tree's agent said (or was seen) it is changing. Two working
//! trees of one repository claiming the same file overlap; the app turns a
//! new overlap into notices.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::TaskStore;
use crate::error::TaskError;
use crate::lock;
use crate::model::*;

/// A claimed path relative to the working tree, `/`-separated, with no
/// `.` or `..`; a folder keeps its trailing `/`.
pub fn normalize_claim_path(raw: &str) -> Result<String, TaskError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(TaskError::invalid("A path is empty."));
    }
    if raw.len() > CLAIM_PATH_MAX {
        return Err(TaskError::invalid(format!(
            "Paths are at most {CLAIM_PATH_MAX} characters."
        )));
    }
    if raw.chars().any(char::is_control) {
        return Err(TaskError::invalid("A path has a control character."));
    }
    let raw = raw.replace('\\', "/");
    let bytes = raw.as_bytes();
    if raw.starts_with('/') || (bytes.len() > 1 && bytes[1] == b':') {
        return Err(TaskError::invalid(
            "Paths are relative to your working tree's top folder.",
        ));
    }
    let mut parts = Vec::new();
    for part in raw.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err(TaskError::invalid("Paths can't use `..`.")),
            p => parts.push(p),
        }
    }
    if parts.is_empty() {
        return Err(TaskError::invalid(
            "Claim files or folders, not the whole working tree.",
        ));
    }
    let mut out = parts.join("/");
    if raw.ends_with('/') {
        out.push('/');
    }
    Ok(out)
}

/// The path two claims share: the same file, or a file inside a claimed
/// folder (the more specific path).
fn shared<'a>(a: &'a str, b: &'a str) -> Option<&'a str> {
    if a == b {
        Some(a)
    } else if a.ends_with('/') && b.starts_with(a) {
        Some(b)
    } else if b.ends_with('/') && a.starts_with(b) {
        Some(a)
    } else {
        None
    }
}

/// True when `path` is `release` or inside it.
fn covered(path: &str, release: &str) -> bool {
    path == release || (release.ends_with('/') && path.starts_with(release))
}

/// A working tree's name in notices: its folder's last part (a lane's slug).
fn agent_name(folder: &str) -> String {
    Path::new(folder)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| folder.to_string())
}

/// `(repo, first folder, second folder)`.
type PairKey = (String, String, String);
/// The two agents' names and the paths they share.
type Pair = (String, String, Vec<String>);

/// Every overlap between working trees in `claims`, one per pair.
pub fn overlaps_of(claims: &[Claim]) -> Vec<ClaimOverlap> {
    let mut pairs: BTreeMap<PairKey, Pair> = BTreeMap::new();
    for (i, a) in claims.iter().enumerate() {
        for b in &claims[i + 1..] {
            if a.repo != b.repo || a.folder == b.folder {
                continue;
            }
            let Some(path) = shared(&a.path, &b.path) else {
                continue;
            };
            let (first, second) = if a.folder < b.folder { (a, b) } else { (b, a) };
            let entry = pairs
                .entry((
                    first.repo.clone(),
                    first.folder.clone(),
                    second.folder.clone(),
                ))
                .or_insert_with(|| (first.agent.clone(), second.agent.clone(), Vec::new()));
            entry.2.push(path.to_string());
        }
    }
    pairs
        .into_iter()
        .map(|((repo, f1, f2), (a1, a2, mut paths))| {
            paths.sort();
            paths.dedup();
            ClaimOverlap {
                repo,
                folders: [f1, f2],
                agents: [a1, a2],
                paths,
            }
        })
        .collect()
}

impl TaskStore {
    fn claims_path(&self) -> PathBuf {
        self.dir.join("claims.json")
    }

    /// Every claim, oldest first.
    pub fn claims(&self) -> Result<Vec<Claim>, TaskError> {
        let path = self.claims_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        Self::read_json(&path)
    }

    fn edit_claims<R>(
        &self,
        f: impl FnOnce(&mut Vec<Claim>) -> Result<R, TaskError>,
    ) -> Result<R, TaskError> {
        let _g = lock::acquire(&self.lock_path("claims"))?;
        let path = self.claims_path();
        let mut claims: Vec<Claim> = if path.exists() {
            Self::read_json(&path)?
        } else {
            Vec::new()
        };
        let out = f(&mut claims)?;
        self.write_json(&path, &claims)?;
        Ok(out)
    }

    /// The live connection's working tree, or `INVALID` when its session
    /// isn't in one.
    pub fn claim_scope(&self, connection: &str) -> Result<ClaimScope, TaskError> {
        self.connection(connection)?.scope.ok_or_else(|| {
            TaskError::invalid(
                "This session isn't in a git working tree, so it has no files to claim.",
            )
        })
    }

    /// `claim(paths, intent)` for a connection's agent. Returns the overlaps
    /// its working tree now has.
    pub fn claim(
        &self,
        connection: &str,
        paths: &[String],
        intent: &str,
    ) -> Result<Vec<ClaimOverlap>, TaskError> {
        let scope = self.claim_scope(connection)?;
        if intent.chars().count() > CLAIM_INTENT_MAX {
            return Err(TaskError::invalid(format!(
                "The intent is at most {CLAIM_INTENT_MAX} characters."
            )));
        }
        self.put(&scope, Some(connection), paths, intent.trim(), false)
    }

    /// Claims the app saw an agent make by editing (`PreToolUse`), for
    /// agents that don't call the tool.
    pub fn claim_inferred(
        &self,
        scope: &ClaimScope,
        paths: &[String],
    ) -> Result<Vec<ClaimOverlap>, TaskError> {
        self.put(scope, None, paths, "", true)
    }

    fn put(
        &self,
        scope: &ClaimScope,
        connection: Option<&str>,
        paths: &[String],
        intent: &str,
        inferred: bool,
    ) -> Result<Vec<ClaimOverlap>, TaskError> {
        if paths.is_empty() || paths.len() > CLAIM_PATHS_MAX {
            return Err(TaskError::invalid(format!(
                "Claim 1–{CLAIM_PATHS_MAX} paths at a time."
            )));
        }
        let paths = paths
            .iter()
            .map(|p| normalize_claim_path(p))
            .collect::<Result<Vec<_>, _>>()?;
        let now = self.now();
        self.edit_claims(|claims| {
            for path in paths {
                let existing = claims
                    .iter_mut()
                    .find(|c| c.folder == scope.folder && c.path == path);
                match existing {
                    // An edit doesn't replace what the agent said it's doing.
                    Some(c) if inferred => {
                        c.at = now;
                        c.main = scope.main;
                    }
                    Some(c) => {
                        c.intent = intent.to_string();
                        c.inferred = false;
                        c.connection = connection.map(str::to_string);
                        c.at = now;
                        c.main = scope.main;
                    }
                    None => claims.push(Claim {
                        path,
                        intent: intent.to_string(),
                        repo: scope.repo.clone(),
                        folder: scope.folder.clone(),
                        agent: agent_name(&scope.folder),
                        connection: connection.map(str::to_string),
                        inferred,
                        at: now,
                        main: scope.main,
                    }),
                }
            }
            trim_folder(claims, &scope.folder);
            Ok(overlaps_of(claims)
                .into_iter()
                .filter(|o| o.folders.contains(&scope.folder))
                .collect())
        })
    }

    /// `release(paths)`: the connection's working tree gives up `paths`
    /// (and what's inside a released folder), or every claim when `None`.
    pub fn release(&self, connection: &str, paths: Option<&[String]>) -> Result<usize, TaskError> {
        let scope = self.claim_scope(connection)?;
        let paths = match paths {
            Some(p) => Some(
                p.iter()
                    .map(|p| normalize_claim_path(p))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            None => None,
        };
        self.edit_claims(|claims| {
            let before = claims.len();
            claims.retain(|c| {
                c.folder != scope.folder
                    || paths
                        .as_ref()
                        .is_some_and(|ps| !ps.iter().any(|p| covered(&c.path, p)))
            });
            Ok(before - claims.len())
        })
    }

    /// A working tree was merged or removed: its claims go (F-TK-05 AC3).
    pub fn release_folder(&self, folder: &str) -> Result<usize, TaskError> {
        self.edit_claims(|claims| {
            let before = claims.len();
            claims.retain(|c| c.folder != folder);
            Ok(before - claims.len())
        })
    }

    /// Every overlap there is now.
    pub fn claim_overlaps(&self) -> Result<Vec<ClaimOverlap>, TaskError> {
        Ok(overlaps_of(&self.claims()?))
    }
}

/// A working tree over its cap loses its oldest inferred claims, then its
/// oldest.
fn trim_folder(claims: &mut Vec<Claim>, folder: &str) {
    let count = claims.iter().filter(|c| c.folder == folder).count();
    let mut over = count.saturating_sub(CLAIMS_PER_FOLDER_MAX);
    for inferred_pass in [true, false] {
        while over > 0 {
            let oldest = claims
                .iter()
                .enumerate()
                .filter(|(_, c)| c.folder == folder && (!inferred_pass || c.inferred))
                .min_by_key(|(_, c)| c.at)
                .map(|(i, _)| i);
            let Some(i) = oldest else { break };
            claims.remove(i);
            over -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_tk_05_paths_are_relative_and_tidy() {
        assert_eq!(
            normalize_claim_path("./src//a.ts").expect("tidy"),
            "src/a.ts"
        );
        assert_eq!(
            normalize_claim_path("src\\auth\\").expect("folder"),
            "src/auth/"
        );
        for bad in ["", "/etc/passwd", "C:/x", "a/../b", "./", "a\nb"] {
            assert!(normalize_claim_path(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn f_tk_05_a_file_in_a_claimed_folder_overlaps() {
        assert_eq!(
            shared("src/auth/", "src/auth/session.ts"),
            Some("src/auth/session.ts")
        );
        assert_eq!(shared("a.ts", "a.ts"), Some("a.ts"));
        assert_eq!(shared("src/auth", "src/auth/session.ts"), None);
        assert_eq!(shared("src/a.ts", "src/b.ts"), None);
    }
}
