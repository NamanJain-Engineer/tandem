//! Local data controls (F-TL-05, ARCHITECTURE §16.5).
//!
//! - "Export my data" writes a zip of what the user made here: every table
//!   as JSON, teammates' memory and skills, tasks and scratchpad notes, and a
//!   manifest. Engine sign-ins (`providers/`), Vibezeo's own sign-in
//!   (`auth/`) and keychain values are never read, so they can't leak in.
//! - "Delete local data" is carried out on the next start, before anything
//!   opens `$DATA`: the command leaves a marker and the app relaunches. Engine
//!   sign-ins stay unless "Also remove engine sign-ins" was ticked.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use tandem_core::keychain::{self, Keychain};
use tandem_core::DataDir;
use tandem_store::repo::dump;
use tandem_store::{Store, StoreError};
use ts_rs::TS;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// Left in `$DATA` by "Delete local data"; read once at the next start.
pub const DELETE_MARKER: &str = ".delete-pending";

/// Folders under `$DATA` that hold the user's own work, copied as they are.
const EXPORT_DIRS: &[&str] = &["tasks", "scratchpad"];
/// Per-teammate folders copied from `teammates/<id>/`.
const TEAMMATE_DIRS: &[&str] = &["memory", "skills"];

/// What the manifest says an export leaves out, and why.
const EXCLUDED: &[&str] = &[
    "providers/: engine sign-ins, which belong to each engine",
    "auth/: your Vibezeo sign-in (encrypted)",
    "keychain secrets",
    "logs, caches, backups, the browser profile and terminal scrollback",
];

#[derive(Debug, thiserror::Error)]
pub enum DataControlsError {
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("{0}")]
    Store(#[from] StoreError),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
}

/// A finished export (F-TL-05).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DataExport {
    pub path: String,
    pub tables: u32,
    pub files: u32,
}

