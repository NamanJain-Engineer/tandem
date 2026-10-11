//! `.git/info/exclude` (F-CD-15): Tandem never edits `.gitignore`.

use std::io::Write;
use std::path::Path;

pub const TANDEM_DIR_LINE: &str = ".tandem/";

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
