//! A repository whose `.git/config` runs a command from each helper git
//! supports: fsmonitor, a clean filter, a textconv driver and hooks
//! (F-TL-06 rule 6). Shared with `tests/invariants`.
#![cfg(unix)]

use super::git;
use super::Fixture;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub struct Trap {
    pub f: Fixture,
    pub marks: PathBuf,
}

fn script(path: &Path, body: &str) {
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("script");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

pub fn trapped_repo() -> Trap {
    let f = Fixture::new();
    let marks = f.dir.path().join("marks");
    std::fs::create_dir_all(&marks).expect("mkdir");
    let bin = f.dir.path().join("bin");
    std::fs::create_dir_all(&bin).expect("mkdir");
    let m = marks.display();
    script(&bin.join("fsmonitor"), &format!("touch {m}/fsmonitor"));
    script(&bin.join("clean"), &format!("touch {m}/clean\ncat"));
    script(
        &bin.join("textconv"),
        &format!("touch {m}/textconv\ncat \"$1\""),
    );
    let hooks = f.dir.path().join("repo-hooks");
    std::fs::create_dir_all(&hooks).expect("mkdir");
    for hook in [
        "pre-commit",
        "post-checkout",
        "post-index-change",
        "reference-transaction",
    ] {
        script(&hooks.join(hook), &format!("touch {m}/hook-{hook}"));
    }
    std::fs::write(
        f.repo.join(".gitattributes"),
        "*.txt filter=trap diff=trap\n",
    )
    .expect("attrs");
    f.commit_all(&f.repo, "attributes");
    let b = bin.display();
    git(
        &f.repo,
        &["config", "core.fsmonitor", &format!("{b}/fsmonitor")],
    );
    git(
        &f.repo,
        &["config", "filter.trap.clean", &format!("{b}/clean")],
    );
    git(
        &f.repo,
        &["config", "diff.trap.textconv", &format!("{b}/textconv")],
    );
    git(
        &f.repo,
        &["config", "core.hooksPath", &hooks.display().to_string()],
    );
    git(
        &f.repo,
        &["config", "credential.helper", &format!("!{b}/fsmonitor")],
    );
    // A worktree change, so status and diff have content to look at.
    std::fs::write(f.repo.join("a.txt"), "one\ntwo\nthree\nfour\n").expect("write");
    Trap { f, marks }
}

pub fn fired(marks: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(marks)
        .expect("marks")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}
