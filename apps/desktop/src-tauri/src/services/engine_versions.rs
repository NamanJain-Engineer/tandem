//! Engine versions under the user's control (F-EN-08): the table's rows,
//! the versions seen (`$DATA/engine-versions.json`), Pin, and the turn
//! results that make a version "last good". Reads detection and settings,
//! never the network: Check for updates is its own command.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_engines::detect::DetectionReport;
use tandem_engines::events::TurnEndReason;
use tandem_engines::registry::{EngineSpec, InstallMethod};
use tandem_engines::versions::{self, VersionBook};

pub const FILE: &str = "engine-versions.json";

/// A version change that threads have failed since.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VersionChange {
    pub from: String,
    pub to: String,
}

/// One row of the table (UI-161).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EngineVersionView {
    pub engine_id: String,
    pub version: Option<String>,
    pub method: InstallMethod,
    pub self_updates: bool,
    /// The registry has a documented switch to stop self-updates.
    pub pinnable: bool,
    pub pinned: bool,
    pub previous: Option<String>,
    pub last_good: Option<String>,
    pub broken: Option<VersionChange>,
}

/// The version book on disk, one writer at a time.
pub struct EngineVersions {
    path: PathBuf,
    lock: Mutex<()>,
}

impl EngineVersions {
    pub fn new(data_root: &Path) -> Self {
        Self {
            path: data_root.join(FILE),
            lock: Mutex::new(()),
        }
    }

    fn update<T>(&self, f: impl FnOnce(&mut VersionBook) -> T) -> T {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut book = VersionBook::parse(&std::fs::read(&self.path).unwrap_or_default());
        let before = book.clone();
        let out = f(&mut book);
        if book != before {
            if let Err(e) =
                tandem_core::atomic::atomic_write(&self.path, &book.to_json(), Some(0o600))
            {
                tracing::warn!(code = "engine_versions.write_failed", error = %e, "couldn't save engine versions");
            }
        }
        out
    }

    /// The book after taking in what detection last saw.
    pub fn sync(&self, report: Option<&DetectionReport>) -> VersionBook {
        self.update(|book| {
            for found in report.map(|r| r.found.as_slice()).unwrap_or_default() {
                if let Some(v) = &found.version {
                    book.observe(&found.engine_id, v);
                }
            }
            book.clone()
        })
    }

    /// A Thread turn of `engine` ended (`end_turn`'s health sink).
    pub fn record(&self, report: Option<&DetectionReport>, engine: &str, reason: TurnEndReason) {
        let ok = match reason {
            TurnEndReason::Completed => true,
            TurnEndReason::Error => false,
            // A stop or a usage limit says nothing about the version.
            TurnEndReason::Cancelled | TurnEndReason::Limit => return,
        };
        self.sync(report);
        if self.update(|book| book.record(engine, ok)) {
            tracing::info!(code = "engine_versions.recorded", ok, "engine turn result");
        }
    }
}

/// Where a found command resolves, through any links.
pub fn resolved(path: &str) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path))
}

