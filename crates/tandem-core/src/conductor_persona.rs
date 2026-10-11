//! Conductor's persona and opt-in memory (F-CN-06, ARCHITECTURE §12.6).
//!
//! The persona is read from settings (Settings › Conductor, UI-67). Memory is
//! a Markdown file at `$DATA/conductor/memory.md`, written only while
//! "Remember things between calls" is on, never holding credential-shaped
//! text. A call reads both once, at start, as a [`CallContext`].

use std::path::{Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::settings::SettingKey;

/// "Tell it about you" and each remembered fact are capped (UI-67).
pub const ABOUT_YOU_MAX: usize = 1_000;
pub const FACT_MAX: usize = 300;
/// The part of memory a call's instructions carry, so a long file can't
/// crowd out the workspace summary.
pub const MEMORY_IN_CALL_MAX: usize = 4_000;
/// Speech speed is a 5-step slider; 3 is normal.
pub const SPEED_STEPS: u8 = 5;

const MEMORY_HEADER: &str =
    "# Conductor memory\n\nThings you asked Conductor to remember. Edit or delete any line.\n\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Tone {
    #[default]
    Neutral,
    Warm,
    Brief,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Verbosity {
    Short,
    #[default]
    Normal,
    Detailed,
}

/// "After each call" (UI-67).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Reflection {
    /// Proposed memories become Keep / Discard cards.
    #[default]
    Ask,
    /// Proposed memories are saved without asking.
    Auto,
    /// No post-call reflection.
    Off,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Persona {
    pub name: String,
    pub voice: String,
    pub tone: Tone,
    pub verbosity: Verbosity,
    /// 1 (slowest) to 5 (fastest); 3 is normal.
    pub speed: u8,
    pub about_you: Option<String>,
}

impl Default for Persona {
    fn default() -> Self {
        Self {
            name: "Conductor".into(),
            voice: "default".into(),
            tone: Tone::default(),
            verbosity: Verbosity::default(),
            speed: 3,
            about_you: None,
        }
    }
}

/// What a call starts with: the persona and, when memory is on, what's
/// remembered. Read once per call by [`CallContext::from_settings`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CallContext {
    pub persona: Persona,
    /// `None` while memory is off, or when nothing is remembered yet.
    pub memory: Option<String>,
    pub memory_enabled: bool,
}

impl Persona {
    /// Builds the persona from setting values; anything missing or out of
    /// range (an older build's value) falls back to the default.
    pub fn from_settings(get: impl Fn(SettingKey) -> Option<Value>) -> Self {
        let d = Persona::default();
        let text = |k| {
            get(k)
                .and_then(|v| v.as_str().map(str::trim).map(String::from))
                .filter(|s| !s.is_empty())
        };
        Persona {
            name: text(SettingKey::ConductorName)
                .map(|n| n.chars().take(40).collect())
                .unwrap_or(d.name),
            voice: text(SettingKey::ConductorVoice).unwrap_or(d.voice),
            tone: parse(&get, SettingKey::ConductorTone).unwrap_or(d.tone),
            verbosity: parse(&get, SettingKey::ConductorVerbosity).unwrap_or(d.verbosity),
            speed: get(SettingKey::ConductorSpeed)
                .and_then(|v| v.as_u64())
                .filter(|s| (1..=SPEED_STEPS as u64).contains(s))
                .map(|s| s as u8)
                .unwrap_or(d.speed),
            about_you: text(SettingKey::ConductorAboutYou)
                .map(|a| a.chars().take(ABOUT_YOU_MAX).collect()),
        }
    }
}

impl CallContext {
    /// The one read a call makes when it starts: the persona from settings
    /// and, only while memory is on, `memory.md`.
    pub fn from_settings(get: impl Fn(SettingKey) -> Option<Value>, memory: &MemoryStore) -> Self {
        let persona = Persona::from_settings(&get);
        let memory_enabled = get(SettingKey::ConductorMemory)
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let memory = memory_enabled
            .then(|| memory.facts().ok())
            .flatten()
            .filter(|f| !f.is_empty())
            .map(|facts| {
                let mut out = String::new();
                for f in facts {
                    let line = format!("- {f}\n");
                    if out.len() + line.len() > MEMORY_IN_CALL_MAX {
                        break;
                    }
                    out.push_str(&line);
                }
                out
            });
        Self {
            persona,
            memory,
            memory_enabled,
        }
    }

