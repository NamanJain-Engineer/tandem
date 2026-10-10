//! Dictate in the app (F-DI-*; ARCHITECTURE §12): wires the voice crate's
//! controller to settings, the store, the model files, the widget window
//! and the webview. `new` has no side effects; `start` opens the mic, the
//! hotkeys and the model, so unit tests and `AppState::for_tests` stay quiet.
//! Under the E2E harness the microphone, the hotkeys and the insertion
//! backend are fakes the `test_dictate_*` commands drive.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use tandem_core::ids::TakeId;
use tandem_core::settings::SettingKey;
use tandem_core::{DataDir, ErrorCode, IpcError};
use tandem_store::repo::{dictate as repo, settings};
use tandem_store::Store;
use tandem_voice::capture::{
    AudioBackend, AudioService, CaptureConfig, CaptureEvent, FakeAudioBackend, FakeSource,
};
use tandem_voice::chimes::{ChimePlayer, SilentPlayer};
use tandem_voice::cloud::EngineChoice;
use tandem_voice::devices::{self, DeviceInfo, MicPrefs};
use tandem_voice::dictate::{
    Dictate, DictateConfig, DictateDeps, DictatePhase, DictateSink, DictateStateEvent,
    DictateToast, FinishedTake, ToastCode,
};
use tandem_voice::dictionary::{Dictionary, Rule, RuleKind, Spacing};
use tandem_voice::hotkey::{
    Binding, Bindings, FakeHotkeys, HotkeyBackend, HotkeyStatus, Os, Phase,
};
use tandem_voice::insert::fake::FakePlatform;
use tandem_voice::insert::{MissingTools, Output, Platform};
use tandem_voice::media::PauseMedia;
use tandem_voice::models::{Catalog, DownloadProgress, Downloader, ModelError, ModelStore};
use tandem_voice::stt::{self, EngineSlot, Switch};
use tandem_voice::take::{Origin, TakeState};

/// Where an event goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum To {
    Main,
    /// `main` and `dictate-widget`.
    Both,
}

pub type Emitter = Arc<dyn Fn(&str, Value, To) + Send + Sync>;

