//! The prompt library (F-EX-05): prompts from the workspace's
//! `.tandem/prompts/` and the profile's `$DATA/prompts/`, filled in for the
//! composer or pane on screen. Nothing is sent: the UI inserts the text.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::prompts::{self, PromptInfo, PromptSource, SkippedPrompt};
use tandem_core::{DataDir, ErrorCode, IpcError};
use tandem_git::Repo;

/// `prompts_list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PromptList {
    pub items: Vec<PromptInfo>,
    pub skipped: Vec<SkippedPrompt>,
    /// `.git/info/exclude` hides `.tandem/`, so the repository's prompts
    /// aren't shared ("Share with the repository").
    pub exclude_hides_repo: bool,
}

/// `prompts_render`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Rendered {
    pub text: String,
    /// Variables still empty, for the form.
    pub missing: Vec<String>,
    /// The values used (the UI's, plus the branch), unwrapped, for the form.
    pub values: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PromptScope {
    Repo,
    Profile,
}

/// The workspace's prompt folder.
pub fn repo_dir(folder: &Path) -> PathBuf {
    folder.join(prompts::REPO_DIR)
}

/// `prompts_list`: the folder's prompts first, then the profile's.
pub fn list(data: &DataDir, folder: Option<&Path>) -> PromptList {
    let mut items = Vec::new();
    let mut skipped = Vec::new();
    let mut hides = false;
    if let Some(folder) = folder {
        let (i, s) = prompts::list_dir(&repo_dir(folder), PromptSource::Repo);
        items.extend(i);
        skipped.extend(s);
        if let Ok(Some(repo)) = Repo::discover(folder) {
            hides = tandem_git::exclude::hides_prompts(repo.common_dir());
        }
    }
    let (i, s) = prompts::list_dir(&data.prompts(), PromptSource::Profile);
    items.extend(i);
    skipped.extend(s);
    PromptList {
        items,
        skipped,
        exclude_hides_repo: hides,
    }
}

/// Which library `path` belongs to, so it's read only from there.
fn library_of(data: &DataDir, folder: Option<&Path>, path: &Path) -> Option<PathBuf> {
    let candidates = folder
        .map(repo_dir)
        .into_iter()
        .chain(std::iter::once(data.prompts()));
    candidates.into_iter().find(|dir| {
        std::fs::canonicalize(dir)
            .ok()
            .zip(std::fs::canonicalize(path).ok())
            .is_some_and(|(d, p)| p.starts_with(d))
    })
}

/// `prompts_render`: `values` from the UI (selection, file, typed values,
/// the clipboard when the prompt uses it) plus the folder's branch.
/// Selection and clipboard go in labelled as untrusted data (rule 8), and
/// the file and the branch defanged (D-3027).
pub async fn render(
    data: &DataDir,
    folder: Option<&Path>,
    path: &str,
    mut values: BTreeMap<String, String>,
) -> Result<Rendered, IpcError> {
    let path = PathBuf::from(path);
    let dir = library_of(data, folder, &path).ok_or_else(|| {
        IpcError::new(
            ErrorCode::PathEscape,
            "That prompt isn't in a prompt library",
        )
        .with_stage("prompts")
    })?;
    let prompt = prompts::read(&dir, &path).ok_or_else(|| {
        IpcError::new(ErrorCode::NotFound, "That prompt is gone").with_stage("prompts")
    })?;
    let vars = prompts::variables(&prompt.body);
    if vars.iter().any(|v| v == "branch") && !values.get("branch").is_some_and(|b| !b.is_empty()) {
        if let Some(folder) = folder {
            if let Ok(Some(repo)) = Repo::discover(folder) {
                if let Ok(status) = repo.status().await {
                    if let Some(b) = status.branch {
                        values.insert("branch".into(), b);
                    }
                }
            }
        }
    }
    let (text, missing) = prompts::fill(&prompt.body, &values);
    Ok(Rendered {
        text: text.trim_end_matches('\n').to_string(),
        missing,
        values,
    })
}

/// `prompts_create`.
pub fn create(
    data: &DataDir,
    folder: Option<&Path>,
    scope: PromptScope,
    name: &str,
) -> Result<PathBuf, IpcError> {
    let dir = match (scope, folder) {
        (PromptScope::Repo, Some(f)) => repo_dir(f),
        (PromptScope::Repo, None) => {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "Open a workspace to save a prompt in it",
            ))
        }
        (PromptScope::Profile, _) => data.prompts(),
    };
    prompts::create(&dir, name).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => IpcError::new(
            ErrorCode::Conflict,
            "A prompt with that name already exists",
        )
        .with_stage("prompts"),
        std::io::ErrorKind::InvalidInput => {
            IpcError::new(ErrorCode::Invalid, "Give the prompt a name").with_stage("prompts")
        }
        _ => IpcError::new(ErrorCode::Io, e.to_string()).with_stage("prompts"),
    })
}