/// Writes the export zip to `dest`. The zip is built next to it and renamed
/// into place, so a failed export leaves nothing half-written.
pub fn export(
    store: &Store,
    data: &DataDir,
    dest: &Path,
    version: &str,
    exported_at: &str,
) -> Result<DataExport, DataControlsError> {
    let tables = store.read_blocking(dump::dump_tables)?;
    let part = dest.with_extension("zip.part");
    let result = (|| {
        let mut zip = ZipWriter::new(File::create(&part)?);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        let mut files = Vec::new();
        for table in &tables {
            let name = format!("{}.json", table.name);
            zip.start_file(name.as_str(), opts)?;
            serde_json::to_writer_pretty(&mut zip, &table.rows)?;
            files.push(name);
        }
        if let Ok(entries) = fs::read_dir(data.teammates()) {
            let mut ids: Vec<_> = entries.flatten().map(|e| e.file_name()).collect();
            ids.sort();
            for id in ids {
                let id = id.to_string_lossy().into_owned();
                for sub in TEAMMATE_DIRS {
                    let from = data.teammate_home(&id).join(sub);
                    add_tree(
                        &mut zip,
                        opts,
                        &from,
                        &format!("teammates/{id}/{sub}"),
                        &mut files,
                    )?;
                }
            }
        }
        for dir in EXPORT_DIRS {
            add_tree(&mut zip, opts, &data.root().join(dir), dir, &mut files)?;
        }
        let manifest = serde_json::json!({
            "app": "Vibezeo",
            "version": version,
            "exportedAt": exported_at,
            "format": 1,
            "tables": tables.iter().map(|t| &t.name).collect::<Vec<_>>(),
            "files": files,
            "excluded": EXCLUDED,
        });
        zip.start_file("manifest.json", opts)?;
        serde_json::to_writer_pretty(&mut zip, &manifest)?;
        zip.finish()?.flush()?;
        fs::rename(&part, dest)?;
        Ok(DataExport {
            path: dest.to_string_lossy().into_owned(),
            tables: u32::try_from(tables.len()).unwrap_or(u32::MAX),
            files: u32::try_from(files.len()).unwrap_or(u32::MAX),
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}

/// Adds the files under `from` as `prefix/…`. Symbolic links are skipped, so
/// a link inside a teammate's folder can't pull in anything from elsewhere.
fn add_tree(
    zip: &mut ZipWriter<File>,
    opts: SimpleFileOptions,
    from: &Path,
    prefix: &str,
    files: &mut Vec<String>,
) -> Result<(), DataControlsError> {
    let Ok(meta) = fs::symlink_metadata(from) else {
        return Ok(());
    };
    if !meta.is_dir() {
        return Ok(());
    }
    let mut entries: Vec<_> = fs::read_dir(from)?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = format!("{prefix}/{}", entry.file_name().to_string_lossy());
        let meta = fs::symlink_metadata(entry.path())?;
        if meta.is_dir() {
            add_tree(zip, opts, &entry.path(), &name, files)?;
        } else if meta.is_file() {
            zip.start_file(name.as_str(), opts)?;
            io::copy(&mut File::open(entry.path())?, zip)?;
            files.push(name);
        }
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteRequest {
    remove_engine_sign_ins: bool,
}

/// Marks `$DATA` for deletion at the next start (the app relaunches next).
pub fn request_delete(root: &Path, remove_engine_sign_ins: bool) -> io::Result<()> {
    let body = serde_json::to_vec(&DeleteRequest {
        remove_engine_sign_ins,
    })
    .map_err(io::Error::other)?;
    tandem_core::atomic::atomic_write(&root.join(DELETE_MARKER), &body, None)
}

/// Runs at startup before anything opens `$DATA`: carries out a delete the
/// user asked for. Everything under the root goes except `providers/`,
/// which goes only when they ticked "Also remove engine sign-ins". A marker
/// that can't be read keeps the engine sign-ins. Returns whether it ran.
pub fn apply_pending_delete(root: &Path) -> io::Result<bool> {
    let marker = root.join(DELETE_MARKER);
    let Ok(body) = fs::read(&marker) else {
        return Ok(false);
    };
    let remove_providers = serde_json::from_slice::<DeleteRequest>(&body)
        .map(|r| r.remove_engine_sign_ins)
        .unwrap_or(false);
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == DELETE_MARKER || (name == "providers" && !remove_providers) {
            continue;
        }
        let path = entry.path();
        if fs::symlink_metadata(&path)?.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    fs::remove_file(&marker)?;
    Ok(true)
}

/// Removes the keychain secrets Vibezeo itself keeps (its sign-in key), so
/// none outlive the data they protected. Engines keep their own sign-ins in
/// their provider homes, never in Vibezeo's keychain entries.
pub fn forget_secrets(keychain: &dyn Keychain) {
    let _ = keychain.delete(
        &keychain::service("auth"),
        crate::services::auth::KEY_ACCOUNT,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use tandem_core::keychain::MemoryKeychain;

    const SECRET: &str = "sk-keychain-SECRET-0123456789";
    const PROVIDER_TOKEN: &str = "oauth-PROVIDER-TOKEN-abcdef";
    const AUTH_BLOB: &str = "ENCRYPTED-AUTH-BLOB-xyz";

    fn rig() -> (tempfile::TempDir, DataDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        (dir, data, store)
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, text).expect("write");
    }

    fn unzip(path: &Path) -> Vec<(String, String)> {
        let mut zip = zip::ZipArchive::new(File::open(path).expect("open")).expect("zip");
        (0..zip.len())
            .map(|i| {
                let mut f = zip.by_index(i).expect("entry");
                let mut text = String::new();
                f.read_to_string(&mut text).expect("read");
                (f.name().to_string(), text)
            })
            .collect()
    }

    #[test]
    fn f_tl_05_1_the_export_has_teammates_threads_and_memory_and_no_keychain_values() {
        let (dir, data, store) = rig();
        store
            .write_blocking(|txn| {
                txn.execute(
                    "INSERT INTO teammates (id, name, name_fold, engine_id, home, settings_json, sort_key, created_at)
                     VALUES ('tm_1', 'Ada', 'ada', 'claude', '/t/ada', '{}', 'a0', 1)",
                    [],
                )?;
                Ok(())
            })
            .expect("insert");
        write(
            &data.teammate_home("tm_1").join("memory/notes.md"),
            "Ada prefers small PRs.",
        );
        write(
            &data.teammate_home("tm_1").join("skills/review/SKILL.md"),
            "# Review",
        );
        write(&data.scratchpad().join("note_1.md"), "my note");
        // Secrets the export must never contain.
        let keychain = MemoryKeychain::new();
        keychain
            .set(
                &keychain::service("auth"),
                crate::services::auth::KEY_ACCOUNT,
                SECRET.as_bytes(),
            )
            .expect("keychain");
        write(
            &data
                .provider_home("claude", "acc_1")
                .join(".credentials.json"),
            PROVIDER_TOKEN,
        );
        write(&data.auth_credentials(), AUTH_BLOB);

        let dest = dir.path().join("export.zip");
        let out = export(&store, &data, &dest, "0.3.0", "2026-10-10T08:00:00Z").expect("export");
        assert_eq!(out.path, dest.to_string_lossy());
        let entries = unzip(&dest);
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        for expected in [
            "teammates.json",
            "threads.json",
            "teammates/tm_1/memory/notes.md",
            "teammates/tm_1/skills/review/SKILL.md",
            "scratchpad/note_1.md",
            "manifest.json",
        ] {
            assert!(
                names.contains(&expected),
                "{expected} missing from {names:?}"
            );
        }
        let teammates = &entries
            .iter()
            .find(|(n, _)| n == "teammates.json")
            .expect("teammates")
            .1;
        assert!(teammates.contains("\"Ada\""));
        assert!(!names
            .iter()
            .any(|n| n.starts_with("providers/") || n.starts_with("auth/")));
        for (name, text) in &entries {
            for secret in [SECRET, PROVIDER_TOKEN, AUTH_BLOB] {
                assert!(!text.contains(secret), "{name} contains a secret");
            }
        }
        let manifest: serde_json::Value = serde_json::from_str(
            &entries
                .iter()
                .find(|(n, _)| n == "manifest.json")
                .expect("manifest")
                .1,
        )
        .expect("json");
        assert_eq!(manifest["version"], "0.3.0");
        assert!(manifest["excluded"].to_string().contains("providers/"));
        assert!(!dest.with_extension("zip.part").exists());
    }

    #[cfg(unix)]
    #[test]
    fn f_tl_05_the_export_skips_symbolic_links() {
        let (dir, data, store) = rig();
        let outside = dir.path().join("outside.txt");
        fs::write(&outside, PROVIDER_TOKEN).expect("outside");
        let memory = data.teammate_home("tm_1").join("memory");
        fs::create_dir_all(&memory).expect("mkdir");
        std::os::unix::fs::symlink(&outside, memory.join("link.md")).expect("symlink");
        let dest = dir.path().join("export.zip");
        export(&store, &data, &dest, "0.3.0", "now").expect("export");
        let entries = unzip(&dest);
        assert!(!entries
            .iter()
            .any(|(n, t)| n.ends_with("link.md") || t.contains(PROVIDER_TOKEN)));
    }

    fn populated() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("data");
        let data = DataDir::at(root.clone()).expect("data");
        write(&data.db_path(), "db");
        write(&data.logs().join("app.log"), "log");
        write(
            &data.teammate_home("tm_1").join("memory/notes.md"),
            "memory",
        );
        write(
            &data
                .provider_home("claude", "acc_1")
                .join(".credentials.json"),
            PROVIDER_TOKEN,
        );
        (dir, root)
    }

    #[test]
    fn f_tl_05_2_delete_local_data_keeps_provider_homes_unless_ticked() {
        let (_dir, root) = populated();
        request_delete(&root, false).expect("request");
        assert!(apply_pending_delete(&root).expect("apply"));
        let left: Vec<String> = fs::read_dir(&root)
            .expect("ls")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, vec!["providers".to_string()]);
        assert!(root
            .join("providers/claude/acc_1/.credentials.json")
            .exists());
    }

    #[test]
    fn f_tl_05_2_also_remove_engine_sign_ins_removes_provider_homes() {
        let (_dir, root) = populated();
        request_delete(&root, true).expect("request");
        assert!(apply_pending_delete(&root).expect("apply"));
        assert_eq!(fs::read_dir(&root).expect("ls").count(), 0);
    }

    #[test]
    fn f_tl_05_2_without_a_request_nothing_is_deleted_and_a_bad_marker_keeps_sign_ins() {
        let (_dir, root) = populated();
        assert!(!apply_pending_delete(&root).expect("apply"));
        assert!(root.join("tandem.db").exists());
        fs::write(root.join(DELETE_MARKER), "not json").expect("marker");
        assert!(apply_pending_delete(&root).expect("apply"));
        assert!(!root.join("tandem.db").exists());
        assert!(root
            .join("providers/claude/acc_1/.credentials.json")
            .exists());
    }

    #[test]
    fn f_tl_05_2_delete_forgets_vibezeo_keychain_secrets() {
        let keychain = MemoryKeychain::new();
        let service = keychain::service("auth");
        keychain
            .set(
                &service,
                crate::services::auth::KEY_ACCOUNT,
                SECRET.as_bytes(),
            )
            .expect("set");
        forget_secrets(&keychain);
        assert_eq!(
            keychain
                .get(&service, crate::services::auth::KEY_ACCOUNT)
                .expect("get"),
            None
        );
    }
}