/// The widget window (F-DI-07), behind a trait so the visibility rules are
/// tested without a window.
pub trait WidgetHost: Send + Sync {
    fn show(&self);
    fn hide(&self);
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct KeyLabel {
    /// Canonical, as stored (`Ctrl+Alt`).
    pub binding: String,
    /// As the OS names it (`Ctrl+Win`, `Control+Option`).
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ModelStage {
    None,
    Verifying,
    Loading,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModelRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModelFallback {
    pub using: String,
    pub missing: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DictateStatus {
    pub enabled: bool,
    pub hotkey: HotkeyStatus,
    pub verified: bool,
    pub hold: KeyLabel,
    pub toggle: Option<KeyLabel>,
    pub latch: bool,
    /// `x11`, `wayland`, `mac`, `windows`, `none`.
    pub session: String,
    /// Shown when no global shortcut can be registered (Wayland).
    pub deep_link_command: Option<String>,
    pub mic: Option<DeviceInfo>,
    pub engine: String,
    pub model: Option<ModelRef>,
    pub model_stage: ModelStage,
    pub fallback: Option<ModelFallback>,
    pub state: tandem_voice::take::TakeState,
    pub missing_tools: Option<MissingTools>,
    /// "Pause other audio" is offered on this OS.
    pub can_pause_media: bool,
    /// Windows doesn't restore the clipboard after a paste.
    pub restores_clipboard: bool,
    /// For the widget, which can't read settings: the app theme, its style
    /// and whether its position is locked.
    pub theme: String,
    pub widget_style: String,
    pub lock_position: bool,
    /// Show Enhance and Polish after a take (the setting, on a paid plan).
    pub quick_actions: bool,
    /// F-DI-10's one-time disclosure was shown.
    pub enhance_disclosed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ModelState {
    Absent,
    Queued,
    Downloading,
    Verifying,
    Loading,
    Downloaded,
    Active,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModelView {
    pub id: String,
    pub name: String,
    #[ts(type = "number")]
    pub bytes: u64,
    pub langs_label: String,
    pub languages: Vec<String>,
    pub recommended: bool,
    pub available: bool,
    pub cpu_warning: bool,
    /// This build can run the model's runtime.
    pub runnable: bool,
    pub state: ModelState,
    pub progress: Option<DownloadProgress>,
    pub error: Option<String>,
    pub license: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModelsView {
    pub models: Vec<ModelView>,
    #[ts(type = "number")]
    pub used_bytes: u64,
    pub active: Option<String>,
    /// "Switches after this take."
    pub pending: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TakeView {
    pub id: String,
    #[ts(type = "number")]
    pub created_at: i64,
    pub text: String,
    pub engine: String,
    pub model: String,
    pub word_count: u32,
    #[ts(type = "number")]
    pub duration_ms: u64,
    pub app_id: Option<String>,
}

impl From<repo::TakeRow> for TakeView {
    fn from(r: repo::TakeRow) -> Self {
        Self {
            id: r.id,
            created_at: r.created_at,
            text: r.text,
            engine: r.engine,
            model: r.model,
            word_count: r.word_count,
            duration_ms: r.duration_ms,
            app_id: r.app_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HistoryCursor {
    #[ts(type = "number")]
    pub created_at: i64,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TakePage {
    pub takes: Vec<TakeView>,
    pub next: Option<HistoryCursor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DictateStats {
    #[ts(type = "number")]
    pub words: u64,
    #[ts(type = "number")]
    pub speaking_ms: u64,
    #[ts(type = "number")]
    pub takes: u64,
    /// Words per minute of speaking time.
    pub wpm: u32,
    /// Typing the same words at 40 wpm minus the speaking time.
    #[ts(type = "number")]
    pub saved_ms: u64,
}

impl DictateStats {
    pub fn from_totals(t: repo::Totals) -> Self {
        let minutes = t.speaking_ms as f64 / 60_000.0;
        let wpm = if minutes > 0.0 {
            (t.words as f64 / minutes).round() as u32
        } else {
            0
        };
        let typing_ms = (t.words as f64 / 40.0 * 60_000.0) as u64;
        Self {
            words: t.words,
            speaking_ms: t.speaking_ms,
            takes: t.takes,
            wpm,
            saved_ms: typing_ms.saturating_sub(t.speaking_ms),
        }
    }
}

/// The history's CSV header (F-DI-08 AC1).
pub const CSV_HEADER: &str = "timestamp,text,word_count,duration_seconds,model";

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn takes_csv(rows: &[repo::TakeRow]) -> String {
    let mut out = String::from(CSV_HEADER);
    out.push('\n');
    for r in rows {
        let ts = chrono::DateTime::from_timestamp_millis(r.created_at)
            .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
            .unwrap_or_default();
        out.push_str(&format!(
            "{},{},{},{:.1},{}\n",
            ts,
            csv_field(&r.text),
            r.word_count,
            r.duration_ms as f64 / 1000.0,
            csv_field(&r.model)
        ));
    }
    out
}

// ── Rules ⇄ rows ────────────────────────────────────────────────────────

pub fn rule_from_row(r: repo::RuleRow) -> Rule {
    Rule {
        id: r.id,
        kind: if r.kind == "word" {
            RuleKind::Word
        } else {
            RuleKind::Correction
        },
        replacement: r.replacement,
        phrases: r.phrases,
        match_case: r.match_case,
        protect: r.protect,
        spacing: if r.spacing == "flexible" {
            Spacing::Flexible
        } else {
            Spacing::Exact
        },
        enabled: r.enabled,
        lang: r.lang,
        app_profiles: r.app_profiles,
        updated_at: r.updated_at,
        deleted_at: r.deleted_at,
    }
}

pub fn row_from_rule(r: &Rule, sync_rev: i64) -> repo::RuleRow {
    repo::RuleRow {
        id: r.id.clone(),
        kind: match r.kind {
            RuleKind::Word => "word".into(),
            RuleKind::Correction => "correction".into(),
        },
        replacement: r.replacement.clone(),
        phrases: r.phrases.clone(),
        match_case: r.match_case,
        protect: r.protect,
        spacing: match r.spacing {
            Spacing::Exact => "exact".into(),
            Spacing::Flexible => "flexible".into(),
        },
        enabled: r.enabled,
        lang: r.lang.clone(),
        app_profiles: r.app_profiles.clone(),
        updated_at: r.updated_at,
        deleted_at: r.deleted_at,
        sync_rev,
    }
}

/// Trims and checks a rule from the UI or an import.
pub fn clean_rule(mut r: Rule) -> Result<Rule, IpcError> {
    r.replacement = r.replacement.trim().to_string();
    r.phrases = r
        .phrases
        .iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    r.phrases.dedup();
    r.app_profiles = r
        .app_profiles
        .iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if r.lang.trim().is_empty() {
        r.lang = "*".into();
    }
    let invalid = |m: &str| Err(IpcError::new(ErrorCode::Invalid, m));
    if r.replacement.is_empty() || r.replacement.len() > 200 {
        return invalid("Enter what to write (up to 200 characters)");
    }
    if r.kind == RuleKind::Correction && r.phrases.is_empty() {
        return invalid("Enter at least one phrase to replace");
    }
    if r.phrases.len() > 50 || r.phrases.iter().any(|p| p.len() > 200) {
        return invalid("Phrases are limited to 50, of up to 200 characters each");
    }
    if r.id.trim().is_empty() {
        r.id = format!("rule_{}", ulid::Ulid::new().to_string().to_lowercase());
    }
    Ok(r)
}

/// The correction rule "Remember this correction" makes: the words that
/// changed between what was dictated and what the user wrote.
pub fn correction_from(original: &str, corrected: &str) -> Option<(String, String)> {
    let a: Vec<&str> = original.split_whitespace().collect();
    let b: Vec<&str> = corrected.split_whitespace().collect();
    let norm = |w: &str| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
    let mut start = 0;
    while start < a.len() && start < b.len() && norm(a[start]) == norm(b[start]) {
        start += 1;
    }
    let (mut ea, mut eb) = (a.len(), b.len());
    while ea > start && eb > start && norm(a[ea - 1]) == norm(b[eb - 1]) {
        ea -= 1;
        eb -= 1;
    }
    let from: Vec<String> = a[start..ea].iter().map(|w| norm(w)).collect();
    let to: Vec<String> = b[start..eb].iter().map(|w| norm(w)).collect();
    let (from, to) = (from.join(" "), to.join(" "));
    (!from.trim().is_empty() && !to.trim().is_empty() && from != to).then_some((from, to))
}

/// Locally mined Review suggestions (F-DI-09): proper nouns and acronyms
/// that keep coming back, and words spelled more than one way.
pub fn mine_suggestions(
    takes: &[repo::TakeRow],
    known: &[String],
) -> Vec<(String, String, Vec<String>)> {
    let known: std::collections::HashSet<String> = known.iter().map(|k| k.to_lowercase()).collect();
    let mut seen: HashMap<String, (HashMap<String, usize>, Vec<String>)> = HashMap::new();
    for t in takes {
        let words: Vec<&str> = t.text.split_whitespace().collect();
        for (i, raw) in words.iter().enumerate() {
            let w = raw.trim_matches(|c: char| !c.is_alphanumeric());
            if w.len() < 2 || w.chars().any(|c| c.is_ascii_digit()) {
                continue;
            }
            let entry = seen.entry(w.to_lowercase()).or_default();
            *entry.0.entry(w.to_string()).or_default() += 1;
            if entry.1.len() < 3 {
                let lo = i.saturating_sub(3);
                let hi = (i + 4).min(words.len());
                entry.1.push(words[lo..hi].join(" "));
            }
        }
    }
    const COMMON: &[&str] = &["i", "i'm", "i'll", "i've", "i'd", "ok", "okay"];
    let mut out = Vec::new();
    for (lower, (forms, evidence)) in seen {
        if known.contains(&lower) || COMMON.contains(&lower.as_str()) {
            continue;
        }
        let total: usize = forms.values().sum();
        let acronym = forms
            .keys()
            .find(|f| f.len() <= 6 && f.chars().all(|c| c.is_ascii_uppercase()));
        let capitalised = forms
            .iter()
            .filter(|(f, _)| f.chars().next().is_some_and(char::is_uppercase))
            .max_by_key(|(_, n)| **n);
        if forms.len() > 1 && total >= 3 {
            let best = forms
                .iter()
                .max_by_key(|(_, n)| **n)
                .map(|(f, _)| f.clone())
                .unwrap_or_default();
            out.push((best, "spelling".to_string(), evidence));
        } else if let Some(a) = acronym {
            if total >= 2 {
                out.push((a.clone(), "acronym".into(), evidence));
            }
        } else if let Some((f, n)) = capitalised {
            if *n >= 3 {
                out.push((f.clone(), "proper_noun".into(), evidence));
            }
        }
    }
    out.sort();
    out.truncate(50);
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "DictionarySuggestion")]
pub struct Suggestion {
    pub id: String,
    pub term: String,
    /// `spelling`, `unknown`, `acronym`, `proper_noun`.
    pub kind: String,
    pub evidence: Vec<String>,
    #[ts(type = "number")]
    pub last_seen: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportSkip {
    pub line: u32,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportPreview {
    pub preview_id: String,
    pub added: Vec<Rule>,
    pub updated: Vec<Rule>,
    pub skipped: Vec<ImportSkip>,
}

/// The JSON a dictionary exports and imports.
#[derive(Debug, Serialize, Deserialize)]
pub struct DictionaryFile {
    pub version: u32,
    pub rules: Vec<Rule>,
}

pub const RULES_CSV_HEADER: &str =
    "kind,write,phrases,match_case,protect,spacing,language,apps,enabled";

pub fn rules_csv(rules: &[Rule]) -> String {
    let mut out = String::from(RULES_CSV_HEADER);
    out.push('\n');
    for r in rules {
        let kind = if r.kind == RuleKind::Word {
            "word"
        } else {
            "correction"
        };
        let spacing = if r.spacing == Spacing::Flexible {
            "flexible"
        } else {
            "exact"
        };
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            kind,
            csv_field(&r.replacement),
            csv_field(&r.phrases.join("|")),
            r.match_case,
            r.protect,
            spacing,
            csv_field(&r.lang),
            csv_field(&r.app_profiles.join("|")),
            r.enabled
        ));
    }
    out
}

/// Splits one CSV line, honouring quotes.
fn csv_split(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            ('"', _) => quoted = !quoted,
            (',', false) => out.push(std::mem::take(&mut cur)),
            (c, _) => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// Parses an import into rules, with the lines it couldn't use.
pub fn parse_rules(text: &str, csv: bool, now: i64) -> (Vec<Rule>, Vec<ImportSkip>) {
    let mut rules = Vec::new();
    let mut skipped = Vec::new();
    if !csv {
        match serde_json::from_str::<DictionaryFile>(text) {
            Ok(file) => {
                for (i, mut r) in file.rules.into_iter().enumerate() {
                    r.updated_at = now;
                    match clean_rule(r) {
                        Ok(r) => rules.push(r),
                        Err(e) => skipped.push(ImportSkip {
                            line: i as u32 + 1,
                            reason: e.message,
                        }),
                    }
                }
            }
            Err(e) => skipped.push(ImportSkip {
                line: e.line() as u32,
                reason: "This isn't a Vibezeo dictionary file".into(),
            }),
        }
        return (rules, skipped);
    }
    for (i, line) in text.lines().enumerate() {
        if i == 0 && line.trim().starts_with("kind,") || line.trim().is_empty() {
            continue;
        }
        let f = csv_split(line);
        if f.len() < 3 {
            skipped.push(ImportSkip {
                line: i as u32 + 1,
                reason: "Expected at least kind, write and phrases".into(),
            });
            continue;
        }
        let get = |n: usize| f.get(n).map(|s| s.trim().to_string()).unwrap_or_default();
        let truthy = |n: usize, default: bool| match get(n).to_lowercase().as_str() {
            "true" | "yes" | "1" => true,
            "false" | "no" | "0" => false,
            _ => default,
        };
        let rule = Rule {
            id: String::new(),
            kind: if get(0).eq_ignore_ascii_case("word") {
                RuleKind::Word
            } else {
                RuleKind::Correction
            },
            replacement: get(1),
            phrases: get(2).split('|').map(str::to_string).collect(),
            match_case: truthy(3, false),
            protect: truthy(4, true),
            spacing: if get(5).eq_ignore_ascii_case("flexible") {
                Spacing::Flexible
            } else {
                Spacing::Exact
            },
            enabled: truthy(8, true),
            lang: if get(6).is_empty() {
                "*".into()
            } else {
                get(6)
            },
            app_profiles: get(7).split('|').map(str::to_string).collect(),
            updated_at: now,
            deleted_at: None,
        };
        match clean_rule(rule) {
            Ok(r) => rules.push(r),
            Err(e) => skipped.push(ImportSkip {
                line: i as u32 + 1,
                reason: e.message,
            }),
        }
    }
    (rules, skipped)
}

/// The same rule, ignoring id and timestamps (import de-duplication).
fn same_rule_key(r: &Rule) -> (bool, String, Vec<String>) {
    let mut p: Vec<String> = r.phrases.iter().map(|x| x.to_lowercase()).collect();
    p.sort();
    (r.kind == RuleKind::Word, r.replacement.to_lowercase(), p)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FakePlatformView {
    pub calls: Vec<String>,
    pub pasted: Vec<String>,
    pub clipboard: Option<String>,
}

/// The fakes the E2E harness drives.
pub struct Fakes {
    pub audio: Arc<FakeAudioBackend>,
    pub hotkeys: Arc<FakeHotkeys>,
    pub platform: Arc<FakePlatform>,
}

struct Download {
    cancel: CancellationToken,
    progress: Option<DownloadProgress>,
}

#[derive(Default)]
struct Models {
    downloads: HashMap<String, Download>,
    failed: HashMap<String, String>,
    loading: Option<String>,
    stage: Option<ModelStage>,
    fallback: Option<ModelFallback>,
}

struct Shared {
    store: Store,
    emit: OnceLock<Emitter>,
    composer: Mutex<Option<String>>,
    main_focused: AtomicBool,
    rules: Mutex<Vec<Rule>>,
    widget: OnceLock<Arc<dyn WidgetHost>>,
    widget_gen: Arc<std::sync::atomic::AtomicU64>,
    visibility: Mutex<String>,
    /// The current take came from `tandem://dictate/toggle`: the widget shows
    /// even with "Show widget: Hidden" (D-515).
    link_take: AtomicBool,
    /// The widget shows Enhance and Polish after a take, so it stays up long
    /// enough to click them (D-517).
    quick_actions: AtomicBool,
    switch: OnceLock<Arc<dyn Fn(String) + Send + Sync>>,
}

impl Shared {
    fn emit(&self, name: &str, payload: Value, to: To) {
        if let Some(e) = self.emit.get() {
            e(name, payload, to);
        }
    }

    /// Shows or hides the widget for a state (F-DI-07): "While recording"
    /// shows it while a global take runs and hides it 1 s after insertion
    /// (4 s after "Copied", 6 s after an error).
    fn widget_for(&self, e: &DictateStateEvent) {
        let Some(w) = self.widget.get() else { return };
        let mut vis = self.visibility.lock().clone();
        // A take started from outside the app is never invisible (D-515).
        if self.link_take.load(Ordering::Acquire) && e.composer.is_none() {
            if !matches!(
                e.state,
                DictatePhase::Listening
                    | DictatePhase::Transcribing
                    | DictatePhase::Reconnecting
                    | DictatePhase::Enhancing
                    | DictatePhase::Polishing
            ) {
                self.link_take.store(false, Ordering::Release);
            }
            if vis == "hidden" {
                vis = "while_recording".into();
            }
        }
        let gen = self.widget_gen.fetch_add(1, Ordering::AcqRel) + 1;
        if vis == "hidden" || e.composer.is_some() {
            if vis != "always" {
                w.hide();
            }
            return;
        }
        let hide_after = match e.state {
            DictatePhase::Listening
            | DictatePhase::Transcribing
            | DictatePhase::Reconnecting
            | DictatePhase::Enhancing
            | DictatePhase::Polishing => {
                w.show();
                None
            }
            DictatePhase::Inserted if self.quick_actions.load(Ordering::Acquire) => {
                Some(Duration::from_secs(4))
            }
            DictatePhase::Inserted => Some(Duration::from_secs(1)),
            DictatePhase::Copied => Some(Duration::from_secs(4)),
            DictatePhase::Error => {
                w.show();
                Some(Duration::from_secs(6))
            }
            DictatePhase::Idle => Some(Duration::ZERO),
        };
        if vis == "always" {
            w.show();
            return;
        }
        if let Some(delay) = hide_after {
            let w = w.clone();
            let me = self.widget_gen.clone();
            std::thread::spawn(move || {
                std::thread::sleep(delay);
                if me.load(Ordering::Acquire) == gen {
                    w.hide();
                }
            });
        }
    }
}

struct Sink(Arc<Shared>);

impl DictateSink for Sink {
    fn state(&self, e: DictateStateEvent) {
        self.0.widget_for(&e);
        self.0.emit("dictate:state", json!(e), To::Both);
    }

    fn toast(&self, t: DictateToast) {
        self.0.emit("dictate:toast", json!(t), To::Both);
    }

    fn composer_insert(&self, target_id: &str, text: &str) {
        self.0.emit(
            "dictate:insert",
            json!({ "targetId": target_id, "text": text }),
            To::Main,
        );
    }

    fn finished(&self, take: FinishedTake) {
        let store = self.0.store.clone();
        let shared = self.0.clone();
        let row = repo::TakeRow {
            id: take.id.as_str().to_string(),
            created_at: tandem_store::now_ms(),
            text: take.text,
            engine: if take.model == "cloud" {
                "cloud"
            } else {
                "local"
            }
            .into(),
            model: take.model,
            word_count: take.word_count,
            duration_ms: take.duration_ms,
            app_id: take.app_id,
        };
        tauri::async_runtime::spawn(async move {
            if let Err(err) = repo::add_take(&store, row).await {
                tracing::warn!(error = %err, "dictate: take not stored");
                return;
            }
            shared.emit("dictate:history", json!(null), To::Main);
            mine(&store, &shared).await;
        });
    }

    fn rules(&self) -> Vec<Rule> {
        self.0.rules.lock().clone()
    }

    fn composer_focus(&self) -> Option<String> {
        if self.0.main_focused.load(Ordering::Acquire) {
            self.0.composer.lock().clone()
        } else {
            None
        }
    }

    fn key(&self, phase: Phase, native: bool) {
        let phase = match phase {
            Phase::Down => "down",
            Phase::Up => "up",
        };
        self.0.emit(
            "dictate:key",
            json!({ "phase": phase, "native": native }),
            To::Main,
        );
    }

    fn verified(&self) {
        let store = self.0.store.clone();
        let shared = self.0.clone();
        tauri::async_runtime::spawn(async move {
            let _ = settings::set(&store, SettingKey::DictateHotkeyVerified, json!(true)).await;
            shared.emit(
                "settings:changed",
                json!({ "key": "dictate_hotkey_verified", "value": true }),
                To::Main,
            );
            shared.emit("dictate:verified", json!(null), To::Main);
        });
    }

    fn switch_due(&self, model_id: String) {
        if let Some(f) = self.0.switch.get() {
            f(model_id);
        }
    }
}

/// Refreshes the Review tab's suggestions from recent history.
async fn mine(store: &Store, shared: &Shared) {
    let Ok((recent, _)) = repo::history(store, None, None, 200).await else {
        return;
    };
    let mut known: Vec<String> = Vec::new();
    for r in shared.rules.lock().iter() {
        known.push(r.replacement.clone());
        known.extend(r.phrases.iter().cloned());
    }
    let now = tandem_store::now_ms();
    let rows: Vec<repo::SuggestionRow> = mine_suggestions(&recent, &known)
        .into_iter()
        .map(|(term, kind, evidence)| repo::SuggestionRow {
            id: format!("sug_{}", ulid::Ulid::new().to_string().to_lowercase()),
            term,
            kind,
            evidence,
            first_seen: now,
            last_seen: now,
            state: "open".into(),
        })
        .collect();
    if !rows.is_empty() {
        let _ = repo::upsert_suggestions(store, rows).await;
    }
}

pub struct DictateService {
    store: Store,
    data: DataDir,
    e2e: bool,
    shared: Arc<Shared>,
    catalog: Catalog,
    model_store: ModelStore,
    platform_name: String,
    engine: Arc<EngineSlot>,
    backend: Arc<dyn AudioBackend>,
    hotkeys: Arc<dyn HotkeyBackend>,
    platform: Arc<dyn Platform>,
    pub fakes: Option<Fakes>,
    audio: OnceLock<Arc<AudioService>>,
    controller: OnceLock<Dictate>,
    models: Mutex<Models>,
    hotkey_status: Mutex<HotkeyStatus>,
    previews: Mutex<HashMap<String, Vec<Rule>>>,
    started: AtomicBool,
    /// The plan check and the relay call for cloud takes (F-DI-05).
    cloud: OnceLock<(PlanFn, crate::services::dictate_cloud::SttCall)>,
    rewriter: OnceLock<crate::services::dictate_cloud::RewriteCall>,
}

fn clamp_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

pub type PlanFn = Arc<dyn Fn() -> tandem_core::entitlement::Plan + Send + Sync>;

/// Whether `tandem://dictate/toggle` may start or stop a take (D-515): only
/// where it's the documented fallback, a Wayland session or a Linux desktop
/// whose global shortcut is unavailable. Browsers open `tandem://` links (after
/// a prompt the user can tick away), so anywhere else a web page could start a
/// take and have the transcript pasted into its own focused field.
pub fn deep_link_toggle_allowed(session: &str, hotkey_unavailable: bool, linux: bool) -> bool {
    linux && (session == "wayland" || hotkey_unavailable)
}

fn session() -> String {
    if cfg!(target_os = "macos") {
        "mac".into()
    } else if cfg!(windows) {
        "windows".into()
    } else {
        #[cfg(target_os = "linux")]
        {
            match tandem_voice::hotkey::linux::Session::detect() {
                tandem_voice::hotkey::linux::Session::X11 => "x11".into(),
                tandem_voice::hotkey::linux::Session::Wayland => "wayland".into(),
                tandem_voice::hotkey::linux::Session::None => "none".into(),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            "none".into()
        }
    }
}

impl DictateService {
    /// `e2e`: fakes for the mic, the hotkeys and insertion. `models_base`
    /// (debug builds) points downloads at a local server.
    pub fn new(store: Store, data: DataDir, e2e: bool) -> Arc<Self> {
        let mut catalog = Catalog::bundled();
        if cfg!(debug_assertions) {
            if let Ok(base) = std::env::var("TANDEM_MODELS_BASE") {
                catalog = catalog.with_base(&base);
            }
        }
        let (backend, hotkeys, platform, fakes): Backends = if e2e {
            let audio = Arc::new(FakeAudioBackend::new(vec![(
                DeviceInfo::new("fake-mic", "Test Microphone", true),
                FakeSource::Samples {
                    samples: Arc::new(dither(16_000)),
                    rate: 16_000,
                    speed: 1,
                },
            )]));
            let hk = Arc::new(FakeHotkeys::default());
            let pf = Arc::new(FakePlatform::default());
            (
                audio.clone(),
                hk.clone(),
                pf.clone(),
                Some(Fakes {
                    audio,
                    hotkeys: hk,
                    platform: pf,
                }),
            )
        } else {
            (
                native_backend(),
                tandem_voice::hotkey::native(),
                tandem_voice::insert::native(),
                None,
            )
        };
        Arc::new(Self {
            model_store: ModelStore::new(data.models()),
            store: store.clone(),
            data,
            e2e,
            shared: Arc::new(Shared {
                store,
                emit: OnceLock::new(),
                composer: Mutex::new(None),
                main_focused: AtomicBool::new(true),
                rules: Mutex::new(Vec::new()),
                widget: OnceLock::new(),
                widget_gen: Arc::new(std::sync::atomic::AtomicU64::new(0)),
                visibility: Mutex::new("while_recording".into()),
                link_take: AtomicBool::new(false),
                quick_actions: AtomicBool::new(false),
                switch: OnceLock::new(),
            }),
            catalog,
            platform_name: tandem_voice::models::current_platform(),
            engine: Arc::new(EngineSlot::new()),
            backend,
            hotkeys,
            platform,
            fakes,
            audio: OnceLock::new(),
            controller: OnceLock::new(),
            models: Mutex::new(Models::default()),
            hotkey_status: Mutex::new(HotkeyStatus::NotRunning),
            previews: Mutex::new(HashMap::new()),
            started: AtomicBool::new(false),
            cloud: OnceLock::new(),
            rewriter: OnceLock::new(),
        })
    }

    pub fn set_emitter(&self, emit: Emitter) {
        let _ = self.shared.emit.set(emit);
    }

    pub fn set_widget(&self, widget: Arc<dyn WidgetHost>) {
        let _ = self.shared.widget.set(widget);
    }

    fn emit(&self, name: &str, payload: Value, to: To) {
        self.shared.emit(name, payload, to);
    }

    async fn setting<T: serde::de::DeserializeOwned>(&self, key: SettingKey, fallback: T) -> T {
        settings::get(&self.store, key)
            .await
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or(fallback)
    }

    async fn config(&self) -> DictateConfig {
        let lang: String = self
            .setting(SettingKey::DictateLanguage, "auto".to_string())
            .await;
        DictateConfig {
            enabled: self.setting(SettingKey::DictateEnabled, true).await,
            output: Output::parse(
                &self
                    .setting::<String>(SettingKey::DictateOutput, "paste".into())
                    .await,
            ),
            sounds: self.setting(SettingKey::DictateSounds, true).await,
            pause_media: self.setting(SettingKey::DictatePauseMedia, false).await,
            language: (lang != "auto").then_some(lang),
            latch: self.setting(SettingKey::DictateLatch, true).await,
            engine: if self
                .setting::<String>(SettingKey::DictateEngine, "local".into())
                .await
                == "cloud"
            {
                EngineChoice::Cloud
            } else {
                EngineChoice::Local
            },
            share_hints: self.setting(SettingKey::DictateShareHints, false).await,
            auto_enhance: self.setting(SettingKey::DictateAutoEnhance, false).await,
            formatting: self
                .setting::<Option<String>>(SettingKey::DictateFormatting, None)
                .await
                .map(|t| clamp_chars(&t, 2_000))
                .filter(|t| !t.trim().is_empty()),
            formatting_apps: self
                .setting::<serde_json::Map<String, Value>>(
                    SettingKey::DictateFormattingApps,
                    serde_json::Map::new(),
                )
                .await
                .into_iter()
                .filter_map(|(app, v)| {
                    let text = clamp_chars(v.as_str()?, 2_000);
                    (!app.trim().is_empty() && !text.trim().is_empty()).then_some((app, text))
                })
                .collect(),
        }
    }

    /// Gives cloud takes their plan check and relay call (set before `start`).
    pub fn set_cloud(&self, plan: PlanFn, call: crate::services::dictate_cloud::SttCall) {
        let _ = self.cloud.set((plan, call));
    }

    /// Gives Enhance and Polish their relay call (set before `start`; the
    /// plan check is the cloud's).
    pub fn set_rewriter(&self, call: crate::services::dictate_cloud::RewriteCall) {
        let _ = self.rewriter.set(call);
    }

    fn plan(&self) -> tandem_core::entitlement::Plan {
        self.cloud
            .get()
            .map(|(plan, _)| plan())
            .unwrap_or(tandem_core::entitlement::Plan::Free)
    }

    /// Records that F-DI-10's disclosure was shown.
    pub async fn mark_enhance_disclosed(&self) -> Result<(), IpcError> {
        settings::set(
            &self.store,
            SettingKey::DictateEnhanceDisclosed,
            json!(true),
        )
        .await?;
        self.emit(
            "settings:changed",
            json!({ "key": "dictate_enhance_disclosed", "value": true }),
            To::Main,
        );
        self.emit_status().await;
        Ok(())
    }

    /// "Enhance" / "Polish" after a take (F-DI-10).
    pub fn quick_action(&self, mode: tandem_voice::cloud::RewriteMode) -> bool {
        self.controller().is_some_and(|c| c.quick_action(mode))
    }

    /// "Use local for this take" (F-DI-05).
    pub fn use_local(&self) -> bool {
        self.controller().is_some_and(|c| c.use_local())
    }

    async fn capture_config(&self) -> CaptureConfig {
        CaptureConfig {
            prefs: MicPrefs {
                primary: self.setting(SettingKey::DictateMic, None::<String>).await,
                backup: self
                    .setting(SettingKey::DictateBackupMic, None::<String>)
                    .await,
            },
            gain: tandem_voice::audio::Gain::parse(
                &self
                    .setting::<String>(SettingKey::DictateGain, "off".into())
                    .await,
            ),
            silence_timeout: Duration::from_secs(1),
        }
    }

    async fn bindings(&self) -> Bindings {
        let hold: Option<String> = self.setting(SettingKey::DictateHotkey, None).await;
        let toggle: Option<String> = self.setting(SettingKey::DictateToggleHotkey, None).await;
        Bindings {
            hold: Some(
                hold.and_then(|h| h.parse().ok())
                    .unwrap_or_else(|| Binding::default_hold(Os::current())),
            ),
            toggle: toggle.and_then(|t| t.parse().ok()),
        }
    }

    pub fn controller(&self) -> Option<&Dictate> {
        self.controller.get()
    }

    /// `tandem://dictate/toggle` (F-DI-01 "Deep link"): honoured only where
    /// it's the documented fallback (D-515); elsewhere it's logged and ignored.
    /// True when it reached the controller.
    pub fn toggle_from_link(&self) -> bool {
        let unavailable = matches!(*self.hotkey_status.lock(), HotkeyStatus::Unavailable { .. });
        self.toggle_from_link_in(&session(), unavailable, cfg!(target_os = "linux"))
    }

    fn toggle_from_link_in(&self, session: &str, unavailable: bool, linux: bool) -> bool {
        if !deep_link_toggle_allowed(session, unavailable, linux) {
            tracing::warn!(
                code = "dictate.deep_link_refused",
                session,
                "ignored tandem://dictate/toggle: the global shortcut works here"
            );
            return false;
        }
        let Some(c) = self.controller() else {
            return false;
        };
        // Here the link is the documented trigger, not a key the verifier can
        // hear: reaching the controller through it verifies the setup (F-DI-11).
        Sink(self.shared.clone()).verified();
        self.shared.link_take.store(true, Ordering::Release);
        c.toggle();
        // Refused at once (no model, Dictate off) or a stop: nothing to show.
        if matches!(
            c.state(),
            TakeState::Idle | TakeState::Cancelled | TakeState::Done
        ) {
            self.shared.link_take.store(false, Ordering::Release);
        }
        true
    }

    /// Opens everything: called once from the app's setup.
    pub async fn start(self: &Arc<Self>) {
        if self.started.swap(true, Ordering::AcqRel) {
            return;
        }
        // A crash while other audio was muted is undone first (F-DI-03 AC3).
        let media = tandem_voice::media::native().map(|c| {
            Arc::new(PauseMedia::new(
                c,
                self.data.root().join("audio-mute.marker"),
            ))
        });
        if let Some(m) = &media {
            if m.recover() {
                tracing::info!(
                    code = "dictate.media_restored",
                    "restored the output mute state from the last run"
                );
            }
        }
        let shared = self.shared.clone();
        let me = Arc::downgrade(self);
        let events: tandem_voice::capture::EventSink = Arc::new(move |e: CaptureEvent| {
            if let Some(me) = me.upgrade() {
                if let Some(c) = me.controller.get() {
                    c.on_capture_event(e);
                }
            }
        });
        let audio = Arc::new(AudioService::start(
            self.backend.clone(),
            self.capture_config().await,
            events,
        ));
        let _ = self.audio.set(audio.clone());
        let chimes: Arc<dyn ChimePlayer> = if self.e2e {
            Arc::new(SilentPlayer::default())
        } else {
            native_chimes()
        };
        let controller = Dictate::new(
            DictateDeps {
                audio: audio.clone(),
                hotkeys: self.hotkeys.clone(),
                platform: self.platform.clone(),
                engine: self.engine.clone(),
                chimes,
                media: if self.e2e { None } else { media },
                sink: Arc::new(Sink(shared.clone())),
                cloud: self.cloud.get().map(|(plan, call)| {
                    Arc::new(crate::services::dictate_cloud::HostedStt::new(
                        plan.clone(),
                        call.clone(),
                        tokio::runtime::Handle::current(),
                    )) as Arc<dyn tandem_voice::cloud::CloudStt>
                }),
                rewriter: match (self.cloud.get(), self.rewriter.get()) {
                    (Some((plan, _)), Some(call)) => Some(Arc::new(
                        crate::services::dictate_cloud::HostedRewriter::new(
                            plan.clone(),
                            call.clone(),
                            tokio::runtime::Handle::current(),
                        ),
                    )
                        as Arc<dyn tandem_voice::cloud::Rewriter>),
                    _ => None,
                },
            },
            self.config().await,
        );
        let _ = self.controller.set(controller);
        let weak = Arc::downgrade(self);
        let _ = self.shared.switch.set(Arc::new(move |id| {
            if let Some(me) = weak.upgrade() {
                tauri::async_runtime::spawn(async move {
                    me.load_model(&id).await;
                });
            }
        }));
        *self.shared.visibility.lock() = self
            .setting(
                SettingKey::DictateWidgetVisibility,
                "while_recording".to_string(),
            )
            .await;
        self.reload_rules().await;
        let keep: String = self
            .setting(SettingKey::DictateKeepHistory, "forever".into())
            .await;
        self.prune_history(&keep).await;
        let _ = repo::purge(&self.store, tandem_store::now_ms()).await;
        if self.setting(SettingKey::DictateEnabled, true).await {
            audio.warm();
            self.restart_hotkeys().await;
        }
        self.load_selected().await;
        self.level_pump(audio);
        self.emit_status().await;
    }

    /// Re-reads every Dictate setting (called after a `dictate_*` change).
    pub async fn reload_settings(&self, key: SettingKey) {
        let Some(c) = self.controller.get() else {
            return;
        };
        c.set_config(self.config().await);
        if let Some(a) = self.audio.get() {
            a.set_config(self.capture_config().await);
        }
        match key {
            SettingKey::DictateEnabled => {
                if self.setting(SettingKey::DictateEnabled, true).await {
                    if let Some(a) = self.audio.get() {
                        a.warm();
                    }
                    self.restart_hotkeys().await;
                } else {
                    c.cancel();
                    self.hotkeys.stop();
                    *self.hotkey_status.lock() = HotkeyStatus::NotRunning;
                }
            }
            SettingKey::DictateHotkey | SettingKey::DictateToggleHotkey => {
                self.restart_hotkeys().await
            }
            SettingKey::DictateWidgetVisibility => {
                let vis: String = self
                    .setting(
                        SettingKey::DictateWidgetVisibility,
                        "while_recording".into(),
                    )
                    .await;
                if let Some(w) = self.shared.widget.get() {
                    if vis == "always" {
                        w.show();
                    } else {
                        w.hide();
                    }
                }
                *self.shared.visibility.lock() = vis;
            }
            SettingKey::DictateKeepHistory => {
                let keep: String = self
                    .setting(SettingKey::DictateKeepHistory, "forever".into())
                    .await;
                self.prune_history(&keep).await;
            }
            SettingKey::DictateModel => self.load_selected().await,
            _ => {}
        }
        self.emit_status().await;
    }

    async fn prune_history(&self, keep: &str) {
        let days = match keep {
            "7d" => 7,
            "30d" => 30,
            "90d" => 90,
            _ => return,
        };
        let cutoff = tandem_store::now_ms() - days * 24 * 60 * 60 * 1000;
        if let Ok(n) = repo::prune_takes(&self.store, cutoff).await {
            if n > 0 {
                self.emit("dictate:history", json!(null), To::Main);
            }
        }
    }

    async fn restart_hotkeys(&self) {
        let Some(c) = self.controller.get() else {
            return;
        };
        self.hotkeys.stop();
        let status = c.start_hotkeys(&self.bindings().await);
        if status != HotkeyStatus::Active {
            tracing::info!(code = "dictate.hotkey_status", status = ?status, "global shortcut isn't active");
        }
        *self.hotkey_status.lock() = status;
    }

    /// The meter level, ≤30 a second while recording (D-506).
    fn level_pump(&self, audio: Arc<AudioService>) {
        let shared = self.shared.clone();
        std::thread::Builder::new()
            .name("dictate-level".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_millis(33));
                if audio.is_recording() {
                    shared.emit("dictate:level", json!(audio.level()), To::Both);
                }
            })
            .ok();
    }

    pub async fn status(&self) -> DictateStatus {
        let os = Os::current();
        let b = self.bindings().await;
        let hold = b.hold.clone().unwrap_or_else(|| Binding::default_hold(os));
        let devices = self.devices();
        let prefs = self.capture_config().await.prefs;
        let enabled = self.setting(SettingKey::DictateEnabled, true).await;
        let verified = self.setting(SettingKey::DictateHotkeyVerified, false).await;
        let latch = self.setting(SettingKey::DictateLatch, true).await;
        let theme: String = self.setting(SettingKey::Theme, "dark".to_string()).await;
        let widget_style: String = self
            .setting(SettingKey::DictateWidgetStyle, "logo_text".to_string())
            .await;
        let lock_position = self.setting(SettingKey::DictateLockPosition, false).await;
        let quick_actions = self.setting(SettingKey::DictateQuickActions, true).await
            && self.rewriter.get().is_some()
            && self.plan() != tandem_core::entitlement::Plan::Free;
        self.shared
            .quick_actions
            .store(quick_actions, Ordering::Release);
        let mic_id = devices::choose(&devices, &prefs);
        let active = self.engine.current_id();
        let session = session();
        let hotkey = self.hotkey_status.lock().clone();
        let unavailable = matches!(hotkey, HotkeyStatus::Unavailable { .. });
        let (stage, fallback) = {
            let m = self.models.lock();
            (m.stage, m.fallback.clone())
        };
        DictateStatus {
            enabled,
            hotkey,
            verified,
            hold: KeyLabel {
                binding: hold.to_string(),
                label: hold.label(os),
            },
            toggle: b.toggle.map(|t| KeyLabel {
                binding: t.to_string(),
                label: t.label(os),
            }),
            latch,
            deep_link_command: deep_link_toggle_allowed(
                &session,
                unavailable,
                cfg!(target_os = "linux"),
            )
            .then(|| "xdg-open tandem://dictate/toggle".to_string()),
            session,
            mic: mic_id.and_then(|id| devices.into_iter().find(|d| d.id == id)),
            engine: self
                .setting::<String>(SettingKey::DictateEngine, "local".into())
                .await,
            model: active
                .as_deref()
                .and_then(|id| self.catalog.get(id))
                .map(|m| ModelRef {
                    id: m.id.clone(),
                    name: m.name.clone(),
                }),
            model_stage: stage.unwrap_or(if active.is_some() {
                ModelStage::Ready
            } else {
                ModelStage::None
            }),
            fallback,
            state: self
                .controller
                .get()
                .map(|c| c.state())
                .unwrap_or(TakeState::Idle),
            missing_tools: self.platform.missing_tools(),
            can_pause_media: !cfg!(windows),
            restores_clipboard: self.platform.restores_clipboard(),
            theme,
            widget_style,
            lock_position,
            quick_actions,
            enhance_disclosed: self
                .setting(SettingKey::DictateEnhanceDisclosed, false)
                .await,
        }
    }

    pub async fn emit_status(&self) {
        let s = self.status().await;
        crate::services::tray::update(&s);
        self.emit("dictate:status", json!(s), To::Both);
    }

    pub fn devices(&self) -> Vec<DeviceInfo> {
        self.backend.devices()
    }

    pub fn set_composer_focus(&self, target: Option<String>) {
        *self.shared.composer.lock() = target;
    }

    pub fn set_main_focused(&self, focused: bool) {
        self.shared.main_focused.store(focused, Ordering::Release);
    }

    pub async fn set_binding(
        &self,
        binding: Option<String>,
        toggle: bool,
    ) -> Result<HotkeyStatus, IpcError> {
        let parsed = match &binding {
            None => None,
            Some(s) => Some(
                s.parse::<Binding>()
                    .map_err(|e| IpcError::new(ErrorCode::Invalid, e.to_string()))?,
            ),
        };
        if let Some(b) = &parsed {
            b.validate(toggle, Os::current())
                .map_err(|e| IpcError::new(ErrorCode::Invalid, e.to_string()))?;
        }
        let key = if toggle {
            SettingKey::DictateToggleHotkey
        } else {
            SettingKey::DictateHotkey
        };
        let value = parsed.map(|b| json!(b.to_string())).unwrap_or(Value::Null);
        settings::set(&self.store, key, value.clone()).await?;
        self.emit(
            "settings:changed",
            json!({ "key": key, "value": value }),
            To::Main,
        );
        if !toggle {
            settings::set(&self.store, SettingKey::DictateHotkeyVerified, json!(false)).await?;
            self.emit(
                "settings:changed",
                json!({ "key": "dictate_hotkey_verified", "value": false }),
                To::Main,
            );
        }
        self.restart_hotkeys().await;
        self.emit_status().await;
        Ok(self.hotkey_status.lock().clone())
    }

    pub fn start_take(&self, origin: Origin) -> Result<TakeId, IpcError> {
        let c = self
            .controller
            .get()
            .ok_or_else(|| IpcError::new(ErrorCode::Unavailable, "Dictate isn't running"))?;
        if self.engine.current().is_none() {
            return Err(IpcError::new(
                ErrorCode::Unavailable,
                "Download a model to dictate locally",
            ));
        }
        c.start_take(origin)
            .map_err(|e| IpcError::new(ErrorCode::Unavailable, e.to_string()))
    }

    // ── Models ──────────────────────────────────────────────────────────

    fn installed(&self) -> Vec<String> {
        self.model_store.installed(&self.catalog)
    }

    pub fn models_view(&self) -> ModelsView {
        let installed = self.installed();
        let active = self.engine.current_id();
        let m = self.models.lock();
        let models = self
            .catalog
            .models
            .iter()
            .map(|model| {
                let id = &model.id;
                let dl = m.downloads.get(id);
                let state = if m.loading.as_deref() == Some(id) {
                    ModelState::Loading
                } else if let Some(d) = dl {
                    match d.progress {
                        Some(DownloadProgress::Verifying) => ModelState::Verifying,
                        Some(_) => ModelState::Downloading,
                        None => ModelState::Queued,
                    }
                } else if active.as_deref() == Some(id) {
                    ModelState::Active
                } else if installed.contains(id) {
                    ModelState::Downloaded
                } else if m.failed.contains_key(id) {
                    ModelState::Failed
                } else {
                    ModelState::Absent
                };
                ModelView {
                    id: id.clone(),
                    name: model.name.clone(),
                    bytes: model.bytes(),
                    langs_label: model.langs_label.clone(),
                    languages: model.languages.clone(),
                    recommended: model.recommended_on(&self.platform_name),
                    available: model.available_on(&self.platform_name),
                    cpu_warning: model.cpu_warning_on(&self.platform_name),
                    runnable: stt::runtime_built(model.runtime),
                    state,
                    progress: dl.and_then(|d| d.progress.clone()),
                    error: m.failed.get(id).cloned(),
                    license: model.license.clone(),
                }
            })
            .collect();
        ModelsView {
            models,
            used_bytes: self.model_store.used_bytes(),
            active,
            pending: self.engine.pending(),
        }
    }

    fn emit_models(&self) {
        self.emit("dictate:models", json!(self.models_view()), To::Main);
    }

    /// The model setting, or the platform's recommendation.
    async fn wanted_model(&self) -> Option<String> {
        let chosen: Option<String> = self.setting(SettingKey::DictateModel, None).await;
        chosen.or_else(|| {
            self.catalog
                .recommended(&self.platform_name)
                .map(|m| m.id.clone())
        })
    }

    /// Loads the chosen model, or the best one downloaded (and says so).
    async fn load_selected(&self) {
        let installed = self.installed();
        let Some(wanted) = self.wanted_model().await else {
            return;
        };
        let runnable = |id: &str| {
            self.catalog.get(id).is_some_and(|m| {
                stt::runtime_built(m.runtime) && m.available_on(&self.platform_name)
            })
        };
        if installed.contains(&wanted) && runnable(&wanted) {
            self.models.lock().fallback = None;
            if self.engine.current_id().as_deref() != Some(wanted.as_str()) {
                self.load_model(&wanted).await;
            }
            return;
        }
        let usable: Vec<String> = installed.into_iter().filter(|id| runnable(id)).collect();
        let best = self
            .catalog
            .best_installed(&self.platform_name, &usable)
            .map(|m| m.id.clone());
        let Some(best) = best else {
            self.models.lock().fallback = None;
            return;
        };
        let explicit: Option<String> = self.setting(SettingKey::DictateModel, None).await;
        if explicit.is_some() {
            let name = |id: &str| {
                self.catalog
                    .get(id)
                    .map(|m| m.name.clone())
                    .unwrap_or_default()
            };
            self.models.lock().fallback = Some(ModelFallback {
                using: name(&best),
                missing: name(&wanted),
            });
            self.emit(
                "dictate:toast",
                json!(DictateToast {
                    code: ToastCode::ModelFallback,
                    error: false,
                    app: None,
                    model: Some(name(&best)),
                    previous: Some(name(&wanted)),
                    message: None,
                    actions: Vec::new(),
                    detail: format!("{wanted} isn't in the models folder."),
                }),
                To::Main,
            );
        }
        if self.engine.current_id().as_deref() != Some(best.as_str()) {
            self.load_model(&best).await;
        }
    }

    /// Verifying → Loading → Ready; a failure keeps the old engine. During
    /// a take the switch waits for the take to end (F-DI-04 AC4).
    pub async fn load_model(&self, id: &str) {
        let Some(model) = self.catalog.get(id).cloned() else {
            return;
        };
        if self.engine.request(id) == Switch::Queued {
            self.emit_models();
            return;
        }
        {
            let mut m = self.models.lock();
            m.loading = Some(id.to_string());
            m.stage = Some(ModelStage::Verifying);
        }
        self.emit_models();
        self.emit_status().await;
        let installed = self.model_store.is_installed(&model);
        let result = if !installed {
            Err(stt::SttError::Load(format!(
                "{} isn't downloaded",
                model.name
            )))
        } else {
            self.models.lock().stage = Some(ModelStage::Loading);
            self.emit_status().await;
            if let Ok(fake) = std::env::var("TANDEM_FAKE_STT") {
                if cfg!(debug_assertions) && self.e2e {
                    Ok(Arc::new(stt::FakeStt::new(id, &fake)) as Arc<dyn stt::SttEngine>)
                } else {
                    Err(stt::SttError::Unsupported("fake".into()))
                }
            } else {
                let store = self.model_store.clone();
                let m = model.clone();
                tokio::task::spawn_blocking(move || stt::load(&m, &store))
                    .await
                    .unwrap_or_else(|e| Err(stt::SttError::Load(e.to_string())))
            }
        };
        let previous = self.engine.current_id();
        match result {
            Ok(engine) => {
                let installed_now = self.engine.install(engine);
                let mut m = self.models.lock();
                m.loading = None;
                m.stage = Some(ModelStage::Ready);
                m.failed.remove(id);
                if installed_now {
                    tracing::info!(code = "dictate.model_ready", model = %id, "speech model loaded");
                }
            }
            Err(err) => {
                tracing::warn!(code = "dictate.model_failed", model = %id, error = %err, "speech model not loaded");
                {
                    let mut m = self.models.lock();
                    m.loading = None;
                    m.stage = Some(if previous.is_some() {
                        ModelStage::Ready
                    } else {
                        ModelStage::Failed
                    });
                }
                if let Some(prev) = previous {
                    let name = |id: &str| {
                        self.catalog
                            .get(id)
                            .map(|m| m.name.clone())
                            .unwrap_or_default()
                    };
                    self.emit(
                        "dictate:toast",
                        json!(DictateToast {
                            code: ToastCode::ModelFailed,
                            error: true,
                            app: None,
                            model: Some(model.name.clone()),
                            previous: Some(name(&prev)),
                            message: None,
                            actions: Vec::new(),
                            detail: err.to_string(),
                        }),
                        To::Main,
                    );
                }
            }
        }
        self.emit_models();
        self.emit_status().await;
    }

    pub async fn select_model(&self, id: &str) -> Result<ModelsView, IpcError> {
        let model = self
            .catalog
            .get(id)
            .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "Unknown model"))?;
        if !model.available_on(&self.platform_name) || !stt::runtime_built(model.runtime) {
            return Err(IpcError::new(
                ErrorCode::Unavailable,
                "Not available on this computer",
            ));
        }
        settings::set(&self.store, SettingKey::DictateModel, json!(id)).await?;
        self.emit(
            "settings:changed",
            json!({ "key": "dictate_model", "value": id }),
            To::Main,
        );
        if self.model_store.is_installed(model) {
            self.models.lock().fallback = None;
            self.load_model(id).await;
        }
        Ok(self.models_view())
    }

    pub fn download(
        self: &Arc<Self>,
        id: &str,
        client: reqwest::Client,
    ) -> Result<ModelsView, IpcError> {
        let model = self
            .catalog
            .get(id)
            .cloned()
            .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "Unknown model"))?;
        if !model.available_on(&self.platform_name) {
            return Err(IpcError::new(
                ErrorCode::Unavailable,
                "Not available on this computer",
            ));
        }
        let cancel = CancellationToken::new();
        {
            let mut m = self.models.lock();
            if m.downloads.contains_key(id) {
                drop(m);
                return Ok(self.models_view());
            }
            m.failed.remove(id);
            m.downloads.insert(
                id.to_string(),
                Download {
                    cancel: cancel.clone(),
                    progress: None,
                },
            );
        }
        self.emit_models();
        let me = self.clone();
        let id = id.to_string();
        tauri::async_runtime::spawn(async move {
            let last = Arc::new(Mutex::new(
                std::time::Instant::now() - Duration::from_secs(1),
            ));
            let sink_me = me.clone();
            let sink_id = id.clone();
            let progress: tandem_voice::models::ProgressSink =
                Arc::new(move |p: DownloadProgress| {
                    let urgent = !matches!(p, DownloadProgress::Downloading { .. });
                    if let Some(d) = sink_me.models.lock().downloads.get_mut(&sink_id) {
                        d.progress = Some(p);
                    }
                    let mut l = last.lock();
                    if urgent || l.elapsed() >= Duration::from_millis(250) {
                        *l = std::time::Instant::now();
                        drop(l);
                        sink_me.emit_models();
                    }
                });
            let result = Downloader::new(client)
                .download(&me.model_store, &model, progress, cancel)
                .await;
            {
                let mut m = me.models.lock();
                m.downloads.remove(&id);
                match &result {
                    Ok(()) => {}
                    Err(ModelError::Cancelled) => {}
                    Err(e) => {
                        m.failed.insert(id.clone(), e.to_string());
                    }
                }
            }
            match result {
                Ok(()) => {
                    tracing::info!(code = "dictate.model_downloaded", model = %id, "speech model downloaded");
                    // The first model, or the chosen one, starts right away.
                    let wanted = me.wanted_model().await;
                    if me.engine.current().is_none() || wanted.as_deref() == Some(id.as_str()) {
                        me.load_selected().await;
                    }
                }
                Err(e) => {
                    tracing::info!(code = "dictate.model_download_failed", model = %id, error = %e, "download stopped")
                }
            }
            me.emit_models();
            me.emit_status().await;
        });
        Ok(self.models_view())
    }

    pub fn cancel_download(&self, id: &str) -> ModelsView {
        if let Some(d) = self.models.lock().downloads.get(id) {
            d.cancel.cancel();
        }
        self.models_view()
    }

    pub async fn remove_model(&self, id: &str) -> Result<ModelsView, IpcError> {
        self.cancel_download(id);
        if self.engine.current_id().as_deref() == Some(id) {
            self.engine.clear();
        }
        self.model_store
            .remove(id)
            .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
        self.models.lock().failed.remove(id);
        self.load_selected().await;
        self.emit_models();
        self.emit_status().await;
        Ok(self.models_view())
    }

    /// "Remove unused models…": every downloaded model but the active one.
    pub async fn remove_unused(&self) -> Result<ModelsView, IpcError> {
        let active = self.engine.current_id();
        for id in self.installed() {
            if Some(&id) != active.as_ref() {
                self.model_store
                    .remove(&id)
                    .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
            }
        }
        self.emit_models();
        Ok(self.models_view())
    }

    // ── History ─────────────────────────────────────────────────────────

    pub async fn history(
        &self,
        query: Option<String>,
        before: Option<HistoryCursor>,
        limit: Option<u32>,
    ) -> Result<TakePage, IpcError> {
        let (rows, next) = repo::history(
            &self.store,
            query.filter(|q| !q.trim().is_empty()),
            before.map(|b| (b.created_at, b.id)),
            limit.unwrap_or(repo::PAGE as u32) as usize,
        )
        .await?;
        Ok(TakePage {
            takes: rows.into_iter().map(TakeView::from).collect(),
            next: next.map(|(created_at, id)| HistoryCursor { created_at, id }),
        })
    }

    pub async fn stats(&self) -> Result<DictateStats, IpcError> {
        Ok(DictateStats::from_totals(repo::totals(&self.store).await?))
    }

    pub async fn export_history(&self, path: &str) -> Result<(), IpcError> {
        let rows = repo::all_takes(&self.store).await?;
        let csv = takes_csv(&rows);
        tandem_core::atomic::atomic_write(std::path::Path::new(path), csv.as_bytes(), None)
            .map_err(|e| IpcError::new(ErrorCode::Io, format!("Couldn't save the file: {e}")))
    }

    pub async fn correct_take(
        &self,
        take_id: &str,
        corrected: &str,
        remember: bool,
    ) -> Result<Option<Rule>, IpcError> {
        let corrected = corrected.trim();
        if corrected.is_empty() {
            return Err(IpcError::new(ErrorCode::Invalid, "Write your correction"));
        }
        let take = repo::get_take(&self.store, take_id.to_string())
            .await?
            .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "That take is gone"))?;
        let words = tandem_voice::postproc::word_count(corrected);
        repo::update_take_text(
            &self.store,
            take_id.to_string(),
            corrected.to_string(),
            words,
        )
        .await?;
        self.emit("dictate:history", json!(null), To::Main);
        if !remember {
            return Ok(None);
        }
        let Some((from, to)) = correction_from(&take.text, corrected) else {
            return Ok(None);
        };
        let rule = Rule::correction("", &from, &to);
        Ok(Some(self.save_rule(rule).await?))
    }

    // ── Dictionary ──────────────────────────────────────────────────────

    pub async fn reload_rules(&self) {
        if let Ok(rows) = repo::rules(&self.store).await {
            *self.shared.rules.lock() = rows.into_iter().map(rule_from_row).collect();
        }
    }

    pub fn rules(&self) -> Vec<Rule> {
        self.shared.rules.lock().clone()
    }

    pub async fn save_rule(&self, rule: Rule) -> Result<Rule, IpcError> {
        let mut rule = clean_rule(rule)?;
        rule.updated_at = tandem_store::now_ms();
        rule.deleted_at = None;
        let rev = repo::get_rule(&self.store, rule.id.clone())
            .await?
            .map(|r| r.sync_rev)
            .unwrap_or(0);
        repo::put_rules(&self.store, vec![row_from_rule(&rule, rev)]).await?;
        self.reload_rules().await;
        Ok(rule)
    }

    pub async fn delete_rules(&self, ids: Vec<String>) -> Result<(), IpcError> {
        repo::delete_rules(&self.store, ids).await?;
        self.reload_rules().await;
        Ok(())
    }

    pub async fn restore_rule(&self, id: &str) -> Result<Rule, IpcError> {
        if !repo::restore_rule(&self.store, id.to_string()).await? {
            return Err(IpcError::new(
                ErrorCode::NotFound,
                "That rule can't be restored",
            ));
        }
        self.reload_rules().await;
        let row = repo::get_rule(&self.store, id.to_string())
            .await?
            .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "That rule is gone"))?;
        Ok(rule_from_row(row))
    }

    pub async fn deleted_rules(&self) -> Result<Vec<Rule>, IpcError> {
        Ok(repo::deleted_rules(&self.store)
            .await?
            .into_iter()
            .map(rule_from_row)
            .collect())
    }

    pub fn test_dictionary(&self, sample: &str, app: Option<&str>, lang: Option<&str>) -> String {
        let rules = self.rules();
        let lang = lang.filter(|l| *l != "auto" && !l.is_empty());
        let app = app.filter(|a| !a.trim().is_empty());
        tandem_voice::postproc::apply(sample, &Dictionary::compile(&rules, lang, app))
    }

    pub async fn import_preview(&self, path: &str, csv: bool) -> Result<ImportPreview, IpcError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| IpcError::new(ErrorCode::Io, format!("Couldn't read the file: {e}")))?;
        if text.len() > 5 * 1024 * 1024 {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "That file is too large to import (5 MB max)",
            ));
        }
        let (rules, skipped) = parse_rules(&text, csv, tandem_store::now_ms());
        let existing = self.rules();
        let mut added = Vec::new();
        let mut updated = Vec::new();
        for r in rules {
            if let Some(old) = existing.iter().find(|e| e.id == r.id) {
                if *old != r {
                    updated.push(r);
                }
            } else if existing
                .iter()
                .any(|e| same_rule_key(e) == same_rule_key(&r))
            {
                continue;
            } else {
                added.push(r);
            }
        }
        let preview_id = format!("imp_{}", ulid::Ulid::new().to_string().to_lowercase());
        let all: Vec<Rule> = added.iter().chain(updated.iter()).cloned().collect();
        self.previews.lock().insert(preview_id.clone(), all);
        Ok(ImportPreview {
            preview_id,
            added,
            updated,
            skipped,
        })
    }

    pub async fn import_commit(&self, preview_id: &str) -> Result<usize, IpcError> {
        let rules = self
            .previews
            .lock()
            .remove(preview_id)
            .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "Preview the import again"))?;
        let n = rules.len();
        let now = tandem_store::now_ms();
        let rows = rules
            .into_iter()
            .map(|mut r| {
                r.updated_at = now;
                row_from_rule(&r, 0)
            })
            .collect();
        repo::put_rules(&self.store, rows).await?;
        self.reload_rules().await;
        Ok(n)
    }

    pub async fn export_rules(&self, path: &str, csv: bool) -> Result<(), IpcError> {
        let rules = self.rules();
        let body = if csv {
            rules_csv(&rules)
        } else {
            serde_json::to_string_pretty(&DictionaryFile { version: 1, rules })
                .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?
        };
        tandem_core::atomic::atomic_write(std::path::Path::new(path), body.as_bytes(), None)
            .map_err(|e| IpcError::new(ErrorCode::Io, format!("Couldn't save the file: {e}")))
    }

    pub async fn suggestions(&self) -> Result<Vec<Suggestion>, IpcError> {
        let _ = repo::purge(&self.store, tandem_store::now_ms()).await;
        Ok(repo::open_suggestions(&self.store)
            .await?
            .into_iter()
            .map(|s| Suggestion {
                id: s.id,
                term: s.term,
                kind: s.kind,
                evidence: s.evidence,
                last_seen: s.last_seen,
            })
            .collect())
    }

    /// Only Remember creates a rule (F-DI-09).
    pub async fn resolve_suggestion(
        &self,
        id: &str,
        remember: bool,
    ) -> Result<Option<Rule>, IpcError> {
        let s = repo::resolve_suggestion(&self.store, id.to_string(), remember)
            .await?
            .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "That suggestion is gone"))?;
        if !remember {
            return Ok(None);
        }
        let rule = Rule::word("", &s.term, &[]);
        Ok(Some(self.save_rule(rule).await?))
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// The harness's fake mic source for the next take.
    pub fn test_mic(&self, source: FakeSource) -> Result<(), IpcError> {
        let fakes = self
            .fakes
            .as_ref()
            .ok_or_else(|| IpcError::new(ErrorCode::Unavailable, "not under the harness"))?;
        fakes.audio.set_source("fake-mic", source);
        if let Some(a) = self.audio.get() {
            a.reopen();
        }
        Ok(())
    }
}

