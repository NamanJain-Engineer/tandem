//! Local speech models (F-DI-04, ARCHITECTURE §12.4): the pinned catalog, the
//! models on disk under `$DATA/models/<id>/` and the downloader.
//!
//! Downloads resume from `<file>.partial` with an HTTP range, wait out
//! `Retry-After` on 429/503, and are only kept when both the size and the
//! SHA-256 match the catalog; anything else is deleted and the UI offers
//! Retry. A symlink anywhere in a model's folder is refused.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

pub const CATALOG_JSON: &str = include_str!("../models/catalog.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "ModelRuntime")]
pub enum Runtime {
    Whisper,
    Onnx,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum PerFile<T> {
    One(T),
    Many(BTreeMap<String, T>),
}

#[derive(Debug, Clone, Deserialize)]
struct RawModel {
    id: String,
    name: String,
    runtime: Runtime,
    url: String,
    #[serde(default)]
    files: Vec<String>,
    sha256: PerFile<String>,
    bytes: PerFile<u64>,
    #[serde(default)]
    languages: Vec<String>,
    langs_label: String,
    #[serde(default)]
    recommended: bool,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    default_on: Vec<String>,
    #[serde(default)]
    warn_cpu: Vec<String>,
    license: String,
}

#[derive(Debug, Deserialize)]
struct RawCatalog {
    models: Vec<RawModel>,
}

/// One file of a model, with what it must be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFile {
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub runtime: Runtime,
    pub files: Vec<ModelFile>,
    /// BCP-47 codes, or `*` for many.
    pub languages: Vec<String>,
    pub langs_label: String,
    pub recommended: bool,
    exclude: Vec<String>,
    default_on: Vec<String>,
    warn_cpu: Vec<String>,
    pub license: String,
}

impl Model {
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|f| f.bytes).sum()
    }

    pub fn available_on(&self, platform: &str) -> bool {
        !self.exclude.iter().any(|p| p == platform)
    }

    /// Recommended on this platform (Whisper Turbo on Intel Macs).
    pub fn recommended_on(&self, platform: &str) -> bool {
        self.default_on.iter().any(|p| p == platform)
            || self.recommended && self.available_on(platform) && !platform_has_default(platform)
    }

    /// The "CPU transcription" badge.
    pub fn cpu_warning_on(&self, platform: &str) -> bool {
        let short = platform.split('-').next().unwrap_or("");
        let short = match short {
            "windows" => "win",
            "linux" => "lin",
            s => s,
        };
        self.warn_cpu.iter().any(|p| p == short)
    }

    pub fn supports_language(&self, lang: &str) -> bool {
        lang == "auto"
            || self.languages.iter().any(|l| l == "*")
            || self
                .languages
                .iter()
                .any(|l| lang == l || lang.starts_with(&format!("{l}-")))
    }
}

fn platform_has_default(platform: &str) -> bool {
    Catalog::bundled()
        .models
        .iter()
        .any(|m| m.default_on.iter().any(|p| p == platform))
}

/// `mac-aarch64`, `mac-x86_64`, `windows-x86_64`, `linux-x86_64`.
pub fn current_platform() -> String {
    let os = if cfg!(target_os = "macos") {
        "mac"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    };
    format!("{os}-{}", std::env::consts::ARCH)
}

#[derive(Debug, Clone)]
pub struct Catalog {
    pub models: Vec<Model>,
}

