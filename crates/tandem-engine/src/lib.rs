use std::path::{Path, PathBuf};

/// A hosted coding CLI. Tandem starts these. It does not replace them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Engine {
    pub id: &'static str,
    pub bin_names: &'static [&'static str],
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
}
