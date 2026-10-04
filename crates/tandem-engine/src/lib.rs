use std::path::{Path, PathBuf};

use thiserror::Error;

/// A hosted coding CLI. Tandem starts these. It does not replace them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Engine {
    pub id: &'static str,
    pub bin_names: &'static [&'static str],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnPlan {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EngineError {
    #[error("program is not a file")]
    Program,
    #[error("working directory is not a directory")]
    Cwd,
}

pub fn builtin() -> &'static [Engine] {
    &[
        Engine {
            id: "claude",
            bin_names: &["claude"],
        },
        Engine {
            id: "codex",
            bin_names: &["codex"],
        },
        Engine {
            id: "cursor",
            bin_names: &["cursor-agent", "agent"],
        },
        Engine {
            id: "gemini",
            bin_names: &["gemini"],
        },
    ]
}

pub fn get(id: &str) -> Option<&'static Engine> {
    builtin().iter().find(|engine| engine.id == id)
}

pub fn find_on_path(engine: &Engine, path_env: &str) -> Option<PathBuf> {
    for dir in split_path(path_env) {
        for name in engine.bin_names {
            let candidate = Path::new(dir).join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn plan(
    program: &Path,
    cwd: &Path,
    args: &[String],
    env: &[(&str, &str)],
) -> Result<SpawnPlan, EngineError> {
    if !program.is_file() {
        return Err(EngineError::Program);
    }
    if !cwd.is_dir() {
        return Err(EngineError::Cwd);
    }
    Ok(SpawnPlan {
        program: program.to_path_buf(),
        args: args.to_vec(),
        cwd: cwd.to_path_buf(),
        env: scrub_env(env),
    })
}

pub fn scrub_env(vars: &[(&str, &str)]) -> Vec<(String, String)> {
    vars.iter()
        .filter(|(key, _)| !is_secret_key(key))
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

fn is_secret_key(key: &str) -> bool {
    let folded = key.to_ascii_uppercase();
    [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "AUTHORIZATION",
        "API_KEY",
        "ACCESS_KEY",
    ]
    .iter()
    .any(|part| folded.contains(part))
}

fn split_path(path_env: &str) -> impl Iterator<Item = &str> {
    let sep = if path_env.contains(';') && !path_env.contains(':') {
        ';'
    } else {
        ':'
    };
    path_env.split(sep).filter(|entry| !entry.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<_> = builtin().iter().map(|engine| engine.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), builtin().len());
        assert!(get("missing").is_none());
        assert_eq!(get("codex").map(|engine| engine.id), Some("codex"));
    }

    #[test]
    fn path_search_prefers_earlier_dir_then_earlier_name() {
        let root = tempfile::tempdir().expect("tempdir");
        let first = root.path().join("first");
        let second = root.path().join("second");
        fs::create_dir_all(&first).expect("mkdir");
        fs::create_dir_all(&second).expect("mkdir");
        fs::write(first.join("agent"), b"").expect("write");
        fs::write(second.join("cursor-agent"), b"").expect("write");
        let engine = get("cursor").expect("cursor");
        let path_env = format!("{}:{}", first.display(), second.display());
        let found = find_on_path(engine, &path_env).expect("found");
        assert_eq!(found, first.join("agent"));
        assert!(find_on_path(engine, "").is_none());
    }

    #[test]
    fn spawn_plan_drops_secret_env() {
        let root = tempfile::tempdir().expect("tempdir");
        let program = root.path().join("claude");
        fs::write(&program, b"").expect("write");
        let spawn = plan(
            &program,
            root.path(),
            &["--print".to_string()],
            &[
                ("PATH", "/usr/bin"),
                ("HOME", "/tmp/home"),
                ("ANTHROPIC_API_KEY", "sk-secret"),
                ("GITHUB_TOKEN", "ghp_secret"),
            ],
        )
        .expect("plan");
        assert_eq!(spawn.args, vec!["--print".to_string()]);
        assert!(spawn.env.iter().any(|(key, _)| key == "PATH"));
        assert!(spawn.env.iter().all(|(key, _)| !is_secret_key(key)));
        assert!(plan(&root.path().join("missing"), root.path(), &[], &[]).is_err());
    }
}