type Backends = (
    Arc<dyn AudioBackend>,
    Arc<dyn HotkeyBackend>,
    Arc<dyn Platform>,
    Option<Fakes>,
);

/// Faint noise (about −100 dBFS): a quiet room, which a real mic never
/// reports as exact zeros.
pub fn dither(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| if i % 2 == 0 { 1e-5 } else { -1e-5 })
        .collect()
}

#[cfg(feature = "os-audio")]
fn native_backend() -> Arc<dyn AudioBackend> {
    Arc::new(tandem_voice::capture_cpal::CpalBackend)
}

#[cfg(feature = "os-audio")]
fn native_chimes() -> Arc<dyn ChimePlayer> {
    Arc::new(tandem_voice::chimes::CpalPlayer)
}

/// Type-check builds without `os-audio` (D-2105): no microphone, no chimes.
#[cfg(not(feature = "os-audio"))]
fn native_backend() -> Arc<dyn AudioBackend> {
    struct NoMicrophone;
    impl AudioBackend for NoMicrophone {
        fn devices(&self) -> Vec<DeviceInfo> {
            Vec::new()
        }
        fn open(
            &self,
            _device: &str,
            _sink: tandem_voice::capture::SampleSink,
            _on_error: tandem_voice::capture::ErrorSink,
        ) -> Result<Box<dyn tandem_voice::capture::InputStream>, tandem_voice::capture::CaptureError>
        {
            Err(tandem_voice::capture::CaptureError::NoDevice)
        }
    }
    Arc::new(NoMicrophone)
}

