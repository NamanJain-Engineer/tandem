//! `.git/info/exclude` (F-CD-15): Tandem never edits `.gitignore`.

use std::io::Write;
use std::path::Path;

pub const TANDEM_DIR_LINE: &str = ".tandem/";

/// What "Share with the repository" (F-EX-05, D-709) puts in place of
/// [`TANDEM_DIR_LINE`]: everything under `.tandem/` stays excluded but the
/// prompt library, which the repository then shares.
pub const SHARED_PROMPTS_LINES: [&str; 2] = [".tandem/*", "!.tandem/prompts/"];

fn exclude_lines(common_dir: &Path) -> Vec<String> {
    std::fs::read_to_string(common_dir.join("info").join("exclude"))
        .map(|t| {
            t.lines()
                .map(|l| l.trim_end_matches('\r').to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// The lines that exclude the whole `.tandem/` folder, prompts included.
fn hides_line(line: &str) -> bool {
    matches!(
        line.trim(),
        ".tandem/" | "/.tandem/" | ".tandem" | "/.tandem"
    )
}

/// `.git/info/exclude` hides `.tandem/prompts` from the repository.
pub fn hides_prompts(common_dir: &Path) -> bool {
    exclude_lines(common_dir).iter().any(|l| hides_line(l))
}

/// Replaces the lines hiding `.tandem/` with [`SHARED_PROMPTS_LINES`].
/// Returns whether it changed the file.
pub fn share_prompts(common_dir: &Path) -> std::io::Result<bool> {
    let lines = exclude_lines(common_dir);
    if !lines.iter().any(|l| hides_line(l)) {
        return Ok(false);
    }
    let mut out: Vec<String> = Vec::new();
    for l in lines {
        if hides_line(&l) {
            for s in SHARED_PROMPTS_LINES {
                if !out.iter().any(|o| o == s) {
                    out.push(s.to_string());
                }
            }
        } else {
            out.push(l);
        }
    }
    let path = common_dir.join("info").join("exclude");
    crate::exclude::write_atomic(&path, (out.join("\n") + "\n").as_bytes())?;
    Ok(true)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("vibezeo-tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// Lanes' exclusion (F-CD-15): `.tandem/` once, unless the shared form is
/// already there (it excludes the worktrees too).
pub fn ensure_tandem_excluded(common_dir: &Path) -> std::io::Result<bool> {
    if exclude_lines(common_dir)
        .iter()
        .any(|l| l == SHARED_PROMPTS_LINES[0])
    {
        return Ok(false);
    }
    ensure_excluded(common_dir, TANDEM_DIR_LINE)
}

/// Appends `line` to `<common_dir>/info/exclude` unless a line already says
/// exactly that. Returns whether it wrote.
pub fn ensure_excluded(common_dir: &Path, line: &str) -> std::io::Result<bool> {
    let info = common_dir.join("info");
    let path = info.join("exclude");
    let current = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e),
    };
    let text = String::from_utf8_lossy(&current);
    if text.lines().any(|l| l.trim_end_matches('\r') == line) {
        return Ok(false);
    }
    std::fs::create_dir_all(&info)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    if !current.is_empty() && !current.ends_with(b"\n") {
        file.write_all(b"\n")?;
    }
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(path: &Path) -> usize {
        std::fs::read_to_string(path)
            .expect("read")
            .lines()
            .filter(|l| *l == TANDEM_DIR_LINE)
            .count()
    }

    #[test]
    fn f_ex_05_4_sharing_narrows_the_line_and_lanes_keep_it_narrow() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::create_dir_all(dir.path().join("info")).expect("mkdir");
        let path = dir.path().join("info/exclude");
        std::fs::write(&path, "*.log\n.tandem/\n").expect("seed");
        assert!(hides_prompts(dir.path()));
        assert!(share_prompts(dir.path()).expect("share"));
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "*.log\n.tandem/*\n!.tandem/prompts/\n"
        );
        assert!(!hides_prompts(dir.path()));
        // A new lane doesn't hide the prompts again.
        assert!(!ensure_tandem_excluded(dir.path()).expect("lane"));
        assert_eq!(count(&path), 0);
        assert!(!share_prompts(dir.path()).expect("again"));
    }

    #[test]
    fn f_cd_15_exclude_is_appended_once() {
        let dir = tempfile::tempdir().expect("tmp");
        assert!(ensure_excluded(dir.path(), TANDEM_DIR_LINE).expect("first"));
        assert!(!ensure_excluded(dir.path(), TANDEM_DIR_LINE).expect("second"));
        assert_eq!(count(&dir.path().join("info/exclude")), 1);
    }

    #[test]
    fn f_cd_15_exclude_keeps_existing_lines() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::create_dir_all(dir.path().join("info")).expect("mkdir");
        let path = dir.path().join("info/exclude");
        std::fs::write(
            &path,
            "# git ls-files --others --exclude-from=.git/info/exclude\n*.log",
        )
        .expect("seed");
        ensure_excluded(dir.path(), TANDEM_DIR_LINE).expect("append");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "# git ls-files --others --exclude-from=.git/info/exclude\n*.log\n.tandem/\n"
        );
        ensure_excluded(dir.path(), TANDEM_DIR_LINE).expect("again");
        assert_eq!(count(&path), 1);
    }
}