impl Catalog {
    pub fn parse(json: &str) -> Result<Self, ModelError> {
        let raw: RawCatalog =
            serde_json::from_str(json).map_err(|e| ModelError::Catalog(e.to_string()))?;
        let mut models = Vec::new();
        for m in raw.models {
            let names: Vec<String> = if m.files.is_empty() {
                vec![m.url.rsplit('/').next().unwrap_or("model.bin").to_string()]
            } else {
                m.files.clone()
            };
            let mut files = Vec::new();
            for name in names {
                let sha = match &m.sha256 {
                    PerFile::One(s) => s.clone(),
                    PerFile::Many(map) => map.get(&name).cloned().unwrap_or_default(),
                };
                let bytes = match &m.bytes {
                    PerFile::One(b) => *b,
                    PerFile::Many(map) => map.get(&name).copied().unwrap_or(0),
                };
                if sha.len() != 64 || !sha.chars().all(|c| c.is_ascii_hexdigit()) || bytes == 0 {
                    return Err(ModelError::Catalog(format!(
                        "{}: {name} isn't pinned",
                        m.id
                    )));
                }
                let url = if m.files.is_empty() {
                    m.url.clone()
                } else {
                    format!("{}{}", m.url, name)
                };
                files.push(ModelFile {
                    name,
                    url,
                    sha256: sha.to_lowercase(),
                    bytes,
                });
            }
            models.push(Model {
                id: m.id,
                name: m.name,
                runtime: m.runtime,
                files,
                languages: m.languages,
                langs_label: m.langs_label,
                recommended: m.recommended,
                exclude: m.exclude,
                default_on: m.default_on,
                warn_cpu: m.warn_cpu,
                license: m.license,
            });
        }
        Ok(Self { models })
    }

    /// The catalog compiled into the app.
    pub fn bundled() -> Self {
        Self::parse(CATALOG_JSON).unwrap_or(Self { models: Vec::new() })
    }

    pub fn get(&self, id: &str) -> Option<&Model> {
        self.models.iter().find(|m| m.id == id)
    }

    /// The recommended model on a platform.
    pub fn recommended(&self, platform: &str) -> Option<&Model> {
        self.models
            .iter()
            .find(|m| m.recommended_on(platform))
            .or_else(|| self.models.iter().find(|m| m.available_on(platform)))
    }

    /// Downloaded models from best to worst: the recommended one, then
    /// catalog order (Parakeet, Turbo, Base, Distil).
    pub fn best_installed<'a>(&'a self, platform: &str, installed: &[String]) -> Option<&'a Model> {
        let mut ranked: Vec<&Model> = self
            .models
            .iter()
            .filter(|m| m.available_on(platform) && installed.contains(&m.id))
            .collect();
        ranked.sort_by_key(|m| !m.recommended_on(platform));
        ranked.first().copied()
    }

    /// Replaces each URL's origin with `base` (`<base>/<id>/<file>`), for the
    /// local test server.
    pub fn with_base(mut self, base: &str) -> Self {
        let base = base.trim_end_matches('/');
        for m in &mut self.models {
            for f in &mut m.files {
                f.url = format!("{base}/{}/{}", m.id, f.name);
            }
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    #[error("the model catalog is invalid: {0}")]
    Catalog(String),
    #[error("Unknown model")]
    Unknown,
    #[error("Download failed: the file didn't match.")]
    Mismatch { file: String },
    #[error("Download failed: {0}")]
    Http(String),
    #[error("Download failed: {0}")]
    Io(String),
    #[error("A model folder can't contain links")]
    Symlink,
    #[error("Download paused")]
    Cancelled,
    #[error("Not available on this computer")]
    Unavailable,
}

impl From<std::io::Error> for ModelError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

/// The models folder.
#[derive(Debug, Clone)]
pub struct ModelStore {
    root: PathBuf,
}

const VERIFIED: &str = ".verified";

impl ModelStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    /// The file a runtime loads (whisper: the one `.bin`; ONNX: the folder).
    pub fn load_path(&self, model: &Model) -> PathBuf {
        match model.runtime {
            Runtime::Whisper => self.dir(&model.id).join(&model.files[0].name),
            Runtime::Onnx => self.dir(&model.id),
        }
    }

    /// Downloaded and verified: every file is there at its size, and the
    /// marker written after the SHA-256 check names the same hashes.
    pub fn is_installed(&self, model: &Model) -> bool {
        let dir = self.dir(&model.id);
        if check_no_links(&self.root, &dir).is_err() {
            return false;
        }
        let Ok(marker) = std::fs::read_to_string(dir.join(VERIFIED)) else {
            return false;
        };
        model.files.iter().all(|f| {
            marker
                .lines()
                .any(|l| l == format!("{}  {}", f.sha256, f.name))
                && std::fs::metadata(dir.join(&f.name))
                    .map(|m| m.is_file() && m.len() == f.bytes)
                    .unwrap_or(false)
        })
    }

    pub fn installed(&self, catalog: &Catalog) -> Vec<String> {
        catalog
            .models
            .iter()
            .filter(|m| self.is_installed(m))
            .map(|m| m.id.clone())
            .collect()
    }

    /// Bytes downloaded so far (partials included).
    pub fn partial_bytes(&self, model: &Model) -> u64 {
        let dir = self.dir(&model.id);
        model
            .files
            .iter()
            .map(|f| {
                let done = std::fs::metadata(dir.join(&f.name)).map(|m| m.len()).ok();
                let part = std::fs::metadata(dir.join(format!("{}.partial", f.name)))
                    .map(|m| m.len())
                    .ok();
                done.or(part).unwrap_or(0)
            })
            .sum()
    }

    pub fn remove(&self, id: &str) -> Result<(), ModelError> {
        let dir = self.dir(id);
        match std::fs::symlink_metadata(&dir) {
            Ok(m) if m.file_type().is_symlink() => std::fs::remove_file(&dir)?,
            Ok(_) => std::fs::remove_dir_all(&dir)?,
            Err(_) => {}
        }
        Ok(())
    }

    /// Bytes used by every installed or partial model.
    pub fn used_bytes(&self) -> u64 {
        fn walk(p: &Path) -> u64 {
            let Ok(rd) = std::fs::read_dir(p) else {
                return 0;
            };
            rd.flatten()
                .map(|e| match e.file_type() {
                    Ok(t) if t.is_dir() => walk(&e.path()),
                    Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
                    _ => 0,
                })
                .sum()
        }
        walk(&self.root)
    }
}