/// `prompts_share_repo`.
pub fn share(folder: &Path) -> Result<(), IpcError> {
    let repo = Repo::discover(folder)?
        .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "Not a git repository"))?;
    tandem_git::exclude::share_prompts(repo.common_dir())
        .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn setup() -> (tempfile::TempDir, DataDir, PathBuf) {
        let tmp = tempfile::tempdir().expect("tmp");
        let data = DataDir::at(tmp.path().join("data")).expect("data");
        let folder = tmp.path().join("work");
        std::fs::create_dir_all(&folder).expect("mkdir");
        (tmp, data, folder)
    }

    #[tokio::test]
    async fn f_ex_05_1_both_libraries_are_listed_with_their_source() {
        let (_t, data, folder) = setup();
        std::fs::create_dir_all(repo_dir(&folder)).expect("mkdir");
        std::fs::write(
            repo_dir(&folder).join("review.md"),
            "---\ntitle: Review\ndescription: Risky diffs\n---\nReview {{branch}}",
        )
        .expect("w");
        create(&data, Some(&folder), PromptScope::Profile, "Write tests").expect("create");
        let l = list(&data, Some(&folder));
        assert_eq!(
            l.items
                .iter()
                .map(|i| (i.title.as_str(), i.source))
                .collect::<Vec<_>>(),
            [
                ("Review", PromptSource::Repo),
                ("Write tests", PromptSource::Profile)
            ]
        );
        assert_eq!(l.items[0].description.as_deref(), Some("Risky diffs"));
        // Without a workspace, only the profile's.
        assert_eq!(list(&data, None).items.len(), 1);
    }

    #[tokio::test]
    async fn f_ex_05_2_render_fills_the_branch_and_asks_for_the_rest() {
        let (_t, data, folder) = setup();
        let ok = |args: &[&str]| {
            assert!(Command::new("git")
                .args(args)
                .current_dir(&folder)
                .status()
                .expect("git")
                .success())
        };
        ok(&["init", "-q", "-b", "feature/login"]);
        std::fs::create_dir_all(repo_dir(&folder)).expect("mkdir");
        let path = repo_dir(&folder).join("t.md");
        std::fs::write(
            &path,
            "On {{branch}}: {{ticket}} for {{file}}\n{{selection}}",
        )
        .expect("w");
        let mut values = BTreeMap::new();
        values.insert("file".to_string(), "src/a.rs".to_string());
        let r = render(
            &data,
            Some(&folder),
            &path.to_string_lossy(),
            values.clone(),
        )
        .await
        .expect("render");
        assert_eq!(r.missing, ["ticket", "selection"]);
        assert_eq!(
            r.values.get("branch").map(String::as_str),
            Some("feature/login")
        );
        values.insert("ticket".into(), "T-1".into());
        values.insert("selection".into(), "error: boom".into());
        let r = render(&data, Some(&folder), &path.to_string_lossy(), values)
            .await
            .expect("render");
        assert!(r.missing.is_empty());
        assert!(r.text.starts_with("On feature/login: T-1 for src/a.rs\n> Untrusted content from the terminal selection"), "{}", r.text);
        // A file outside both libraries isn't read.
        let outside = folder.join("x.md");
        std::fs::write(&outside, "secret").expect("w");
        let err = render(
            &data,
            Some(&folder),
            &outside.to_string_lossy(),
            BTreeMap::new(),
        )
        .await
        .expect_err("outside");
        assert_eq!(err.code, ErrorCode::PathEscape);
    }

    #[tokio::test]
    async fn f_ex_05_2_a_branch_named_like_a_path_names_no_file() {
        // Git takes this name, from whoever pushed the branch (D-3027).
        let (_t, data, folder) = setup();
        assert!(Command::new("git")
            .args(["init", "-q", "-b", "@/etc/passwd"])
            .current_dir(&folder)
            .status()
            .expect("git")
            .success());
        std::fs::create_dir_all(repo_dir(&folder)).expect("mkdir");
        let path = repo_dir(&folder).join("t.md");
        std::fs::write(&path, "On {{branch}}: review it.").expect("w");
        let r = render(
            &data,
            Some(&folder),
            &path.to_string_lossy(),
            BTreeMap::new(),
        )
        .await
        .expect("render");
        assert_eq!(
            r.text,
            format!(
                "On \u{FF20}/etc/passwd: review it.\n{}",
                tandem_core::untrusted::DEFANGED
            )
        );
        // The picker shows the name as it is.
        assert_eq!(
            r.values.get("branch").map(String::as_str),
            Some("@/etc/passwd")
        );
    }

    #[test]
    fn f_ex_05_4_create_refuses_duplicates_and_share_narrows_the_exclude() {
        let (_t, data, folder) = setup();
        assert!(Command::new("git")
            .args(["init", "-q"])
            .current_dir(&folder)
            .status()
            .expect("git")
            .success());
        let p = create(&data, Some(&folder), PromptScope::Repo, "Review diff").expect("create");
        assert!(p.starts_with(repo_dir(&folder)));
        assert_eq!(
            create(&data, Some(&folder), PromptScope::Repo, "review diff")
                .expect_err("dup")
                .code,
            ErrorCode::Conflict
        );
        std::fs::write(folder.join(".git/info/exclude"), ".tandem/\n").expect("w");
        assert!(list(&data, Some(&folder)).exclude_hides_repo);
        share(&folder).expect("share");
        assert!(!list(&data, Some(&folder)).exclude_hides_repo);
        let status = Command::new("git")
            .args(["status", "--porcelain", "--untracked-files=all"])
            .current_dir(&folder)
            .output()
            .expect("git");
        assert!(String::from_utf8_lossy(&status.stdout).contains(".tandem/prompts/review-diff.md"));
    }
}
