use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use crate::error::Error;
use crate::paths::Paths;
use crate::redact::redact;

const MAX_BYTES: u64 = 10 * 1024 * 1024;
const KEEP: usize = 5;

pub fn append_log(paths: &Paths, line: &str) -> Result<(), Error> {
    fs::create_dir_all(&paths.logs).map_err(|err| {
        Error::new(
            "logging",
            format!("create logs: {err}"),
            "check TANDEM_HOME permissions",
        )
    })?;
    let file = paths.logs.join("tandem.log");
    rotate_if_needed(&file)?;
    let mut handle = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .map_err(|err| Error::new("logging", format!("open log: {err}"), "check disk space"))?;
    writeln!(handle, "{}", redact(line))
        .map_err(|err| Error::new("logging", format!("write log: {err}"), "retry"))?;
    Ok(())
}

fn rotate_if_needed(file: &Path) -> Result<(), Error> {
    let len = fs::metadata(file).map(|meta| meta.len()).unwrap_or(0);
    if len < MAX_BYTES {
        return Ok(());
    }
    let oldest = file.with_file_name(format!("tandem.log.{}", KEEP - 1));
    if oldest.exists() {
        fs::remove_file(&oldest).map_err(|err| {
            Error::new(
                "logging",
                format!("drop old log: {err}"),
                "check disk space",
            )
        })?;
    }
    for index in (1..KEEP - 1).rev() {
        let from = file.with_file_name(format!("tandem.log.{index}"));
        let to = file.with_file_name(format!("tandem.log.{}", index + 1));
        if from.exists() {
            fs::rename(&from, &to).map_err(|err| {
                Error::new("logging", format!("rotate: {err}"), "check disk space")
            })?;
        }
    }
    fs::rename(file, file.with_file_name("tandem.log.1")).map_err(|err| {
        Error::new(
            "logging",
            format!("rotate current: {err}"),
            "check disk space",
        )
    })?;
    Ok(())
}

pub fn diagnostics_bundle(paths: &Paths) -> Result<String, Error> {
    let log_path = paths.logs.join("tandem.log");
    let raw = fs::read_to_string(&log_path).unwrap_or_default();
    let lines: Vec<&str> = raw.lines().collect();
    let start = lines.len().saturating_sub(200);
    let tail = lines[start..].join("\n");
    let mut text = format!(
        "version={}\nos={}\n{}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        tail
    );
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        text = text.replace(home.to_string_lossy().as_ref(), "~");
    }
    text = text.replace(paths.data.to_string_lossy().as_ref(), "~");
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    #[test]
    fn f_sh_11_1_secret_patterns_absent_from_log_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = Paths::from_data(dir.path().to_path_buf());
        let secret = "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij";
        append_log(&paths, &format!("using {secret}")).expect("log");
        let body = fs::read_to_string(paths.logs.join("tandem.log")).expect("read");
        assert!(!body.contains(secret));
        assert!(body.contains("[REDACTED]"));
    }

    #[test]
    fn f_sh_11_2_bundle_has_no_home_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = Paths::from_data(dir.path().to_path_buf());
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp/tandem-home".to_string());
        append_log(&paths, &format!("opened {home}/project")).expect("log");
        let bundle = diagnostics_bundle(&paths).expect("bundle");
        assert!(!bundle.contains(&home));
        assert!(bundle.contains('~'));
        assert!(!bundle.contains(dir.path().to_string_lossy().as_ref()));
    }
}
