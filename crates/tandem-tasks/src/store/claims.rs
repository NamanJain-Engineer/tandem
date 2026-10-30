//! Claims (F-TK-05, ARCHITECTURE §9.5): `claims.json`, the files each
//! working tree's agent said (or was seen) it is changing. Two working
//! trees of one repository claiming the same file overlap; the app turns a
//! new overlap into notices.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use tandem_core::contain;

use super::TaskStore;
use crate::error::TaskError;
use crate::lock;
use crate::model::*;

/// What an agent's pattern has: stored literally, `src/*.ts` would match
/// none of the files it meant, so an agent's claim refuses `*` and `?`. `[`
/// is a real file name's (`app/[id]/page.tsx`), and matches literally.
const PATTERN: [char; 2] = ['*', '?'];

/// An agent's path (`claim`, `release`) as it's kept: relative to the
/// working tree's top folder, `/`-separated, with no `.`, `..` or trailing
/// `/`. A relative path starts at the session's folder (`scope.prefix`); an
/// absolute one must be inside `scope.folder`. Anything else is refused, and
/// so is a pattern (`*`, `?`).
pub fn normalize_claim_path(scope: &ClaimScope, raw: &str) -> Result<String, TaskError> {
    let raw = screened(raw)?;
    // Windows' verbatim prefix (`\\?\C:\…`) is the one `?` a path may have.
    if raw.strip_prefix("//?/").unwrap_or(&raw).contains(PATTERN) {
        return Err(TaskError::invalid(
            "Claim files or a folder, not a pattern: `*` and `?` aren't allowed.",
        ));
    }
    resolve(&scope.folder, &scope.prefix, raw)
}

/// [`normalize_claim_path`] for a path already relative to the working
/// tree's top folder (the app's edits): checked and tidied the same way,
/// and refused when it's absolute. It names a real file, so `*`, `?` and
/// `[` are kept, and match literally.
pub fn normalize_relative_claim_path(raw: &str) -> Result<String, TaskError> {
    let raw = screened(raw)?;
    if is_absolute(&raw) {
        return Err(TaskError::invalid(
            "Paths are relative to your working tree's top folder.",
        ));
    }
    tidy(&raw)
}

/// A file the app saw edited ([`TaskStore::claim_inferred`]): absolute
/// inside the working tree at `folder`, or relative to its top.
fn seen_path(folder: &str, raw: &str) -> Result<String, TaskError> {
    resolve(folder, "", screened(raw)?)
}

/// 1 to [`CLAIM_PATHS_MAX`] paths a call.
fn claim_count(n: usize) -> Result<(), TaskError> {
    if n == 0 || n > CLAIM_PATHS_MAX {
        return Err(TaskError::invalid(format!(
            "Claim 1–{CLAIM_PATHS_MAX} paths at a time."
        )));
    }
    Ok(())
}

/// A [`screened`] `raw` from a session in `prefix` of the working tree at
/// `folder`.
fn resolve(folder: &str, prefix: &str, raw: String) -> Result<String, TaskError> {
    let rel = if is_absolute(&raw) {
        inside(folder, &raw)
            .ok_or_else(|| TaskError::invalid("That path is outside your working tree."))?
    } else {
        let prefix = prefix.replace('\\', "/");
        let prefix = prefix.trim_matches('/');
        if prefix.is_empty() {
            raw
        } else {
            format!("{prefix}/{raw}")
        }
    };
    tidy(&rel)
}

/// The checks every path gets, then `/` for `\`.
fn screened(raw: &str) -> Result<String, TaskError> {
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
    Ok(raw.replace('\\', "/"))
}

/// `/…`, or a drive (`C:…`), once `\` is `/`.
fn is_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with('/') || (bytes.len() > 1 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
}

/// A path's parts, without empty ones, `.` and Windows' verbatim `\\?\`
/// (`\\?\UNC\` too).
fn parts(path: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = path
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    if path.starts_with("//?/") && parts.first() == Some(&"?") {
        parts.remove(0);
        if parts.first() == Some(&"UNC") {
            parts.remove(0);
        }
    }
    parts
}