    /// The persona part of the talker's session instructions. The call adds
    /// the workspace summary after it.
    pub fn instructions(&self) -> String {
        let p = &self.persona;
        let tone = match p.tone {
            Tone::Neutral => "Keep a neutral, matter-of-fact tone.",
            Tone::Warm => "Be warm and friendly.",
            Tone::Brief => "Be brisk: no pleasantries.",
        };
        let verbosity = match p.verbosity {
            Verbosity::Short => "Answer in one short sentence when you can.",
            Verbosity::Normal => "Answer in a sentence or two.",
            Verbosity::Detailed => "Give detail when it helps, in a few sentences.",
        };
        let speed = match p.speed {
            1 => "Speak slowly.",
            2 => "Speak a little slower than usual.",
            4 => "Speak a little faster than usual.",
            5 => "Speak quickly.",
            _ => "Speak at a normal pace.",
        };
        let mut out = format!(
            "Your name is {}. The user wakes you with \"Hey Conductor\". {tone} {verbosity} {speed}\n",
            p.name
        );
        if let Some(about) = &p.about_you {
            out.push_str("\nWhat the user told you about themselves:\n");
            out.push_str(about);
            out.push('\n');
        }
        if let Some(memory) = &self.memory {
            out.push_str("\nWhat you remember from earlier calls:\n");
            out.push_str(memory);
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MemoryError {
    /// "Remember things between calls" is off: nothing is written.
    #[error("Memory is off")]
    Off,
    /// The fact looks like a password, key or token.
    #[error("That looks like a secret, so Conductor won't remember it.")]
    Credential,
    #[error("Nothing to remember")]
    Empty,
    #[error("{0}")]
    Io(String),
}

impl From<std::io::Error> for MemoryError {
    fn from(e: std::io::Error) -> Self {
        MemoryError::Io(e.to_string())
    }
}

/// True when `text` looks like it holds a credential: anything the secret
/// redactor would hide, or "my password/token/key is …" with a value.
pub fn looks_like_credential(text: &str) -> bool {
    if crate::redact::Redactor::global().redact(text) != text {
        return true;
    }
    static SAID: std::sync::OnceLock<Option<Regex>> = std::sync::OnceLock::new();
    SAID.get_or_init(|| {
        Regex::new(
            r"(?i)\b(?:password|passcode|passphrase|pin|secret|token|api[ _-]?key|access[ _-]?key|private[ _-]?key|recovery (?:code|phrase)|seed phrase|credit card(?: number)?|card number|cvv|social security number|ssn)\b\s*(?:is|was|=|:)\s*\S+",
        )
        .ok()
    })
    .as_ref()
    .is_some_and(|re| re.is_match(text))
}

/// Proposed memories from post-call reflection, waiting for Keep / Discard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MemoryProposal {
    pub id: String,
    pub text: String,
}

/// What Settings › Conductor shows for memory (`conductor_memory_get`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConductorMemory {
    pub enabled: bool,
    /// The file as it is on disk ("" when there's none).
    pub text: String,
    pub facts: Vec<String>,
    pub proposals: Vec<MemoryProposal>,
    /// The file's path, for "Open memory file".
    pub path: String,
}

/// `$DATA/conductor/`: `memory.md`, `proposals.json` and `journal/<date>.md`.
#[derive(Debug, Clone)]
pub struct MemoryStore {
    dir: PathBuf,
}

impl MemoryStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join("memory.md")
    }

    fn proposals_path(&self) -> PathBuf {
        self.dir.join("proposals.json")
    }

    pub fn journal_dir(&self) -> PathBuf {
        self.dir.join("journal")
    }

    /// The file as written ("" when there's none).
    pub fn text(&self) -> Result<String, MemoryError> {
        match std::fs::read_to_string(self.path()) {
            Ok(s) => Ok(s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(e.into()),
        }
    }

    /// The remembered facts: every `- ` line of the file.
    pub fn facts(&self) -> Result<Vec<String>, MemoryError> {
        Ok(parse_facts(&self.text()?))
    }

    /// "Remember that …": appends one fact. Refused while memory is off and
    /// for credential-shaped text (F-CN-06 AC1, AC2).
    pub fn remember(&self, enabled: bool, fact: &str) -> Result<(), MemoryError> {
        if !enabled {
            return Err(MemoryError::Off);
        }
        let fact = clean_fact(fact)?;
        let mut facts = self.facts()?;
        if facts.iter().any(|f| f.eq_ignore_ascii_case(&fact)) {
            return Ok(());
        }
        facts.push(fact);
        self.write_facts(&facts)
    }

    /// "Forget …": removes every fact containing `query` (case-insensitive)
    /// and returns how many went.
    pub fn forget(&self, query: &str) -> Result<usize, MemoryError> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Ok(0);
        }
        let facts = self.facts()?;
        let kept: Vec<String> = facts
            .iter()
            .filter(|f| !f.to_lowercase().contains(&q))
            .cloned()
            .collect();
        let gone = facts.len() - kept.len();
        if gone > 0 {
            self.write_facts(&kept)?;
        }
        Ok(gone)
    }

    /// Settings' editor saves the whole file. Any credential-shaped line
    /// refuses the save.
    pub fn save_text(&self, enabled: bool, text: &str) -> Result<(), MemoryError> {
        if !enabled {
            return Err(MemoryError::Off);
        }
        if looks_like_credential(text) {
            return Err(MemoryError::Credential);
        }
        self.write(text.as_bytes())
    }

    /// "Forget everything…": deletes the file, pending proposals and the
    /// call journal. Allowed while memory is off (clearing writes nothing).
    pub fn clear(&self) -> Result<(), MemoryError> {
        for p in [self.path(), self.proposals_path()] {
            match std::fs::remove_file(&p) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        match std::fs::remove_dir_all(self.journal_dir()) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn proposals(&self) -> Result<Vec<MemoryProposal>, MemoryError> {
        match std::fs::read(self.proposals_path()) {
            Ok(raw) => Ok(serde_json::from_slice(&raw).unwrap_or_default()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e.into()),
        }
    }

    /// Post-call reflection. With [`Reflection::Ask`] the facts become
    /// cards; with `Auto` they're saved; with `Off`, or memory off, nothing
    /// happens. Credential-shaped proposals are dropped. Returns what was
    /// queued or saved.
    pub fn reflect(
        &self,
        enabled: bool,
        mode: Reflection,
        proposed: &[String],
    ) -> Result<Vec<String>, MemoryError> {
        if !enabled || mode == Reflection::Off {
            return Ok(Vec::new());
        }
        let known = self.facts()?;
        let mut queued = self.proposals()?;
        let mut out = Vec::new();
        for p in proposed {
            let Ok(fact) = clean_fact(p) else { continue };
            let dup = |f: &String| f.eq_ignore_ascii_case(&fact);
            if known.iter().any(dup) || queued.iter().any(|q| dup(&q.text)) {
                continue;
            }
            match mode {
                Reflection::Auto => self.remember(true, &fact)?,
                _ => queued.push(MemoryProposal {
                    id: crate::ids::new_id("mem"),
                    text: fact.clone(),
                }),
            }
            out.push(fact);
        }
        if mode == Reflection::Ask {
            self.write_proposals(&queued)?;
        }
        Ok(out)
    }

    /// Keep or Discard proposal cards (`conductor_memory_resolve`).
    pub fn resolve(&self, enabled: bool, ids: &[String], keep: bool) -> Result<(), MemoryError> {
        let queued = self.proposals()?;
        let (chosen, rest): (Vec<_>, Vec<_>) =
            queued.into_iter().partition(|p| ids.contains(&p.id));
        if keep {
            for p in &chosen {
                self.remember(enabled, &p.text)?;
            }
        }
        self.write_proposals(&rest)
    }

    /// A short line in today's call journal, only while memory is on.
    pub fn journal(&self, enabled: bool, date: &str, line: &str) -> Result<(), MemoryError> {
        if !enabled {
            return Err(MemoryError::Off);
        }
        if looks_like_credential(line) {
            return Err(MemoryError::Credential);
        }
        let dir = self.journal_dir();
        crate::paths::create_private_dir(&dir)?;
        let path = dir.join(format!("{date}.md"));
        let mut text = std::fs::read_to_string(&path).unwrap_or_default();
        text.push_str("- ");
        text.push_str(&line.replace('\n', " "));
        text.push('\n');
        crate::atomic::atomic_write(&path, text.as_bytes(), Some(0o600))?;
        Ok(())
    }

    pub fn view(&self, enabled: bool) -> Result<ConductorMemory, MemoryError> {
        let text = self.text()?;
        Ok(ConductorMemory {
            enabled,
            facts: parse_facts(&text),
            text,
            proposals: self.proposals()?,
            path: self.path().display().to_string(),
        })
    }

    fn write_facts(&self, facts: &[String]) -> Result<(), MemoryError> {
        let mut text = self.text()?;
        if text.trim().is_empty() {
            text = MEMORY_HEADER.to_string();
        }
        // Keep everything that isn't a fact (headings, notes the user typed),
        // then the facts in order.
        let mut out: String = text
            .lines()
            .filter(|l| fact_of(l).is_none())
            .map(|l| format!("{l}\n"))
            .collect();
        while out.ends_with("\n\n") {
            out.pop();
        }
        if !out.is_empty() {
            out.push('\n');
        }
        for f in facts {
            out.push_str("- ");
            out.push_str(f);
            out.push('\n');
        }
        self.write(out.as_bytes())
    }

    fn write_proposals(&self, p: &[MemoryProposal]) -> Result<(), MemoryError> {
        if p.is_empty() {
            return match std::fs::remove_file(self.proposals_path()) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.into()),
            };
        }
        let raw = serde_json::to_vec(p).map_err(|e| MemoryError::Io(e.to_string()))?;
        crate::paths::create_private_dir(&self.dir)?;
        crate::atomic::atomic_write(&self.proposals_path(), &raw, Some(0o600))?;
        Ok(())
    }

    fn write(&self, bytes: &[u8]) -> Result<(), MemoryError> {
        crate::paths::create_private_dir(&self.dir)?;
        crate::atomic::atomic_write(&self.path(), bytes, Some(0o600))?;
        Ok(())
    }
}