/// The table's rows for the installed engines, in registry order.
pub fn views(
    specs: &[EngineSpec],
    report: Option<&DetectionReport>,
    book: &VersionBook,
    home: &Path,
    pinned: impl Fn(&str) -> bool,
) -> Vec<EngineVersionView> {
    let Some(report) = report else {
        return Vec::new();
    };
    specs
        .iter()
        .filter_map(|spec| {
            let found = report.get(&spec.id)?;
            let method =
                versions::install_method(&resolved(&found.path), home, spec.updates.as_ref());
            let seen = book.engines.get(&spec.id).cloned().unwrap_or_default();
            Some(EngineVersionView {
                engine_id: spec.id.clone(),
                version: found.version.clone(),
                method,
                self_updates: versions::self_updates(spec, method),
                pinnable: spec.updates.as_ref().is_some_and(|u| u.pin.is_some()),
                pinned: pinned(&spec.id),
                previous: seen.previous,
                last_good: seen.last_good,
                broken: book
                    .broken(&spec.id)
                    .map(|(from, to)| VersionChange { from, to }),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_engines::detect::{FoundEngine, FoundSource};
    use tandem_engines::registry::EngineRegistry;

    fn report(version: &str, path: &str) -> DetectionReport {
        DetectionReport {
            found: vec![FoundEngine {
                engine_id: "codex".into(),
                path: path.into(),
                version: Some(version.into()),
                source: FoundSource::Shell,
            }],
            ..DetectionReport::default()
        }
    }

    #[test]
    fn f_en_08_1_an_npm_install_at_1_2_0_shows_its_version_and_method() {
        let dir = tempfile::tempdir().expect("dir");
        let bin = dir
            .path()
            .join("lib/node_modules/@openai/codex/bin/codex.js");
        std::fs::create_dir_all(bin.parent().expect("parent")).expect("mkdir");
        std::fs::write(&bin, "#!/usr/bin/env node\n").expect("bin");
        #[cfg(unix)]
        let found = {
            let link = dir.path().join("codex");
            std::os::unix::fs::symlink(&bin, &link).expect("link");
            link
        };
        #[cfg(not(unix))]
        let found = bin.clone();
        let r = report("1.2.0", &found.to_string_lossy());
        let svc = EngineVersions::new(dir.path());
        let book = svc.sync(Some(&r));
        let reg = EngineRegistry::bundled();
        let rows = views(reg.all(), Some(&r), &book, dir.path(), |_| false);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].version.as_deref(), Some("1.2.0"));
        assert_eq!(rows[0].method, InstallMethod::Npm);
        assert!(rows[0].pinnable && !rows[0].pinned);
        assert!(views(reg.all(), None, &book, dir.path(), |_| false).is_empty());
    }

    #[test]
    fn f_en_08_3_codex_updated_from_1_2_0_to_1_3_0_and_a_failed_turn_is_reported() {
        let dir = tempfile::tempdir().expect("dir");
        let svc = EngineVersions::new(dir.path());
        let reg = EngineRegistry::bundled();
        let old = report("1.2.0", "/usr/local/bin/codex");
        svc.record(Some(&old), "codex", TurnEndReason::Completed);
        let new = report("1.3.0", "/usr/local/bin/codex");
        // A stop says nothing about the version.
        svc.record(Some(&new), "codex", TurnEndReason::Cancelled);
        assert_eq!(svc.sync(Some(&new)).broken("codex"), None);
        svc.record(Some(&new), "codex", TurnEndReason::Error);
        let book = svc.sync(Some(&new));
        let rows = views(reg.all(), Some(&new), &book, dir.path(), |id| id == "codex");
        assert_eq!(
            rows[0].broken,
            Some(VersionChange {
                from: "1.2.0".into(),
                to: "1.3.0".into()
            })
        );
        assert!(rows[0].pinned);
        assert_eq!(rows[0].previous.as_deref(), Some("1.2.0"));
        // Kept on disk, owner-only.
        let again = EngineVersions::new(dir.path()).sync(None);
        assert_eq!(
            again.broken("codex"),
            Some(("1.2.0".into(), "1.3.0".into()))
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join(FILE))
                .expect("file")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn f_en_08_3_only_the_engines_own_failures_count_against_its_version() {
        use crate::services::threads::version_health;
        use tandem_engines::events::ErrorStage;
        let failed = |stage| version_health(TurnEndReason::Error, Some(stage));
        assert_eq!(failed(ErrorStage::Spawn), Some(TurnEndReason::Error));
        assert_eq!(failed(ErrorStage::Unknown), Some(TurnEndReason::Error));
        // Offline, signed out, a model the account can't use, a tool's own
        // failure: not the version's fault.
        for stage in [
            ErrorStage::Network,
            ErrorStage::Auth,
            ErrorStage::Model,
            ErrorStage::Tool,
        ] {
            assert_eq!(failed(stage), None, "{stage:?}");
        }
        assert_eq!(
            version_health(TurnEndReason::Error, None),
            Some(TurnEndReason::Error)
        );
        assert_eq!(
            version_health(TurnEndReason::Completed, None),
            Some(TurnEndReason::Completed)
        );
        assert_eq!(version_health(TurnEndReason::Cancelled, None), None);
        assert_eq!(
            version_health(TurnEndReason::Limit, Some(ErrorStage::Unknown)),
            None
        );
    }
}