/// Refuses a model folder (or anything in it) that is a symlink, so a
/// download can't be redirected outside `$DATA/models`.
fn check_no_links(root: &Path, dir: &Path) -> Result<(), ModelError> {
    for p in [root, dir] {
        if let Ok(m) = std::fs::symlink_metadata(p) {
            if m.file_type().is_symlink() {
                return Err(ModelError::Symlink);
            }
        }
    }
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
                return Err(ModelError::Symlink);
            }
        }
    }
    Ok(())
}

/// Download progress for the model manager.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "stage", rename_all = "snake_case")]
#[ts(export)]
pub enum DownloadProgress {
    Downloading {
        #[ts(type = "number")]
        done: u64,
        #[ts(type = "number")]
        total: u64,
        /// Bytes a second over the last few seconds.
        #[ts(type = "number")]
        speed: u64,
    },
    /// Waiting out the server's `Retry-After`.
    Waiting {
        #[ts(type = "number")]
        seconds: u64,
    },
    Verifying,
}

pub type ProgressSink = Arc<dyn Fn(DownloadProgress) + Send + Sync>;

pub struct Downloader {
    client: reqwest::Client,
    /// The longest `Retry-After` honoured before giving up.
    pub max_wait: Duration,
    pub max_attempts: u32,
}