#[cfg(not(feature = "os-audio"))]
fn native_chimes() -> Arc<dyn ChimePlayer> {
    Arc::new(SilentPlayer::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take(text: &str, at: i64) -> repo::TakeRow {
        repo::TakeRow {
            id: format!("t{at}"),
            created_at: at,
            text: text.into(),
            engine: "local".into(),
            model: "whisper-base.en".into(),
            word_count: 2,
            duration_ms: 1500,
            app_id: None,
        }
    }

    #[test]
    fn f_di_08_1_csv_has_the_exact_header_and_quotes_text() {
        let csv = takes_csv(&[take("hello, \"world\"", 0)]);
        let mut lines = csv.lines();
        assert_eq!(
            lines.next(),
            Some("timestamp,text,word_count,duration_seconds,model")
        );
        assert_eq!(
            lines.next(),
            Some("1970-01-01T00:00:00Z,\"hello, \"\"world\"\"\",2,1.5,whisper-base.en")
        );
    }

    #[test]
    fn f_di_08_stats_wpm_and_time_saved() {
        let s = DictateStats::from_totals(repo::Totals {
            words: 400,
            speaking_ms: 120_000,
            takes: 10,
        });
        assert_eq!(s.wpm, 200);
        // 400 words at 40 wpm = 10 min, minus 2 min of speaking.
        assert_eq!(s.saved_ms, 480_000);
        assert_eq!(DictateStats::from_totals(repo::Totals::default()).wpm, 0);
    }

    #[test]
    fn f_di_08_remember_this_correction_takes_the_changed_words() {
        assert_eq!(
            correction_from("ship the tory build now", "ship the Tauri build now"),
            Some(("tory".into(), "Tauri".into()))
        );
        assert_eq!(
            correction_from("we use react native", "we use React Native."),
            Some(("react native".into(), "React Native".into()))
        );
        assert_eq!(correction_from("same", "same"), None);
    }

    #[test]
    fn f_di_09_import_and_export_round_trip() {
        let rules = vec![
            Rule::correction("a", "tandem", "Tandem"),
            Rule::word("b", "Tauri", &["tory", "tawri"]),
        ];
        let csv = rules_csv(&rules);
        assert!(csv.starts_with(RULES_CSV_HEADER));
        let (back, skipped) = parse_rules(&csv, true, 5);
        assert!(skipped.is_empty(), "{skipped:?}");
        assert_eq!(back.len(), 2);
        assert_eq!(back[1].phrases, vec!["tory".to_string(), "tawri".into()]);
        assert_eq!(back[1].kind, RuleKind::Word);
        let json = serde_json::to_string(&DictionaryFile { version: 1, rules }).expect("json");
        let (back, skipped) = parse_rules(&json, false, 5);
        assert_eq!((back.len(), skipped.len()), (2, 0));
        let (_, skipped) = parse_rules("correction,,x\n", true, 5);
        assert_eq!(skipped.len(), 1);
        let (_, skipped) = parse_rules("not json", false, 5);
        assert_eq!(skipped.len(), 1);
    }

    #[test]
    fn f_di_09_review_mines_proper_nouns_acronyms_and_spellings() {
        let takes = vec![
            take("deploy to Kubernetes today", 1),
            take("the Kubernetes pod died", 2),
            take("restart Kubernetes please", 3),
            take("check the CI logs", 4),
            take("CI is red again", 5),
            take("ship postgres", 6),
            take("ship Postgres", 7),
            take("ship POSTGRES", 8),
        ];
        let found = mine_suggestions(&takes, &[]);
        let terms: Vec<(&str, &str)> = found
            .iter()
            .map(|(t, k, _)| (t.as_str(), k.as_str()))
            .collect();
        assert!(terms.contains(&("Kubernetes", "proper_noun")), "{terms:?}");
        assert!(terms.contains(&("CI", "acronym")), "{terms:?}");
        assert!(
            terms
                .iter()
                .any(|(t, k)| t.eq_ignore_ascii_case("postgres") && *k == "spelling"),
            "{terms:?}"
        );
        let known = mine_suggestions(&takes, &["Kubernetes".into()]);
        assert!(!known.iter().any(|(t, _, _)| t == "Kubernetes"));
    }

    #[test]
    fn f_di_09_rules_validate_and_map_to_rows() {
        let r = clean_rule(Rule::correction("", " pr ", " pull request ")).expect("ok");
        assert!(r.id.starts_with("rule_"));
        assert_eq!(r.phrases, vec!["pr".to_string()]);
        assert!(clean_rule(Rule::correction("", "", "x")).is_err());
        assert!(clean_rule(Rule::correction("", "x", " ")).is_err());
        let row = row_from_rule(&r, 3);
        assert_eq!(rule_from_row(row), r);
    }

    struct FakeWidget(Mutex<Vec<bool>>);
    impl WidgetHost for FakeWidget {
        fn show(&self) {
            self.0.lock().push(true);
        }
        fn hide(&self) {
            self.0.lock().push(false);
        }
    }

    #[test]
    fn d_515_the_toggle_link_is_honoured_only_where_it_is_the_fallback() {
        // Wayland: the portal can't bind every key, so the link is documented.
        assert!(deep_link_toggle_allowed("wayland", false, true));
        // X11 or no session with the shortcut unavailable: the fallback too.
        assert!(deep_link_toggle_allowed("x11", true, true));
        assert!(deep_link_toggle_allowed("none", true, true));
        // A working global shortcut, or any other OS: a web page's link.
        assert!(!deep_link_toggle_allowed("x11", false, true));
        assert!(!deep_link_toggle_allowed("mac", false, false));
        assert!(!deep_link_toggle_allowed("mac", true, false));
        assert!(!deep_link_toggle_allowed("windows", true, false));
    }

    #[tokio::test]
    async fn d_515_a_refused_link_reaches_nothing_and_an_allowed_one_reaches_the_controller() {
        let dir = tempfile::tempdir().expect("dir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let svc = DictateService::new(store, data, true);
        // Every take event and toast the controller sends.
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let log = seen.clone();
        svc.set_emitter(Arc::new(move |name: &str, _v: Value, _to: To| {
            if name == "dictate:state" || name == "dictate:toast" {
                log.lock().push(name.to_string());
            }
        }));
        svc.start().await;
        assert!(!svc.toggle_from_link_in("x11", false, true));
        assert!(!svc.toggle_from_link_in("mac", false, false));
        assert!(!svc.toggle_from_link_in("windows", true, false));
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            seen.lock().is_empty(),
            "a refused link reaches nothing: {:?}",
            seen.lock()
        );
        assert!(!svc.shared.link_take.load(Ordering::Acquire));
        assert!(
            !svc.setting(SettingKey::DictateHotkeyVerified, false).await,
            "a refused link verifies nothing"
        );
        // Allowed: the controller acts (with no model here it answers with
        // "Download a model", which is still the take being asked for).
        assert!(svc.toggle_from_link_in("wayland", false, true));
        let mut acted = false;
        for _ in 0..50 {
            if !seen.lock().is_empty() {
                acted = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(acted, "the allowed link reaches the controller");
        // In the fallback mode the link verifies the setup (F-DI-11).
        let mut verified = false;
        for _ in 0..50 {
            if svc.setting(SettingKey::DictateHotkeyVerified, false).await {
                verified = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(verified, "an allowed link marks the shortcut verified");
        if let Some(c) = svc.controller() {
            c.cancel();
        }
    }

    #[test]
    fn d_515_a_link_take_shows_the_widget_even_when_hidden() {
        let dir = tempfile::tempdir().expect("dir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let svc = DictateService::new(store, data, true);
        let w = Arc::new(FakeWidget(Mutex::new(Vec::new())));
        svc.set_widget(w.clone());
        *svc.shared.visibility.lock() = "hidden".into();
        let ev = |state| DictateStateEvent {
            state,
            take_id: None,
            hands_free: false,
            composer: None,
            words: Some(2),
        };
        svc.shared.link_take.store(true, Ordering::Release);
        svc.shared.widget_for(&ev(DictatePhase::Listening));
        assert_eq!(w.0.lock().last(), Some(&true), "shown although Hidden");
        svc.shared.widget_for(&ev(DictatePhase::Inserted));
        assert!(
            !svc.shared.link_take.load(Ordering::Acquire),
            "cleared when the take ends"
        );
        // The next take from the shortcut follows the setting again.
        svc.shared.widget_for(&ev(DictatePhase::Listening));
        assert_eq!(w.0.lock().last(), Some(&false));
    }

    #[test]
    fn f_di_07_1_while_recording_shows_on_start_and_hides_1s_after_insertion() {
        let dir = tempfile::tempdir().expect("dir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let svc = DictateService::new(store, data, true);
        let w = Arc::new(FakeWidget(Mutex::new(Vec::new())));
        svc.set_widget(w.clone());
        let ev = |state| DictateStateEvent {
            state,
            take_id: None,
            hands_free: false,
            composer: None,
            words: Some(2),
        };
        svc.shared.widget_for(&ev(DictatePhase::Listening));
        assert_eq!(*w.0.lock(), vec![true]);
        svc.shared.widget_for(&ev(DictatePhase::Transcribing));
        svc.shared.widget_for(&ev(DictatePhase::Inserted));
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(w.0.lock().last(), Some(&true), "still shown before 1 s");
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(
            w.0.lock().last(),
            Some(&false),
            "hidden 1 s after insertion"
        );
        // A new take before the timer fires keeps it up.
        w.0.lock().clear();
        svc.shared.widget_for(&ev(DictatePhase::Inserted));
        svc.shared.widget_for(&ev(DictatePhase::Listening));
        std::thread::sleep(Duration::from_millis(1300));
        assert_eq!(*w.0.lock(), vec![true]);
        // Composer takes never use the widget.
        w.0.lock().clear();
        let mut c = ev(DictatePhase::Listening);
        c.composer = Some("thread:1".into());
        svc.shared.widget_for(&c);
        assert_eq!(*w.0.lock(), vec![false]);
    }
}
