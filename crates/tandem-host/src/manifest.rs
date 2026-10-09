//! The bundled `tandem-host` binaries and their SHA-256 manifest
//! (`resources/tandem-host/manifest.json`, written by
//! `scripts/build-tandem-host.sh` or CI). The desktop uploads the binary
//! for the host's platform and checks the uploaded copy's hash on the host.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const FILE: &str = "manifest.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binary {
    /// Relative to the manifest's folder.
    pub file: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub protocol: u32,
    pub version: String,
    /// By platform: `linux-x86_64`, `linux-aarch64`, `macos-universal`.
    pub binaries: BTreeMap<String, Binary>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("This build of Vibezeo doesn't include tandem-host binaries")]
    Missing,
    #[error("This build of Vibezeo doesn't include tandem-host for {0}")]
    NoBinary(String),
    #[error("The bundled tandem-host for {0} doesn't match its checksum")]
    Corrupt(String),
}

/// The platform for `uname -sm` output, or `None` when tandem-host isn't
/// built for it ("unsupported architecture").
pub fn platform(uname: &str) -> Option<&'static str> {
    let mut parts = uname.split_whitespace();
    let os = parts.next()?;
    let arch = parts.next()?;
    match (os, arch) {
        ("Linux", "x86_64" | "amd64") => Some("linux-x86_64"),
        ("Linux", "aarch64" | "arm64") => Some("linux-aarch64"),
        ("Darwin", "x86_64" | "arm64") => Some("macos-universal"),
        _ => None,
    }
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(hex::encode(Sha256::digest(&bytes)))
}

impl Manifest {
    pub fn load(dir: &Path) -> Result<Self, ManifestError> {
        let text = std::fs::read_to_string(dir.join(FILE)).map_err(|_| ManifestError::Missing)?;
        serde_json::from_str(&text).map_err(|_| ManifestError::Missing)
    }

    /// The binary for `platform`, checked against the manifest locally too.
    pub fn binary(&self, dir: &Path, platform: &str) -> Result<(PathBuf, &Binary), ManifestError> {
        let bin = self
            .binaries
            .get(platform)
            .ok_or_else(|| ManifestError::NoBinary(platform.into()))?;
        let path = dir.join(&bin.file);
        match sha256_file(&path) {
            Ok(h) if h == bin.sha256 => Ok((path, bin)),
            Ok(_) => Err(ManifestError::Corrupt(platform.into())),
            Err(_) => Err(ManifestError::NoBinary(platform.into())),
        }
    }

    /// Writes a manifest for the binaries found under `dir/<platform>/tandem-host`.
    pub fn generate(dir: &Path, protocol: u32, version: &str) -> std::io::Result<Self> {
        let mut binaries = BTreeMap::new();
        for p in ["linux-x86_64", "linux-aarch64", "macos-universal"] {
            let rel = format!("{p}/tandem-host");
            let path = dir.join(&rel);
            if let Ok(meta) = std::fs::metadata(&path) {
                binaries.insert(
                    p.to_string(),
                    Binary {
                        file: rel,
                        sha256: sha256_file(&path)?,
                        size: meta.len(),
                    },
                );
            }
        }
        let m = Manifest {
            protocol,
            version: version.into(),
            binaries,
        };
        let text = serde_json::to_string_pretty(&m)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(dir.join(FILE), text + "\n")?;
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_rm_01_platforms_from_uname() {
        assert_eq!(platform("Linux x86_64"), Some("linux-x86_64"));
        assert_eq!(platform("Linux aarch64\n"), Some("linux-aarch64"));
        assert_eq!(platform("Darwin arm64"), Some("macos-universal"));
        assert_eq!(platform("Darwin x86_64"), Some("macos-universal"));
        assert_eq!(platform("Linux armv7l"), None);
        assert_eq!(platform("FreeBSD amd64"), None);
        assert_eq!(platform(""), None);
    }

    #[test]
    fn f_rm_01_manifest_round_trip_and_checksum() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::create_dir_all(dir.path().join("linux-x86_64")).expect("mkdir");
        std::fs::write(dir.path().join("linux-x86_64/tandem-host"), b"binary").expect("w");
        let m = Manifest::generate(dir.path(), 1, "0.2.0").expect("gen");
        assert_eq!(Manifest::load(dir.path()).expect("load"), m);
        let (path, bin) = m.binary(dir.path(), "linux-x86_64").expect("bin");
        assert!(path.ends_with("linux-x86_64/tandem-host"));
        assert_eq!(bin.size, 6);
        assert_eq!(
            m.binary(dir.path(), "linux-aarch64").expect_err("absent"),
            ManifestError::NoBinary("linux-aarch64".into())
        );
        std::fs::write(dir.path().join("linux-x86_64/tandem-host"), b"tampered").expect("w");
        assert_eq!(
            m.binary(dir.path(), "linux-x86_64").expect_err("corrupt"),
            ManifestError::Corrupt("linux-x86_64".into())
        );
    }
}