impl Downloader {
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            max_wait: Duration::from_secs(300),
            max_attempts: 6,
        }
    }

    /// Downloads, verifies and installs every file of `model`.
    pub async fn download(
        &self,
        store: &ModelStore,
        model: &Model,
        progress: ProgressSink,
        cancel: CancellationToken,
    ) -> Result<(), ModelError> {
        let dir = store.dir(&model.id);
        std::fs::create_dir_all(&store.root)?;
        check_no_links(&store.root, &dir)?;
        std::fs::create_dir_all(&dir)?;
        check_no_links(&store.root, &dir)?;
        let total = model.bytes();
        let mut base = 0u64;
        let mut meter = Meter::new();
        for f in &model.files {
            let final_path = dir.join(&f.name);
            let ok_already = std::fs::metadata(&final_path)
                .map(|m| m.len() == f.bytes)
                .unwrap_or(false);
            if !ok_already {
                self.fetch(&dir, f, base, total, &progress, &cancel, &mut meter)
                    .await?;
            }
            base += f.bytes;
        }
        progress(DownloadProgress::Verifying);
        let mut marker = String::new();
        for f in &model.files {
            let path = dir.join(&f.name);
            let file = f.clone();
            let p = path.clone();
            let ok = tokio::task::spawn_blocking(move || verify(&p, &file))
                .await
                .map_err(|e| ModelError::Io(e.to_string()))??;
            if !ok {
                let _ = std::fs::remove_file(&path);
                return Err(ModelError::Mismatch {
                    file: f.name.clone(),
                });
            }
            marker.push_str(&format!("{}  {}\n", f.sha256, f.name));
        }
        tandem_core::atomic::atomic_write(&dir.join(VERIFIED), marker.as_bytes(), None)
            .map_err(|e| ModelError::Io(e.to_string()))?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn fetch(
        &self,
        dir: &Path,
        f: &ModelFile,
        base: u64,
        total: u64,
        progress: &ProgressSink,
        cancel: &CancellationToken,
        meter: &mut Meter,
    ) -> Result<(), ModelError> {
        let partial = dir.join(format!("{}.partial", f.name));
        let mut attempt = 0;
        loop {
            attempt += 1;
            if cancel.is_cancelled() {
                return Err(ModelError::Cancelled);
            }
            let have = std::fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);
            if have > f.bytes {
                std::fs::remove_file(&partial)?;
                continue;
            }
            if have == f.bytes {
                break;
            }
            let mut req = self.client.get(&f.url);
            if have > 0 {
                req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
            }
            let resp = tokio::select! {
                r = req.send() => r.map_err(|e| ModelError::Http(e.to_string()))?,
                _ = cancel.cancelled() => return Err(ModelError::Cancelled),
            };
            let status = resp.status();
            if status.as_u16() == 429 || status.as_u16() == 503 {
                let wait = retry_after(resp.headers())
                    .unwrap_or(Duration::from_secs(2u64.pow(attempt.min(6))));
                if attempt >= self.max_attempts || wait > self.max_wait {
                    return Err(ModelError::Http(format!("the server is busy ({status})")));
                }
                progress(DownloadProgress::Waiting {
                    seconds: wait.as_secs(),
                });
                tokio::select! {
                    _ = tokio::time::sleep(wait) => continue,
                    _ = cancel.cancelled() => return Err(ModelError::Cancelled),
                }
            }
            let append = match status.as_u16() {
                206 if have > 0 => true,
                200 => false,
                416 => {
                    // The partial is complete or bogus: start over.
                    std::fs::remove_file(&partial)?;
                    continue;
                }
                _ => return Err(ModelError::Http(format!("HTTP {status}"))),
            };
            let mut out = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .append(append)
                .truncate(!append)
                .open(&partial)?;
            let mut done = if append { have } else { 0 };
            let mut stream = resp.bytes_stream();
            loop {
                let next = tokio::select! {
                    n = stream.next() => n,
                    _ = cancel.cancelled() => {
                        out.flush()?;
                        return Err(ModelError::Cancelled);
                    }
                };
                let Some(chunk) = next else { break };
                let chunk = chunk.map_err(|e| ModelError::Http(e.to_string()))?;
                done += chunk.len() as u64;
                if done > f.bytes {
                    drop(out);
                    let _ = std::fs::remove_file(&partial);
                    return Err(ModelError::Mismatch {
                        file: f.name.clone(),
                    });
                }
                out.write_all(&chunk)?;
                let speed = meter.add(chunk.len() as u64);
                progress(DownloadProgress::Downloading {
                    done: base + done,
                    total,
                    speed,
                });
            }
            out.flush()?;
            drop(out);
            if done == f.bytes {
                break;
            }
            if attempt >= self.max_attempts {
                return Err(ModelError::Http("the download kept stopping".into()));
            }
        }
        std::fs::rename(&partial, dir.join(&f.name))?;
        Ok(())
    }
}