/// What's left of the absolute `path` under `folder`, as written; `None`
/// when it isn't under it.
fn inside(folder: &str, path: &str) -> Option<String> {
    let folder = folder.replace('\\', "/");
    let top = parts(&folder);
    let all = parts(path);
    if top.is_empty() || all.len() < top.len() {
        return None;
    }
    let same = top
        .iter()
        .zip(&all)
        .all(|(f, p)| claim_key(f) == claim_key(p));
    same.then(|| all[top.len()..].join("/"))
}

/// `path` without `.`, refused with `..` or when nothing is left.
fn tidy(path: &str) -> Result<String, TaskError> {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                return Err(TaskError::invalid(
                    "Paths can't use `..`: give a file outside your folder by its absolute path.",
                ))
            }
            p => parts.push(p),
        }
    }
    if parts.is_empty() {
        return Err(TaskError::invalid(
            "Claim files or folders, not the whole working tree.",
        ));
    }
    let out = parts.join("/");
    if out.len() > CLAIM_PATH_MAX {
        return Err(TaskError::invalid(format!(
            "Paths are at most {CLAIM_PATH_MAX} characters."
        )));
    }
    Ok(out)
}

/// The form two claim paths compare in: without a trailing `/` (a folder
/// saved with one, before it stopped mattering), and folded where file
/// names ignore case. Paths are kept as written.
pub fn claim_key(path: &str) -> Cow<'_, str> {
    let path = path.trim_end_matches('/');
    if contain::case_insensitive_fs() {
        Cow::Owned(contain::fold(path))
    } else {
        Cow::Borrowed(path)
    }
}

/// True when `dir` is `path` or a folder holding it (both [`claim_key`]s).
fn key_covers(dir: &str, path: &str) -> bool {
    !dir.is_empty()
        && path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// True when a claim on `claim` covers `path`: the same path, or one inside
/// it.
pub fn claim_covers(claim: &str, path: &str) -> bool {
    key_covers(&claim_key(claim), &claim_key(path))
}

/// The path two claims share: the same file, or a file inside a claimed
/// folder (the more specific path).
#[cfg(test)]
fn shared<'a>(a: &'a str, b: &'a str) -> Option<&'a str> {
    shared_keyed((a, &claim_key(a)), (b, &claim_key(b)))
}

/// [`shared`] for paths with their [`claim_key`]s.
fn shared_keyed<'a>(a: (&'a str, &str), b: (&'a str, &str)) -> Option<&'a str> {
    if a.1 == b.1 || key_covers(b.1, a.1) {
        Some(a.0)
    } else if key_covers(a.1, b.1) {
        Some(b.0)
    } else {
        None
    }
}

/// A working tree's name in notices: its folder's last part (a lane's slug).
fn agent_name(folder: &str) -> String {
    Path::new(folder)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| folder.to_string())
}

/// `(repo, first folder, second folder)`.
type PairKey<'a> = (&'a str, &'a str, &'a str);
/// The two agents' names and the paths they share.
type Pair<'a> = (&'a str, &'a str, Vec<&'a str>);

