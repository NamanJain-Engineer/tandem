//! Help › Copy diagnostics (F-SH-11): version, OS, engine versions, flags and
//! the last 200 sanitized log lines. Home paths become `~`.

use std::path::Path;

use crate::logging;
use crate::redact::Redactor;

#[derive(Debug, Clone, Default)]
pub struct DiagnosticsInfo {
    pub app_version: String,
    pub build: String,
    pub engines: Vec<(String, String)>,
    pub flags: Vec<(String, String)>,
}

pub const TAIL_LINES: usize = 200;

pub fn bundle(logs_dir: &Path, data_root: &Path, info: &DiagnosticsInfo) -> String {
    let redactor = Redactor::global();
    let mut out = String::new();
    out.push_str(&format!("Vibezeo {} ({})\n", info.app_version, info.build));
    out.push_str(&format!(
        "OS: {} {}\n",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    out.push_str("Engines:\n");
    for (id, version) in &info.engines {
        out.push_str(&format!("  {id}: {version}\n"));
    }
    out.push_str("Flags:\n");
    for (name, value) in &info.flags {
        out.push_str(&format!("  {name}={value}\n"));
    }
    out.push_str(&format!("Last {TAIL_LINES} log lines:\n"));
    for line in logging::tail(logs_dir, TAIL_LINES) {
        out.push_str(&line);
        out.push('\n');
    }
    let data = data_root.to_string_lossy().to_string();
    let text = if data.len() > 1 {
        out.replace(&data, "$DATA")
    } else {
        out
    };
    let text = redactor.redact_home(&text);
    redactor.redact(&text).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::{RedactingFile, LOG_FILE, MAX_BYTES};

    #[test]
    fn f_sh_11_2_bundle_has_no_home_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let logs = dir.path().join("logs");
        let file = RedactingFile::new(&logs, LOG_FILE, MAX_BYTES);
        let home = crate::paths::user_home()
            .expect("home")
            .to_string_lossy()
            .to_string();
        file.write_text(&format!("opened {home}/code/project\n"))
            .expect("write");
        file.write_text(&format!("data at {}\n", dir.path().display()))
            .expect("write");
        let info = DiagnosticsInfo {
            app_version: "0.1.0".into(),
            build: "dev".into(),
            engines: vec![("claude".into(), "1.0.0".into())],
            flags: vec![],
        };
        let text = bundle(&logs, dir.path(), &info);
        assert!(
            !text.contains(&format!("{home}/")),
            "home path leaked: {text}"
        );
        assert!(text.contains("~/code/project"));
        assert!(text.contains("claude: 1.0.0"));
    }
}