fn retry_after(h: &reqwest::header::HeaderMap) -> Option<Duration> {
    let v = h.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    v.trim().parse::<u64>().ok().map(Duration::from_secs)
}

/// Size and SHA-256 of a finished file.
fn verify(path: &Path, f: &ModelFile) -> Result<bool, ModelError> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(ModelError::Symlink);
    }
    if meta.len() != f.bytes {
        return Ok(false);
    }
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()) == f.sha256)
}

struct Meter {
    window: Vec<(Instant, u64)>,
}

impl Meter {
    fn new() -> Self {
        Self { window: Vec::new() }
    }

    fn add(&mut self, n: u64) -> u64 {
        let now = Instant::now();
        self.window.push((now, n));
        self.window
            .retain(|(t, _)| now.duration_since(*t) < Duration::from_secs(3));
        let span = self
            .window
            .first()
            .map(|(t, _)| now.duration_since(*t).as_secs_f64())
            .unwrap_or(0.0)
            .max(0.25);
        (self.window.iter().map(|(_, n)| *n).sum::<u64>() as f64 / span) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn f_di_04_the_bundled_catalog_is_pinned() {
        let c = Catalog::parse(CATALOG_JSON).expect("pinned catalog");
        assert_eq!(c.models.len(), 4);
        let base = c.get("whisper-base.en").expect("base");
        assert_eq!(base.files[0].bytes, 147_964_211);
        assert!(base.files[0].url.ends_with("ggml-base.en.bin"));
        let p = c.get("parakeet-tdt-0.6b-v3-int8").expect("parakeet");
        assert_eq!(p.files.len(), 4);
        assert!(p.files[3].url.ends_with("/vocab.txt"));
        assert!(p.supports_language("de"));
        assert!(!p.supports_language("ja"));
        assert!(c
            .get("whisper-large-v3-turbo-q5_0")
            .expect("t")
            .supports_language("ja"));
        assert!(Catalog::parse(&CATALOG_JSON.replacen("6139d2fa", "TBD", 1)).is_err());
    }

    #[test]
    fn f_di_04_platform_rules() {
        let c = Catalog::bundled();
        assert_eq!(
            c.recommended("mac-aarch64").map(|m| m.id.as_str()),
            Some("parakeet-tdt-0.6b-v3-int8")
        );
        assert_eq!(
            c.recommended("mac-x86_64").map(|m| m.id.as_str()),
            Some("whisper-large-v3-turbo-q5_0")
        );
        let p = c.get("parakeet-tdt-0.6b-v3-int8").expect("p");
        assert!(!p.available_on("mac-x86_64"));
        let d = c.get("distil-whisper-large-v3.5").expect("d");
        assert!(d.cpu_warning_on("linux-x86_64"));
        assert!(d.cpu_warning_on("windows-x86_64"));
        assert!(!d.cpu_warning_on("mac-aarch64"));
        let installed = vec![
            "whisper-base.en".to_string(),
            "distil-whisper-large-v3.5".to_string(),
        ];
        assert_eq!(
            c.best_installed("linux-x86_64", &installed)
                .map(|m| m.id.as_str()),
            Some("whisper-base.en")
        );
    }

    /// A tiny catalog served by a local server that can corrupt a byte, rate
    /// limit, or cut the connection.
    struct Server {
        base: String,
        hits: Arc<AtomicUsize>,
        _task: tokio::task::JoinHandle<()>,
    }

    #[derive(Clone)]
    struct Behaviour {
        body: Arc<Vec<u8>>,
        corrupt_at: Option<usize>,
        busy_first: usize,
        cut_first_at: Option<usize>,
        hits: Arc<AtomicUsize>,
        ranges: Arc<parking_lot::Mutex<Vec<String>>>,
    }

    async fn serve(b: Behaviour) -> Server {
        use axum::extract::State;
        use axum::http::{HeaderMap, StatusCode};
        use axum::response::IntoResponse;
        async fn handler(
            State(b): State<Behaviour>,
            headers: HeaderMap,
        ) -> axum::response::Response {
            let n = b.hits.fetch_add(1, Ordering::SeqCst);
            if n < b.busy_first {
                return (StatusCode::TOO_MANY_REQUESTS, [("retry-after", "1")], "").into_response();
            }
            let mut body = (*b.body).clone();
            if let Some(i) = b.corrupt_at {
                body[i] ^= 0xff;
            }
            let range = headers
                .get("range")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            if let Some(r) = &range {
                b.ranges.lock().push(r.clone());
                let start: usize = r
                    .trim_start_matches("bytes=")
                    .trim_end_matches('-')
                    .parse()
                    .unwrap_or(0);
                let part = body[start..].to_vec();
                return (StatusCode::PARTIAL_CONTENT, part).into_response();
            }
            if let Some(cut) = b.cut_first_at {
                if n == b.busy_first {
                    // Declares the full length but sends only part of it.
                    body.truncate(cut);
                    return (StatusCode::OK, body).into_response();
                }
            }
            (StatusCode::OK, body).into_response()
        }
        let hits = b.hits.clone();
        let app = axum::Router::new()
            .route("/{id}/{file}", axum::routing::get(handler))
            .with_state(b);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Server {
            base: format!("http://{addr}"),
            hits,
            _task: task,
        }
    }

    fn tiny(body: &[u8]) -> Catalog {
        let sha = hex::encode(Sha256::digest(body));
        let json = format!(
            r#"{{"models":[{{"id":"tiny","name":"Tiny","runtime":"whisper","url":"https://example.invalid/tiny.bin","sha256":"{sha}","bytes":{},"languages":["en"],"langs_label":"English","license":"MIT"}}]}}"#,
            body.len()
        );
        Catalog::parse(&json).expect("tiny")
    }

    fn behaviour(body: &[u8]) -> Behaviour {
        Behaviour {
            body: Arc::new(body.to_vec()),
            corrupt_at: None,
            busy_first: 0,
            cut_first_at: None,
            hits: Arc::new(AtomicUsize::new(0)),
            ranges: Arc::new(parking_lot::Mutex::new(Vec::new())),
        }
    }

    fn sink() -> (ProgressSink, Arc<parking_lot::Mutex<Vec<DownloadProgress>>>) {
        let seen = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let s = seen.clone();
        (Arc::new(move |p| s.lock().push(p)), seen)
    }

    fn body() -> Vec<u8> {
        (0..5000u32).map(|i| (i % 251) as u8).collect()
    }

    #[tokio::test]
    async fn f_di_04_downloads_verifies_and_installs() {
        let body = body();
        let server = serve(behaviour(&body)).await;
        let catalog = tiny(&body).with_base(&server.base);
        let dir = tempfile::tempdir().expect("dir");
        let store = ModelStore::new(dir.path().join("models"));
        let model = catalog.get("tiny").expect("tiny");
        let (progress, seen) = sink();
        Downloader::new(reqwest::Client::new())
            .download(&store, model, progress, CancellationToken::new())
            .await
            .expect("download");
        assert!(store.is_installed(model));
        assert_eq!(store.installed(&catalog), vec!["tiny".to_string()]);
        assert!(seen.lock().contains(&DownloadProgress::Verifying));
        assert!(seen.lock().iter().any(|p| matches!(
            p,
            DownloadProgress::Downloading {
                done: 5000,
                total: 5000,
                ..
            }
        )));
        store.remove("tiny").expect("remove");
        assert!(!store.is_installed(model));
    }

    #[tokio::test]
    async fn f_di_04_1_a_corrupted_byte_fails_verification_and_deletes_the_file() {
        let body = body();
        let mut b = behaviour(&body);
        b.corrupt_at = Some(1000);
        let server = serve(b).await;
        let catalog = tiny(&body).with_base(&server.base);
        let dir = tempfile::tempdir().expect("dir");
        let store = ModelStore::new(dir.path().join("models"));
        let model = catalog.get("tiny").expect("tiny");
        let (progress, _) = sink();
        let err = Downloader::new(reqwest::Client::new())
            .download(&store, model, progress, CancellationToken::new())
            .await
            .expect_err("mismatch");
        assert_eq!(
            err,
            ModelError::Mismatch {
                file: "tiny.bin".into()
            }
        );
        assert_eq!(err.to_string(), "Download failed: the file didn't match.");
        assert!(!store.dir("tiny").join("tiny.bin").exists());
        assert!(!store.dir("tiny").join("tiny.bin.partial").exists());
        assert!(!store.is_installed(model));
    }

    #[tokio::test]
    async fn f_di_04_resumes_with_a_range_and_honours_retry_after() {
        let body = body();
        let mut b = behaviour(&body);
        b.busy_first = 1;
        b.cut_first_at = Some(2000);
        let ranges = b.ranges.clone();
        let server = serve(b).await;
        let catalog = tiny(&body).with_base(&server.base);
        let dir = tempfile::tempdir().expect("dir");
        let store = ModelStore::new(dir.path().join("models"));
        let model = catalog.get("tiny").expect("tiny");
        let (progress, seen) = sink();
        let started = Instant::now();
        Downloader::new(reqwest::Client::new())
            .download(&store, model, progress, CancellationToken::new())
            .await
            .expect("download");
        assert!(
            started.elapsed() >= Duration::from_millis(900),
            "waited for Retry-After"
        );
        assert!(seen
            .lock()
            .contains(&DownloadProgress::Waiting { seconds: 1 }));
        assert_eq!(*ranges.lock(), vec!["bytes=2000-".to_string()]);
        assert!(store.is_installed(model));
        assert_eq!(server.hits.load(Ordering::SeqCst), 3);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn f_di_04_symlinked_model_folders_are_refused() {
        let body = body();
        let server = serve(behaviour(&body)).await;
        let catalog = tiny(&body).with_base(&server.base);
        let dir = tempfile::tempdir().expect("dir");
        let elsewhere = tempfile::tempdir().expect("elsewhere");
        let root = dir.path().join("models");
        std::fs::create_dir_all(&root).expect("mkdir");
        std::os::unix::fs::symlink(elsewhere.path(), root.join("tiny")).expect("link");
        let store = ModelStore::new(root);
        let (progress, _) = sink();
        let err = Downloader::new(reqwest::Client::new())
            .download(
                &store,
                catalog.get("tiny").expect("t"),
                progress,
                CancellationToken::new(),
            )
            .await
            .expect_err("refused");
        assert_eq!(err, ModelError::Symlink);
        assert_eq!(std::fs::read_dir(elsewhere.path()).expect("rd").count(), 0);
    }

    #[tokio::test]
    async fn f_di_04_cancel_keeps_the_partial_for_resume() {
        let token = CancellationToken::new();
        token.cancel();
        let body = body();
        let server = serve(behaviour(&body)).await;
        let catalog = tiny(&body).with_base(&server.base);
        let dir = tempfile::tempdir().expect("dir");
        let store = ModelStore::new(dir.path().join("models"));
        let (progress, _) = sink();
        let err = Downloader::new(reqwest::Client::new())
            .download(&store, catalog.get("tiny").expect("t"), progress, token)
            .await
            .expect_err("cancelled");
        assert_eq!(err, ModelError::Cancelled);
    }
}