/// Every overlap between working trees in `claims`, one per pair. Only
/// claims in one repository are compared, each with its path's
/// [`claim_key`] worked out once.
pub fn overlaps_of(claims: &[Claim]) -> Vec<ClaimOverlap> {
    let mut repos: HashMap<&str, Vec<(&Claim, Cow<'_, str>)>> = HashMap::new();
    for c in claims {
        repos
            .entry(c.repo.as_str())
            .or_default()
            .push((c, claim_key(&c.path)));
    }
    let mut pairs: BTreeMap<PairKey<'_>, Pair<'_>> = BTreeMap::new();
    for group in repos.values() {
        for (i, &(a, ref ka)) in group.iter().enumerate() {
            for &(b, ref kb) in &group[i + 1..] {
                if a.folder == b.folder {
                    continue;
                }
                let Some(path) = shared_keyed((&a.path, ka), (&b.path, kb)) else {
                    continue;
                };
                let (first, second) = if a.folder < b.folder { (a, b) } else { (b, a) };
                pairs
                    .entry((&first.repo, &first.folder, &second.folder))
                    .or_insert_with(|| (&first.agent, &second.agent, Vec::new()))
                    .2
                    .push(path);
            }
        }
    }
    pairs
        .into_iter()
        .map(|((repo, f1, f2), (a1, a2, mut paths))| {
            // Sorted as paths compare, and each file once, however spelled.
            paths.sort_by_cached_key(|p| claim_key(p).into_owned());
            paths.dedup_by(|a, b| claim_key(a) == claim_key(b));
            ClaimOverlap {
                repo: repo.to_string(),
                folders: [f1.to_string(), f2.to_string()],
                agents: [a1.to_string(), a2.to_string()],
                paths: paths.into_iter().map(str::to_string).collect(),
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

    /// Drops the claims `gone` picks, under the claims lock; how many went.
    /// The file is written only when some did, so a sweep that finds
    /// nothing doesn't wake the app's watcher.
    fn remove_claims(&self, gone: impl Fn(&Claim) -> bool) -> Result<usize, TaskError> {
        let _g = lock::acquire(&self.lock_path("claims"))?;
        let path = self.claims_path();
        if !path.exists() {
            return Ok(0);
        }
        let mut claims: Vec<Claim> = Self::read_json(&path)?;
        let before = claims.len();
        claims.retain(|c| !gone(c));
        let removed = before - claims.len();
        if removed > 0 {
            self.write_json(&path, &claims)?;
        }
        Ok(removed)
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
    /// agents that don't call the tool. `paths` are absolute inside the
    /// working tree, or relative to its top folder whatever the session's
    /// `prefix` (what [`normalize_relative_claim_path`] takes). They're real
    /// files, so `*`, `?` and `[` are kept and match literally.
    pub fn claim_inferred(
        &self,
        scope: &ClaimScope,
        paths: &[String],
    ) -> Result<Vec<ClaimOverlap>, TaskError> {
        self.put(scope, None, paths, "", true)
    }

    /// Claims a person asked for on a working tree's behalf (F-WT-10's
    /// Claim these files): real files relative to its top folder, with an
    /// intent, kept as the agent's own claim would be (not inferred), so its
    /// agent and the other working trees' agents see them. Says which paths
    /// were claimed: the toast counted the ones left out too (the review of
    /// d06b9e3d, N-5).
    pub fn claim_for_tree(
        &self,
        scope: &ClaimScope,
        paths: &[String],
        intent: &str,
    ) -> Result<TreeClaim, TaskError> {
        if intent.chars().count() > CLAIM_INTENT_MAX {
            return Err(TaskError::invalid(format!(
                "The intent is at most {CLAIM_INTENT_MAX} characters."
            )));
        }
        claim_count(paths.len())?;
        // Real files from the top, so `*` and `?` in a name are kept: the
        // agents' check refused `src/what?.ts` as a pattern (the review of
        // 9a4e93ab). A name the checks would refuse or change (spaces at an
        // end, a `\`, a control character, too long) is left out and the
        // rest are claimed: one such file failed the whole claim, or another
        // name was claimed for it (the review of 30d5bcad).
        let paths: Vec<String> = paths
            .iter()
            .filter(|p| p.trim() == p.as_str() && !p.contains('\\'))
            .filter_map(|p| normalize_relative_claim_path(p).ok())
            .collect();
        if paths.is_empty() {
            return Err(TaskError::invalid(
                "None of these files can be claimed by their names.",
            ));
        }
        let overlaps = self.store_claims(scope, None, paths.clone(), intent.trim(), false)?;
        Ok(TreeClaim { paths, overlaps })
    }

    fn put(
        &self,
        scope: &ClaimScope,
        connection: Option<&str>,
        paths: &[String],
        intent: &str,
        inferred: bool,
    ) -> Result<Vec<ClaimOverlap>, TaskError> {
        claim_count(paths.len())?;
        // An agent's paths start at its folder and can't be patterns; the
        // files the app saw edited are real ones.
        let paths = paths
            .iter()
            .map(|p| {
                if inferred {
                    seen_path(&scope.folder, p)
                } else {
                    normalize_claim_path(scope, p)
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.store_claims(scope, connection, paths, intent, inferred)
    }

    /// Keeps checked `paths` (from the working tree's top) as `scope`'s
    /// claims, and returns the overlaps it's in.
    fn store_claims(
        &self,
        scope: &ClaimScope,
        connection: Option<&str>,
        paths: Vec<String>,
        intent: &str,
        inferred: bool,
    ) -> Result<Vec<ClaimOverlap>, TaskError> {
        let now = self.now();
        self.edit_claims(|claims| {
            // This working tree's claims, by how their paths compare.
            let mut mine: HashMap<String, usize> = claims
                .iter()
                .enumerate()
                .filter(|(_, c)| c.folder == scope.folder)
                .map(|(i, c)| (claim_key(&c.path).into_owned(), i))
                .collect();
            for path in paths {
                let key = claim_key(&path).into_owned();
                match mine.get(&key).and_then(|&i| claims.get_mut(i)) {
                    // An edit doesn't replace what the agent said it's doing.
                    Some(c) if inferred => {
                        c.at = now;
                        c.main = scope.main;
                    }
                    // Nor does a claim the app makes for the working tree
                    // (F-WT-10's Claim these files): the agent's own intent
                    // and connection stay.
                    Some(c) if connection.is_none() && c.connection.is_some() && !c.inferred => {}
                    Some(c) => {
                        c.intent = intent.to_string();
                        c.inferred = false;
                        c.connection = connection.map(str::to_string);
                        c.at = now;
                        c.main = scope.main;
                    }
                    None => {
                        mine.insert(key, claims.len());
                        claims.push(Claim {
                            path,
                            intent: intent.to_string(),
                            repo: scope.repo.clone(),
                            folder: scope.folder.clone(),
                            agent: agent_name(&scope.folder),
                            connection: connection.map(str::to_string),
                            inferred,
                            at: now,
                            main: scope.main,
                        });
                    }
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
    /// At most [`CLAIM_PATHS_MAX`] paths, checked and resolved before the
    /// claims lock is taken.
    pub fn release(&self, connection: &str, paths: Option<&[String]>) -> Result<usize, TaskError> {
        if paths.is_some_and(|p| p.len() > CLAIM_PATHS_MAX) {
            return Err(TaskError::invalid(format!(
                "Release at most {CLAIM_PATHS_MAX} paths at a time."
            )));
        }
        let scope = self.claim_scope(connection)?;
        let keys = match paths {
            Some(p) => Some(
                p.iter()
                    .map(|p| normalize_claim_path(&scope, p).map(|n| claim_key(&n).into_owned()))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            None => None,
        };
        self.remove_claims(|c| {
            c.folder == scope.folder
                && keys.as_ref().is_none_or(|ks| {
                    let key = claim_key(&c.path);
                    ks.iter().any(|k| key_covers(k, &key))
                })
        })
    }

    /// A working tree was merged or removed: its claims go (F-TK-05 AC3).
    pub fn release_folder(&self, folder: &str) -> Result<usize, TaskError> {
        self.remove_claims(|c| c.folder == folder)
    }

    /// Drops every claim `keep` refuses, under the claims lock, and says how
    /// many went: the app's sweeps of claims nothing will release (the main
    /// checkout's after a restart, a working tree deleted outside the app).
    pub fn prune_claims(&self, keep: impl Fn(&Claim) -> bool) -> Result<usize, TaskError> {
        self.remove_claims(|c| !keep(c))
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
            normalize_relative_claim_path("./src//a.ts").expect("tidy"),
            "src/a.ts"
        );
        assert_eq!(
            normalize_relative_claim_path("src\\auth\\").expect("folder"),
            "src/auth"
        );
        // A file the app saw edited is a real name, whatever it holds.
        for real in ["app/[id]/page.tsx", "notes?.md", "a*b.txt"] {
            assert_eq!(normalize_relative_claim_path(real).expect(real), real);
        }
        for bad in ["", "/etc/passwd", "C:/x", "a/../b", "./", "a\nb"] {
            assert!(normalize_relative_claim_path(bad).is_err(), "{bad:?}");
        }
    }

    fn scope(folder: &str, prefix: &str) -> ClaimScope {
        ClaimScope {
            repo: "/r".into(),
            folder: folder.into(),
            prefix: prefix.into(),
            main: false,
        }
    }

    #[test]
    fn f_tk_05_an_agents_path_starts_at_its_folder_or_is_absolute_inside_its_tree() {
        let top = scope("/r/wt", "");
        // `Repo::prefix()` on Windows, with a trailing separator.
        let sub = scope("/r/wt", "packages\\web\\");
        let ok = |s: &ClaimScope, raw: &str| normalize_claim_path(s, raw).expect(raw);
        assert_eq!(ok(&top, "src/a.ts"), "src/a.ts");
        assert_eq!(ok(&sub, "src/a.ts"), "packages/web/src/a.ts");
        assert_eq!(ok(&sub, "./"), "packages/web", "the session's folder");
        assert_eq!(ok(&sub, "/r/wt/packages/ui/b.ts"), "packages/ui/b.ts");
        assert_eq!(ok(&sub, "/r/wt//packages/./ui/"), "packages/ui");
        assert_eq!(ok(&top, "app/[id]/page.tsx"), "app/[id]/page.tsx");
        for pattern in ["src/*.ts", "src/?.ts", "/r/wt/*"] {
            assert!(normalize_claim_path(&top, pattern).is_err(), "{pattern}");
        }
        for outside in [
            "/r/wt",
            "/r/wt/",
            "/r/wt-2/a.ts",
            "/r/a.ts",
            "/r/wt/../a.ts",
            "C:/r/wt/a.ts",
            "../a.ts",
            "src/../../a.ts",
        ] {
            assert!(normalize_claim_path(&sub, outside).is_err(), "{outside}");
        }
        // Windows paths, with either slash and the verbatim prefix.
        let win = scope("C:\\Users\\me\\app", "");
        for inside in [
            "C:\\Users\\me\\app\\src\\a.ts",
            "C:/Users/me/app/src/a.ts",
            "\\\\?\\C:\\Users\\me\\app\\src\\a.ts",
        ] {
            assert_eq!(ok(&win, inside), "src/a.ts", "{inside}");
        }
        assert_eq!(
            ok(
                &scope("\\\\?\\C:\\Users\\me\\app", ""),
                "C:\\Users\\me\\app\\src\\a.ts"
            ),
            "src/a.ts"
        );
        assert!(normalize_claim_path(&win, "D:\\app\\a.ts").is_err());
        assert!(normalize_claim_path(&win, "\\\\?\\C:\\Users\\me\\app\\*.ts").is_err());
        let other_case = normalize_claim_path(&win, "c:\\users\\ME\\app\\Src\\a.ts");
        if contain::case_insensitive_fs() {
            assert_eq!(
                other_case.expect("folds"),
                "Src/a.ts",
                "the rest as written"
            );
        } else {
            assert!(other_case.is_err());
        }
    }

    #[test]
    fn f_tk_05_a_file_in_a_claimed_folder_overlaps() {
        assert_eq!(
            shared("src/auth", "src/auth/session.ts"),
            Some("src/auth/session.ts")
        );
        assert_eq!(
            shared("src/auth/session.ts", "src/auth"),
            Some("src/auth/session.ts")
        );
        // A folder claim saved with its slash, before the slash stopped
        // mattering, still covers its files.
        assert_eq!(
            shared("src/auth/", "src/auth/session.ts"),
            Some("src/auth/session.ts")
        );
        assert_eq!(shared("a.ts", "a.ts"), Some("a.ts"));
        assert_eq!(shared("src/auth", "src/authz.ts"), None);
        assert_eq!(shared("src/auth", "src/auth.ts"), None);
        assert_eq!(shared("src/a.ts", "src/b.ts"), None);
    }
}
