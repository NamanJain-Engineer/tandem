//! One instructions file for every engine (F-EN-07): the installed engines'
//! instructions rules from the registry, applied to a local repository's
//! main checkout by `tandem_git::instructions`.

use std::path::{Path, PathBuf};

use tandem_core::{DataDir, ErrorCode, IpcError};
use tandem_engines::registry::{EngineSpec, InstructionsMode};
use tandem_git::instructions::{EngineInput, EngineRules};
use tandem_git::repo::Repo;
use tandem_store::repo::workspaces::Workspace;

/// Backups of applied changes, one folder per change.
pub fn backups(data: &DataDir) -> PathBuf {
    data.root().join("instructions-backup")
}

/// An engine as `tandem_git::instructions` sees it.
pub fn input(spec: &EngineSpec) -> EngineInput {
    EngineInput {
        id: spec.id.clone(),
        label: spec.label.clone(),
        rules: spec.instructions.as_ref().map(|i| EngineRules {
            files: i.files.clone(),
            first: i.mode == InstructionsMode::First,
            fallback: i.fallback.clone(),
            pointer: i
                .pointer
                .as_ref()
                .map(|p| (p.file.clone(), p.import.clone())),
        }),
    }
}

/// The installed engines, in registry order.
pub fn installed(specs: &[EngineSpec], installed: impl Fn(&str) -> bool) -> Vec<EngineInput> {
    specs
        .iter()
        .filter(|s| installed(&s.id))
        .map(input)
        .collect()
}

/// Every instructions file the registry names, folders left out, in
/// registry order without repeats: what Lead reads to answer a plan run's
/// question (F-TK-08).
pub fn file_names(specs: &[EngineSpec]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for i in specs.iter().filter_map(|s| s.instructions.as_ref()) {
        for f in i.files.iter().chain(i.fallback.iter()) {
            if !f.ends_with('/') && !out.contains(f) {
                out.push(f.clone());
            }
        }
    }
    out
}

/// Where a workspace's instructions live: its repository's main checkout,
/// or the folder itself when it isn't a repository.
pub fn root_of(ws: &Workspace) -> Result<PathBuf, IpcError> {
    if ws.kind != "local" {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "Instructions are checked for folders on this computer",
        ));
    }
    let root = Path::new(&ws.root);
    Ok(match Repo::discover(root)? {
        Some(repo) => repo.main_root(),
        None => root.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_engines::registry::EngineRegistry;

    #[test]
    fn f_tk_08_1_lead_reads_every_instructions_file_the_registry_names_once() {
        let names = file_names(EngineRegistry::bundled().all());
        assert_eq!(
            names[..2],
            ["CLAUDE.md".to_string(), ".claude/CLAUDE.md".to_string()]
        );
        for n in ["AGENTS.md", "GEMINI.md", ".github/copilot-instructions.md"] {
            assert_eq!(names.iter().filter(|x| *x == n).count(), 1, "{n}");
        }
        // Folders aren't files to read.
        assert!(names.iter().all(|n| !n.ends_with('/')), "{names:?}");
    }

    #[test]
    fn f_en_07_installed_engines_carry_their_registry_rules_and_the_rest_are_unknown() {
        let reg = EngineRegistry::bundled();
        let got = installed(reg.all(), |id| matches!(id, "claude" | "codex" | "aider"));
        let ids: Vec<&str> = got.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["claude", "codex", "aider"]);
        let claude = got[0].rules.clone().expect("claude");
        assert_eq!(
            claude.pointer,
            Some(("CLAUDE.md".into(), "@AGENTS.md".into()))
        );
        assert!(got[1].rules.as_ref().is_some_and(|r| r.first));
        assert!(got[2].rules.is_none(), "aider is Unknown");
    }

    #[test]
    fn f_en_07_remote_folders_are_refused_and_a_plain_folder_is_its_own_root() {
        let dir = tempfile::tempdir().expect("dir");
        let mut ws = Workspace {
            id: tandem_core::ids::WorkspaceId::from_db("ws_1".into()),
            kind: "local".into(),
            host_id: "local".into(),
            root: dir.path().to_string_lossy().into_owned(),
            wsl_distro: None,
            name: "x".into(),
            parent_id: None,
            lane_branch: None,
            lane_base: None,
            sort_key: "a0".into(),
            pinned: false,
            collapsed: false,
            sessions_collapsed: false,
            created_at: 0,
        };
        assert_eq!(root_of(&ws).expect("root"), dir.path());
        ws.kind = "ssh".into();
        assert_eq!(root_of(&ws).expect_err("remote").code, ErrorCode::Invalid);
    }
}