fn parse<T: serde::de::DeserializeOwned>(
    get: &impl Fn(SettingKey) -> Option<Value>,
    k: SettingKey,
) -> Option<T> {
    get(k).and_then(|v| serde_json::from_value(v).ok())
}

fn fact_of(line: &str) -> Option<&str> {
    let l = line.trim_start();
    l.strip_prefix("- ")
        .or_else(|| l.strip_prefix("* "))
        .map(str::trim)
        .filter(|f| !f.is_empty())
}

fn parse_facts(text: &str) -> Vec<String> {
    text.lines().filter_map(fact_of).map(String::from).collect()
}

/// Trims a spoken fact ("remember that …") to one line within the cap, and
/// refuses credentials.
fn clean_fact(fact: &str) -> Result<String, MemoryError> {
    let f = fact.trim();
    let f = f
        .strip_prefix("remember that ")
        .or_else(|| f.strip_prefix("Remember that "))
        .unwrap_or(f);
    let f: String = f
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(FACT_MAX)
        .collect();
    if f.is_empty() {
        return Err(MemoryError::Empty);
    }
    if looks_like_credential(&f) {
        return Err(MemoryError::Credential);
    }
    Ok(f)
}

/// Where memory lives under a data dir.
pub fn memory_dir(data: &Path) -> PathBuf {
    data.join("conductor")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    fn settings(pairs: &[(SettingKey, Value)]) -> impl Fn(SettingKey) -> Option<Value> {
        let m: HashMap<SettingKey, Value> = pairs.iter().cloned().collect();
        move |k| m.get(&k).cloned()
    }

    // A fake key, assembled so secret scanners don't flag the fixture.
    fn fake_key() -> String {
        format!("sk-{}", "proj-Qdxidq7OyFjVisrM1h8vbvTXfQNNTGnNZlhyg74g")
    }

    #[test]
    fn f_cn_06_1_memory_off_writes_nothing() {
        let dir = tempfile::tempdir().expect("test");
        let store = MemoryStore::new(dir.path().join("conductor"));
        assert_eq!(
            store.remember(false, "I work on the billing service"),
            Err(MemoryError::Off)
        );
        let saved = store
            .reflect(false, Reflection::Auto, &["Prefers short answers".into()])
            .expect("test");
        assert!(saved.is_empty());
        assert_eq!(
            store.journal(false, "2026-10-10", "Call: 2 actions"),
            Err(MemoryError::Off)
        );
        assert_eq!(store.save_text(false, "- anything"), Err(MemoryError::Off));
        assert!(!store.path().exists(), "memory.md must not exist");
        assert!(
            !dir.path().join("conductor").exists(),
            "nothing under conductor/"
        );
        // And a call made with memory off reads none.
        store.remember(true, "Likes tea").expect("test");
        let ctx = CallContext::from_settings(
            settings(&[(SettingKey::ConductorMemory, json!(false))]),
            &store,
        );
        assert_eq!(ctx.memory, None);
        assert!(!ctx.instructions().contains("Likes tea"));
    }

    #[test]
    fn f_cn_06_2_credentials_are_refused() {
        let dir = tempfile::tempdir().expect("test");
        let store = MemoryStore::new(dir.path());
        let said = format!("Remember my token is {}", fake_key());
        assert_eq!(store.remember(true, &said), Err(MemoryError::Credential));
        assert_eq!(
            store.remember(true, "my password is hunter2"),
            Err(MemoryError::Credential)
        );
        assert_eq!(
            store.save_text(true, &format!("- key {}\n", fake_key())),
            Err(MemoryError::Credential)
        );
        let q = store
            .reflect(true, Reflection::Ask, &[format!("API key: {}", fake_key())])
            .expect("test");
        assert!(q.is_empty());
        assert!(!store.path().exists());
        // Talking about tokens without one is fine.
        store
            .remember(true, "Rotates API tokens every Friday")
            .expect("test");
        assert_eq!(
            store.facts().expect("test"),
            vec!["Rotates API tokens every Friday"]
        );
    }

    #[test]
    fn f_cn_06_remember_forget_clear() {
        let dir = tempfile::tempdir().expect("test");
        let store = MemoryStore::new(dir.path().join("conductor"));
        store
            .remember(true, "remember that I work on billing")
            .expect("test");
        store.remember(true, "Prefers  Rust").expect("test");
        store.remember(true, "prefers rust").expect("test"); // duplicate
        assert_eq!(
            store.facts().expect("test"),
            vec!["I work on billing", "Prefers Rust"]
        );
        let text = store.text().expect("test");
        assert!(text.starts_with("# Conductor memory"));
        assert_eq!(store.forget("billing").expect("test"), 1);
        assert_eq!(store.facts().expect("test"), vec!["Prefers Rust"]);
        store
            .journal(true, "2026-10-10", "Launched 2 agents")
            .expect("test");
        store.clear().expect("test");
        assert!(!store.path().exists());
        assert!(!store.journal_dir().exists());
        assert!(store.facts().expect("test").is_empty());
    }

    #[test]
    fn f_cn_06_user_edits_survive_new_facts() {
        let dir = tempfile::tempdir().expect("test");
        let store = MemoryStore::new(dir.path());
        store
            .save_text(true, "# Mine\n\nNotes I wrote.\n\n- Works nights\n")
            .expect("test");
        store.remember(true, "Uses Neovim").expect("test");
        let text = store.text().expect("test");
        assert!(text.starts_with("# Mine\n\nNotes I wrote.\n"), "{text}");
        assert_eq!(
            store.facts().expect("test"),
            vec!["Works nights", "Uses Neovim"]
        );
    }

    #[test]
    fn f_cn_06_reflection_ask_auto_off() {
        let dir = tempfile::tempdir().expect("test");
        let store = MemoryStore::new(dir.path());
        let facts = vec![
            "Works on the API".to_string(),
            "Likes dark mode".to_string(),
        ];
        assert!(store
            .reflect(true, Reflection::Off, &facts)
            .expect("test")
            .is_empty());
        assert_eq!(
            store
                .reflect(true, Reflection::Ask, &facts)
                .expect("test")
                .len(),
            2
        );
        assert!(store.facts().expect("test").is_empty());
        let cards = store.proposals().expect("test");
        assert_eq!(cards.len(), 2);
        // Asking again doesn't duplicate the cards.
        assert!(store
            .reflect(true, Reflection::Ask, &facts)
            .expect("test")
            .is_empty());
        store
            .resolve(true, &[cards[0].id.clone()], true)
            .expect("test");
        store
            .resolve(true, &[cards[1].id.clone()], false)
            .expect("test");
        assert_eq!(store.facts().expect("test"), vec!["Works on the API"]);
        assert!(store.proposals().expect("test").is_empty());
        store
            .reflect(true, Reflection::Auto, &["Ships on Fridays".into()])
            .expect("test");
        assert_eq!(store.facts().expect("test").len(), 2);
    }

    #[test]
    fn f_cn_06_call_context_reads_persona_and_memory_once() {
        let dir = tempfile::tempdir().expect("test");
        let store = MemoryStore::new(dir.path());
        store
            .remember(true, "Works on the billing service")
            .expect("test");
        let get = settings(&[
            (SettingKey::ConductorName, json!("Ada")),
            (SettingKey::ConductorTone, json!("warm")),
            (SettingKey::ConductorVerbosity, json!("short")),
            (SettingKey::ConductorSpeed, json!(5)),
            (SettingKey::ConductorAboutYou, json!("Backend engineer.")),
            (SettingKey::ConductorMemory, json!(true)),
        ]);
        let ctx = CallContext::from_settings(get, &store);
        assert_eq!(ctx.persona.name, "Ada");
        assert_eq!(ctx.persona.tone, Tone::Warm);
        assert_eq!(ctx.persona.verbosity, Verbosity::Short);
        assert_eq!(ctx.persona.speed, 5);
        let i = ctx.instructions();
        assert!(i.contains("Your name is Ada."));
        assert!(i.contains("Be warm"));
        assert!(i.contains("Speak quickly."));
        assert!(i.contains("Backend engineer."));
        assert!(i.contains("- Works on the billing service"));
    }

    #[test]
    fn f_cn_06_defaults_and_old_values() {
        // An older build stored "friendly" and a 75–150 speed: defaults win.
        let get = settings(&[
            (SettingKey::ConductorTone, json!("friendly")),
            (SettingKey::ConductorSpeed, json!(120)),
            (SettingKey::ConductorName, json!("  ")),
        ]);
        let p = Persona::from_settings(get);
        assert_eq!(p, Persona::default());
    }

    #[test]
    fn f_cn_06_memory_in_call_is_capped() {
        let dir = tempfile::tempdir().expect("test");
        let store = MemoryStore::new(dir.path());
        for i in 0..100 {
            store
                .remember(true, &format!("Fact number {i} {}", "x".repeat(80)))
                .expect("test");
        }
        let ctx = CallContext::from_settings(
            settings(&[(SettingKey::ConductorMemory, json!(true))]),
            &store,
        );
        let m = ctx.memory.expect("test");
        assert!(m.len() <= MEMORY_IN_CALL_MAX);
        assert!(m.starts_with("- Fact number 0 "));
    }
}
