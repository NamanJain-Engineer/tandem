//! Teammates (F-AG-01, F-AG-02, F-AG-05; ARCHITECTURE §8.1): the record,
//! its Home folder, the Lead made on first use, and the rail's summaries.
//! Names are unique case-insensitively; the Free plan has one teammate
//! besides the Lead; a deleted teammate's files go to the trash.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::ids::{AccountId, TeammateId};
use tandem_core::settings::SettingKey;
use tandem_core::teammates::{self as rules, TeammateSettings};
use tandem_core::{DataDir, ErrorCode, IpcError};
use tandem_store::repo::teammates::{self as repo, NewTeammate, Teammate, TeammateUpdate};
use tandem_store::Store;
use ts_rs::TS;

/// What the teammate service needs to know about engines.
pub trait EngineFacts: Send + Sync {
    /// Has a chat adapter (memory, skills and the gateway).
    fn chat_capable(&self, engine_id: &str) -> bool;
    fn installed(&self, engine_id: &str) -> bool;
    fn known(&self, engine_id: &str) -> bool;
    /// The engine a new teammate starts with.
    fn default_engine(&self) -> String;
}

/// `teammate_create`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TeammateInput {
    pub name: String,
    #[serde(default)]
    pub brief: String,
    pub engine_id: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub preferred_account_id: Option<AccountId>,
    #[serde(default)]
    pub places: Vec<String>,
    #[serde(default)]
    pub avatar_hue: Option<u8>,
    #[serde(default)]
    pub settings: Option<TeammateSettings>,
}

/// `teammate_update`: only the fields given change. `model`, `effort`,
/// `preferredAccountId` and `avatarHue` take `null` to clear.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TeammatePatch {
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub brief: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub engine_id: Option<String>,
    #[serde(default, with = "double_option")]
    #[ts(optional, type = "string | null")]
    pub model: Option<Option<String>>,
    #[serde(default, with = "double_option")]
    #[ts(optional, type = "string | null")]
    pub effort: Option<Option<String>>,
    #[serde(default, with = "double_option")]
    #[ts(optional, type = "string | null")]
    pub preferred_account_id: Option<Option<AccountId>>,
    #[serde(default, with = "double_option")]
    #[ts(optional, type = "number | null")]
    pub avatar_hue: Option<Option<u8>>,
    #[serde(default)]
    #[ts(optional)]
    pub places: Option<Vec<String>>,
    #[serde(default)]
    #[ts(optional)]
    pub settings: Option<TeammateSettings>,
    #[serde(default)]
    #[ts(optional)]
    pub pinned: Option<bool>,
}

/// `null` and missing are different for a patch: missing leaves the field,
/// `null` clears it.
mod double_option {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<T: Serialize, S: Serializer>(
        v: &Option<Option<T>>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        match v {
            Some(inner) => inner.serialize(s),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<Option<T>>, D::Error> {
        Option::<T>::deserialize(d).map(Some)
    }
}

/// A rail row (F-AG-02, UI-70): the teammate and how its chats are doing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TeammateSummary {
    pub teammate: Teammate,
    /// The engine has a chat adapter: chats, memory and skills.
    pub chat_capable: bool,
    /// Chats with a turn running.
    pub working: u32,
    /// A chat waits on the user (an approval).
    pub waiting: bool,
    /// The latest chat's last turn failed.
    pub failed: bool,
    /// The last thing the teammate said, one line.
    pub last_line: Option<String>,
    pub last_at: Option<i64>,
    pub chats: u32,
}

/// `teammates:changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TeammatesChanged {
    pub teammates: Vec<TeammateSummary>,
}

fn invalid(msg: impl Into<String>) -> IpcError {
    IpcError::new(ErrorCode::Invalid, msg)
}

fn store_err(e: tandem_store::StoreError) -> IpcError {
    e.into()
}

fn not_found() -> IpcError {
    IpcError::new(ErrorCode::NotFound, "This teammate doesn't exist")
}

/// `<home>/memory/{NOTES,ABOUT_YOU}.md` and `<home>/skills/`, owner-only.
pub fn make_home(home: &Path) -> std::io::Result<()> {
    tandem_core::paths::create_private_dir(home)?;
    let memory = home.join("memory");
    std::fs::create_dir_all(&memory)?;
    std::fs::create_dir_all(home.join("skills"))?;
    for file in ["NOTES.md", "ABOUT_YOU.md"] {
        let path = memory.join(file);
        if !path.exists() {
            tandem_core::atomic::atomic_write(&path, b"", Some(0o600))?;
        }
    }
    Ok(())
}

/// Three things to ask a teammate, from its brief's first sentences, made
/// locally (F-AG-03): "You draft our weekly changelog." → "Draft our weekly
/// changelog". Constraints ("Never…", "Don't…") aren't tasks; the rest is
/// filled with general asks.
pub fn suggestions(brief: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let sentences = brief
        .split(['.', '!', '?', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty());
    for s in sentences {
        let lower = s.to_lowercase();
        if ["never ", "don't ", "do not ", "avoid ", "always "]
            .iter()
            .any(|p| lower.starts_with(p))
        {
            continue;
        }
        let task = s
            .strip_prefix("You ")
            .or_else(|| s.strip_prefix("you "))
            .unwrap_or(s);
        let mut chars = task.chars();
        let Some(first) = chars.next() else { continue };
        let chip: String = first.to_uppercase().chain(chars).collect();
        let chip: String = chip.chars().take(80).collect();
        if chip.split_whitespace().count() >= 2 && !out.contains(&chip) {
            out.push(chip);
        }
        if out.len() == 3 {
            return out;
        }
    }
    for general in [
        "What can you take off my plate today?",
        "Summarize what you know about this work",
        "Suggest what we should do next",
    ] {
        if out.len() == 3 {
            break;
        }
        out.push(general.to_string());
    }
    out
}

/// Copies a folder's files (a duplicate's memory and skills).
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            if entry.file_name() == ".history" {
                continue;
            }
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// A name for a copy: "{name} copy", then "{name} copy 2" …, within 40.
fn copy_name(name: &str, taken: &[String]) -> String {
    let fold: Vec<String> = taken.iter().map(|t| rules::fold_name(t)).collect();
    for n in 1.. {
        let suffix = if n == 1 {
            " copy".to_string()
        } else {
            format!(" copy {n}")
        };
        let room = rules::NAME_MAX.saturating_sub(suffix.chars().count());
        let base: String = name.chars().take(room).collect();
        let candidate = format!("{}{suffix}", base.trim_end());
        if !fold.contains(&rules::fold_name(&candidate)) {
            return candidate;
        }
    }
    unreachable!("the loop returns")
}

/// Whether text holds a credential-shaped string (the log redactor's rules).
pub fn has_secret(text: &str) -> bool {
    tandem_store::memory::has_credentials(text)
}

/// The app's engines (detection and the registry).
pub struct AppEngineFacts(pub Arc<crate::services::terminal::TerminalService>);

impl EngineFacts for AppEngineFacts {
    fn chat_capable(&self, engine_id: &str) -> bool {
        self.0
            .engines
            .get(engine_id)
            .is_some_and(|s| crate::services::threads::has_chat_adapter(&s))
    }

    fn installed(&self, engine_id: &str) -> bool {
        self.0.engines.installed(engine_id).is_some()
    }

    fn known(&self, engine_id: &str) -> bool {
        self.0.engines.get(engine_id).is_some()
    }

    /// Claude Code when it's installed, else the first installed chat
    /// engine, else Claude Code (the Lead says it needs installing).
    fn default_engine(&self) -> String {
        let engines = &self.0.engines;
        let registry = engines.registry();
        let chat: Vec<String> = registry
            .all()
            .iter()
            .filter(|e| {
                crate::services::threads::has_chat_adapter(e) && engines.installed(&e.id).is_some()
            })
            .map(|e| e.id.clone())
            .collect();
        chat.iter()
            .find(|id| *id == "claude")
            .or_else(|| chat.first())
            .cloned()
            .unwrap_or_else(|| "claude".into())
    }
}

pub struct TeammatesService {
    store: Store,
    bus: EventBus,
    data: DataDir,
    engines: Arc<dyn EngineFacts>,
    /// The Free plan's limit (teammates besides the Lead), read per create.
    limit: Arc<dyn Fn() -> Option<u32> + Send + Sync>,
    lead: tokio::sync::OnceCell<()>,
    /// A publish is already on its way (chat state changes come in bursts).
    publishing: std::sync::atomic::AtomicBool,
}

impl TeammatesService {
    pub fn new(
        store: Store,
        bus: EventBus,
        data: DataDir,
        engines: Arc<dyn EngineFacts>,
        limit: Arc<dyn Fn() -> Option<u32> + Send + Sync>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            bus,
            data,
            engines,
            limit,
            lead: tokio::sync::OnceCell::new(),
            publishing: std::sync::atomic::AtomicBool::new(false),
        })
    }

    pub fn data(&self) -> &DataDir {
        &self.data
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// The engine has a chat adapter (memory, skills, mail).
    pub fn chat_capable(&self, engine_id: &str) -> bool {
        self.engines.chat_capable(engine_id)
    }

    /// The Lead, made once per profile (a deleted Lead isn't made again).
    pub async fn ensure_lead(&self) {
        let _ = self
            .lead
            .get_or_init(|| async {
                match repo::any_ever(&self.store).await {
                    Ok(false) => {
                        let id = TeammateId::new();
                        let home = self.data.teammate_home(id.as_str());
                        if let Err(e) = make_home(&home) {
                            tracing::warn!(code = "teammates.home_failed", error = %e, "couldn't make the Lead's home");
                            return;
                        }
                        let made = repo::insert(
                            &self.store,
                            NewTeammate {
                                id,
                                name: rules::LEAD_NAME.into(),
                                avatar_hue: None,
                                brief: rules::LEAD_BRIEF.into(),
                                engine_id: self.engines.default_engine(),
                                model: None,
                                effort: None,
                                preferred_account_id: None,
                                home: home.to_string_lossy().into_owned(),
                                places: Vec::new(),
                                settings: TeammateSettings::default(),
                                is_lead: true,
                                pinned: true,
                            },
                        )
                        .await;
                        if let Err(e) = made {
                            tracing::warn!(code = "teammates.lead_failed", error = %e, "couldn't make the Lead");
                        }
                    }
                    Ok(true) => {}
                    Err(e) => {
                        tracing::warn!(code = "teammates.lead_failed", error = %e, "couldn't look for the Lead");
                    }
                }
            })
            .await;
    }

    pub async fn get(&self, id: &TeammateId) -> Result<Teammate, IpcError> {
        repo::get(&self.store, id)
            .await
            .map_err(store_err)?
            .ok_or_else(not_found)
    }

    pub async fn teammates(&self) -> Result<Vec<Teammate>, IpcError> {
        self.ensure_lead().await;
        repo::list(&self.store).await.map_err(store_err)
    }

    /// The rail: every teammate with its chats' state.
    pub async fn list(&self) -> Result<Vec<TeammateSummary>, IpcError> {
        let teammates = self.teammates().await?;
        let stats = tandem_store::repo::threads::teammate_stats(&self.store)
            .await
            .map_err(store_err)?;
        let mut by: HashMap<String, tandem_store::repo::threads::TeammateChatStats> = stats
            .into_iter()
            .map(|s| (s.teammate_id.as_str().to_string(), s))
            .collect();
        Ok(teammates
            .into_iter()
            .map(|t| {
                let s = by.remove(t.id.as_str()).unwrap_or_default();
                TeammateSummary {
                    chat_capable: self.engines.chat_capable(&t.engine_id),
                    working: s.working,
                    waiting: s.waiting,
                    failed: s.failed,
                    last_line: s.last_line,
                    last_at: s.last_at,
                    chats: s.chats,
                    teammate: t,
                }
            })
            .collect())
    }

    /// Tells the views (`teammates:changed`).
    pub async fn publish(&self) {
        match self.list().await {
            Ok(teammates) => self.bus.publish(BusEvent::new(
                Topic::Threads,
                "teammates:changed",
                TeammatesChanged { teammates },
            )),
            Err(e) => {
                tracing::warn!(code = "teammates.publish_failed", error = %e.message, "couldn't list teammates");
            }
        }
    }

    /// Publishes once for a burst of chat changes (100 ms).
    pub fn publish_soon(self: &Arc<Self>) {
        use std::sync::atomic::Ordering;
        if self.publishing.swap(true, Ordering::SeqCst) {
            return;
        }
        let svc = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            svc.publishing.store(false, Ordering::SeqCst);
            svc.publish().await;
        });
    }

    /// Where a new chat works: Home, or one of the teammate's places.
    pub fn place_for(teammate: &Teammate, place: Option<&str>) -> Result<String, IpcError> {
        match place {
            None => Ok(teammate.home.clone()),
            Some(p) if p == teammate.home || teammate.places.iter().any(|x| x == p) => {
                Ok(p.to_string())
            }
            Some(_) => Err(invalid(
                "A chat works in the teammate's Home or one of its places",
            )),
        }
    }

    fn check_brief(brief: &str) -> Result<(), IpcError> {
        if brief.chars().count() > rules::BRIEF_MAX {
            return Err(invalid("Briefs are at most 8,000 characters"));
        }
        Ok(())
    }

    fn check_engine(&self, engine_id: &str) -> Result<(), IpcError> {
        if !self.engines.known(engine_id) {
            return Err(IpcError::new(
                ErrorCode::EngineMissing,
                "This engine isn't known to Vibezeo",
            ));
        }
        if !self.engines.installed(engine_id) {
            return Err(IpcError::new(
                ErrorCode::EngineMissing,
                "Install this engine first, then check again",
            ));
        }
        Ok(())
    }

    fn check_places(places: &[String]) -> Result<Vec<String>, IpcError> {
        let mut out: Vec<String> = Vec::new();
        for p in places {
            let path = PathBuf::from(p);
            if !path.is_absolute() {
                return Err(invalid("Places are full folder paths"));
            }
            if !out.contains(p) {
                out.push(p.clone());
            }
        }
        Ok(out)
    }

    /// `teammate_create` (F-AG-01, F-AG-04): nothing exists until this.
    pub async fn create(&self, input: TeammateInput) -> Result<Teammate, IpcError> {
        self.ensure_lead().await;
        let name = rules::validate_name(&input.name).map_err(invalid)?;
        Self::check_brief(&input.brief)?;
        // A taken name says so before the plan's limit does.
        if repo::name_used(&self.store, &name)
            .await
            .map_err(store_err)?
        {
            return Err(IpcError::new(ErrorCode::Conflict, repo::NAME_USED));
        }
        if let Some(limit) = (self.limit)() {
            let have = repo::count_besides_lead(&self.store)
                .await
                .map_err(store_err)?;
            if have >= limit {
                return Err(IpcError::new(
                    ErrorCode::LimitReached,
                    "Free includes one teammate besides Lead.",
                ));
            }
        }
        self.check_engine(&input.engine_id)?;
        let places = Self::check_places(&input.places)?;
        let id = TeammateId::new();
        let home = self.data.teammate_home(id.as_str());
        let teammate = repo::insert(
            &self.store,
            NewTeammate {
                id,
                name,
                avatar_hue: input.avatar_hue.filter(|h| *h < rules::HUES),
                brief: input.brief,
                engine_id: input.engine_id,
                model: input.model,
                effort: input.effort,
                preferred_account_id: input.preferred_account_id,
                home: home.to_string_lossy().into_owned(),
                places,
                settings: input.settings.unwrap_or_default(),
                is_lead: false,
                pinned: false,
            },
        )
        .await
        .map_err(store_err)?;
        make_home(&home).map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
        tracing::info!(code = "teammates.created", engine = %teammate.engine_id, "teammate created");
        self.publish().await;
        Ok(teammate)
    }

    /// `teammate_update` (F-AG-05). A new engine applies to the next chat;
    /// memory and skills are files and stay as they are.
    pub async fn update(
        &self,
        id: &TeammateId,
        patch: TeammatePatch,
    ) -> Result<Teammate, IpcError> {
        let before = self.get(id).await?;
        let name = patch
            .name
            .as_deref()
            .map(rules::validate_name)
            .transpose()
            .map_err(invalid)?;
        if let Some(b) = &patch.brief {
            Self::check_brief(b)?;
        }
        if let Some(e) = &patch.engine_id {
            if *e != before.engine_id {
                self.check_engine(e)?;
            }
        }
        let places = patch
            .places
            .as_deref()
            .map(Self::check_places)
            .transpose()?;
        if patch.pinned == Some(true) {
            // One hero tile: pinning a teammate unpins the others.
            for other in repo::list(&self.store).await.map_err(store_err)? {
                if other.pinned && other.id != *id {
                    repo::update(
                        &self.store,
                        &other.id,
                        TeammateUpdate {
                            pinned: Some(false),
                            ..Default::default()
                        },
                    )
                    .await
                    .map_err(store_err)?;
                }
            }
        }
        let updated = repo::update(
            &self.store,
            id,
            TeammateUpdate {
                name,
                avatar_hue: patch.avatar_hue.map(|h| h.filter(|h| *h < rules::HUES)),
                avatar_image: None,
                brief: patch.brief,
                engine_id: patch.engine_id,
                model: patch.model,
                effort: patch.effort,
                preferred_account_id: patch.preferred_account_id,
                places,
                settings: patch.settings,
                pinned: patch.pinned,
                sort_key: None,
            },
        )
        .await
        .map_err(store_err)?
        .ok_or_else(not_found)?;
        self.publish().await;
        Ok(updated)
    }

    /// `teammate_delete`: the row stays for its chats ("Deleted teammate");
    /// Home (memory and skills) moves to `$DATA/trash/teammates/<id>-<ts>`.
    pub async fn delete(&self, id: &TeammateId) -> Result<(), IpcError> {
        let teammate = self.get(id).await?;
        let trash = self.data.trash().join("teammates");
        tandem_core::paths::create_private_dir(&trash)
            .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
        let home = PathBuf::from(&teammate.home);
        if home.exists() {
            let to = trash.join(format!("{}-{}", id.as_str(), tandem_store::now_ms()));
            if std::fs::rename(&home, &to).is_err() {
                // Another filesystem: copy, then remove.
                copy_tree(&home, &to).map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
                std::fs::remove_dir_all(&home)
                    .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
            }
        }
        repo::soft_delete(&self.store, id)
            .await
            .map_err(store_err)?;
        tandem_store::repo::plugins::clear_grants(
            &self.store,
            &tandem_store::repo::plugins::GrantSubject::Teammate {
                teammate_id: id.as_str().to_string(),
            },
        )
        .await
        .map_err(store_err)?;
        tracing::info!(code = "teammates.deleted", "teammate deleted");
        self.publish().await;
        Ok(())
    }

    /// `teammate_duplicate`: the same brief, engine, settings, memory and
    /// skills under a new name; chats and grants aren't copied.
    pub async fn duplicate(&self, id: &TeammateId) -> Result<Teammate, IpcError> {
        let source = self.get(id).await?;
        let taken: Vec<String> = repo::list(&self.store)
            .await
            .map_err(store_err)?
            .into_iter()
            .map(|t| t.name)
            .collect();
        let copy = self
            .create(TeammateInput {
                name: copy_name(&source.name, &taken),
                brief: source.brief.clone(),
                engine_id: source.engine_id.clone(),
                model: source.model.clone(),
                effort: source.effort.clone(),
                preferred_account_id: source.preferred_account_id.clone(),
                places: source.places.clone(),
                avatar_hue: None,
                settings: Some(source.settings),
            })
            .await?;
        let from = PathBuf::from(&source.home);
        let to = PathBuf::from(&copy.home);
        for sub in ["memory", "skills"] {
            if from.join(sub).is_dir() {
                copy_tree(&from.join(sub), &to.join(sub))
                    .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
            }
        }
        Ok(copy)
    }

    /// The skills the user made or opened, per teammate (`skills_seen`).
    pub async fn seen_skills(&self, id: &TeammateId) -> Vec<String> {
        tandem_store::repo::settings::get(&self.store, SettingKey::SkillsSeen)
            .await
            .ok()
            .and_then(|v| v.get(id.as_str()).cloned())
            .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
            .unwrap_or_default()
    }

    /// Marks skills as seen (the user made, added or opened them).
    pub async fn mark_skills_seen(&self, id: &TeammateId, slugs: &[String]) {
        let mut all = tandem_store::repo::settings::get(&self.store, SettingKey::SkillsSeen)
            .await
            .ok()
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        let mut mine: Vec<String> = all
            .get(id.as_str())
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        let before = mine.len();
        for s in slugs {
            if !mine.contains(s) {
                mine.push(s.clone());
            }
        }
        if mine.len() == before {
            return;
        }
        all.insert(id.as_str().to_string(), serde_json::json!(mine));
        if let Err(e) = tandem_store::repo::settings::set(
            &self.store,
            SettingKey::SkillsSeen,
            serde_json::Value::Object(all),
        )
        .await
        {
            tracing::warn!(code = "teammates.skills_seen_failed", error = %e, "couldn't remember seen skills");
        }
    }

    /// "Allow teammates to write skills" (Settings › Teammates).
    pub async fn teammates_write_skills(&self) -> bool {
        tandem_store::repo::settings::get(&self.store, SettingKey::AllowTeammateSkills)
            .await
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
    }

    /// A teammate's skills, with their validation and "New" state.
    pub async fn skills(&self, teammate: &Teammate) -> Vec<tandem_store::skills::SkillEntry> {
        let seen = self.seen_skills(&teammate.id).await;
        let allow = self.teammates_write_skills().await;
        let home = PathBuf::from(&teammate.home);
        tokio::task::spawn_blocking(move || tandem_store::skills::list(&home, &seen, allow))
            .await
            .unwrap_or_default()
    }

    /// The briefing a teammate chat's session starts with (F-AG-06).
    pub async fn briefing(
        &self,
        teammate: &Teammate,
        workspace: &Path,
        access: tandem_core::access::Access,
        unattended: bool,
        date: &str,
    ) -> tandem_engines::briefing::Briefing {
        use tandem_engines::briefing::{assemble, BriefingInput, RosterLine, SkillLine, Tools};
        let home = PathBuf::from(&teammate.home);
        let memory = tandem_store::memory::read(&home, teammate.settings.memory_cap);
        let skills: Vec<SkillLine> = self
            .skills(teammate)
            .await
            .into_iter()
            .filter(|s| s.valid)
            .map(|s| SkillLine {
                slug: s.slug,
                description: s.description,
            })
            .collect();
        let roster: Option<Vec<RosterLine>> = if teammate.settings.may_message {
            Some(
                repo::list(&self.store)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|t| t.id != teammate.id && self.engines.chat_capable(&t.engine_id))
                    .map(|t| RosterLine {
                        about: t.brief.lines().next().unwrap_or("").to_string(),
                        name: t.name,
                    })
                    .collect(),
            )
        } else {
            None
        };
        let briefing = assemble(&BriefingInput {
            name: &teammate.name,
            date,
            workspace,
            access,
            unattended,
            brief: &teammate.brief,
            notes: &memory.notes,
            about: &memory.about,
            notes_cap: teammate.settings.memory_cap as usize,
            about_cap: rules::ABOUT_CAP as usize,
            skills: &skills,
            roster: roster.as_deref(),
            tools: Tools {
                memory: true,
                session_search: true,
                skills: true,
                mail: teammate.settings.may_message,
                schedule: false,
            },
        });
        if briefing.injection_lines > 0 {
            tracing::warn!(
                code = "teammates.briefing_defanged",
                lines = briefing.injection_lines,
                "memory or skill lines looked like instructions and were defanged"
            );
        }
        briefing
    }

    /// `memory_get` (F-AG-07).
    pub async fn memory(&self, id: &TeammateId) -> Result<tandem_store::memory::Memory, IpcError> {
        let t = self.get(id).await?;
        let home = PathBuf::from(&t.home);
        let cap = t.settings.memory_cap;
        tokio::task::spawn_blocking(move || tandem_store::memory::read(&home, cap))
            .await
            .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))
    }

    /// A checked memory write (`memory_write`, `tandem__memory_write`): over
    /// the cap is LimitReached ("…Condense it and try again."), a credential
    /// is Invalid ("Memory can't store credentials"); the file is untouched
    /// either way. The change is recorded for Undo; its id is returned.
    pub async fn write_memory(
        &self,
        teammate: &Teammate,
        file: tandem_store::memory::MemoryFile,
        markdown: &str,
    ) -> Result<String, IpcError> {
        use tandem_store::memory::{self, MemoryError};
        let change = format!("mc_{}", uuid::Uuid::new_v4().simple());
        let home = PathBuf::from(&teammate.home);
        let md = markdown.to_string();
        let cap = teammate.settings.memory_cap;
        let id = change.clone();
        let written =
            tokio::task::spawn_blocking(move || memory::write(&home, file, &md, cap, &id))
                .await
                .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?
                .map_err(|e| match e {
                    MemoryError::OverCap { .. } => {
                        IpcError::new(ErrorCode::LimitReached, e.to_string())
                    }
                    MemoryError::Credentials => invalid(e.to_string()),
                    MemoryError::Io(m) => IpcError::new(ErrorCode::Io, m),
                })?;
        tandem_store::repo::memory_changes::insert(
            &self.store,
            tandem_store::repo::memory_changes::MemoryChange {
                id: change.clone(),
                teammate_id: teammate.id.clone(),
                file: file.as_str().to_string(),
                before_hash: written.before_hash,
                after_hash: written.after_hash,
                before_path: written.backup.to_string_lossy().into_owned(),
                created_at: 0,
            },
        )
        .await
        .map_err(store_err)?;
        tracing::info!(
            code = "teammates.memory_written",
            file = file.as_str(),
            "memory written"
        );
        self.publish().await;
        Ok(change)
    }

    /// `memory_undo`: puts back what a change replaced, unless the file has
    /// changed again since.
    pub async fn undo_memory(&self, change_id: &str) -> Result<(), IpcError> {
        use tandem_store::memory::{self, MemoryFile};
        use tandem_store::repo::memory_changes;
        let change = memory_changes::get(&self.store, change_id)
            .await
            .map_err(store_err)?
            .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "There's nothing to undo"))?;
        let teammate = self.get(&change.teammate_id).await?;
        let file = MemoryFile::parse(&change.file)
            .ok_or_else(|| IpcError::new(ErrorCode::Internal, "unknown memory file"))?;
        let home = PathBuf::from(&teammate.home);
        let now = std::fs::read_to_string(memory::path(&home, file)).unwrap_or_default();
        let now_hash = {
            use sha2::{Digest, Sha256};
            hex::encode(Sha256::digest(now.as_bytes()))
        };
        if now_hash != change.after_hash {
            return Err(IpcError::new(
                ErrorCode::Conflict,
                "Memory changed since, so this can't be undone",
            ));
        }
        let before = std::fs::read_to_string(&change.before_path)
            .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
        tandem_core::atomic::atomic_write(
            &memory::path(&home, file),
            before.as_bytes(),
            Some(0o600),
        )
        .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
        memory_changes::delete(&self.store, change_id)
            .await
            .map_err(store_err)?;
        self.publish().await;
        Ok(())
    }

    /// `tandem__read_skill`: the full SKILL.md, fenced as data (F-AG-06).
    pub async fn read_skill_for_agent(
        &self,
        teammate: &Teammate,
        slug: &str,
    ) -> Result<String, IpcError> {
        let seen = self.seen_skills(&teammate.id).await;
        let allow = self.teammates_write_skills().await;
        let skill = tandem_store::skills::read(&PathBuf::from(&teammate.home), slug, &seen, allow)
            .map_err(|e| IpcError::new(ErrorCode::NotFound, e.to_string()))?;
        if !skill.entry.valid {
            return Err(invalid(format!(
                "{slug} can't be used: {}",
                skill.entry.error.unwrap_or_default()
            )));
        }
        let mut hits = 0;
        let text = tandem_engines::briefing::fence_text(&skill.markdown, &mut hits);
        Ok(format!(
            "{}\n<skill name=\"{slug}\">\n{text}\n</skill>",
            tandem_core::untrusted::label(&format!("the skill {slug}"))
        ))
    }

    /// `teammate_reorder`: the rail's custom order.
    pub async fn reorder(&self, ids: &[TeammateId]) -> Result<(), IpcError> {
        repo::reorder(&self.store, ids).await.map_err(store_err)?;
        self.publish().await;
        Ok(())
    }
}

/// Test helpers other services' tests share.
#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;

    pub struct Engines;

    impl EngineFacts for Engines {
        fn chat_capable(&self, id: &str) -> bool {
            matches!(id, "claude" | "codex")
        }
        fn installed(&self, id: &str) -> bool {
            matches!(id, "claude" | "aider")
        }
        fn known(&self, id: &str) -> bool {
            matches!(id, "claude" | "codex" | "aider")
        }
        fn default_engine(&self) -> String {
            "claude".into()
        }
    }

    pub async fn service(limit: Option<u32>) -> (tempfile::TempDir, Arc<TeammatesService>) {
        let dir = tempfile::tempdir().expect("dir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let svc = TeammatesService::new(
            store,
            EventBus::new(),
            data,
            Arc::new(Engines),
            Arc::new(move || limit),
        );
        (dir, svc)
    }

    pub fn input(name: &str, engine: &str) -> TeammateInput {
        TeammateInput {
            name: name.into(),
            brief: "Keeps the changelog.".into(),
            engine_id: engine.into(),
            model: None,
            effort: None,
            preferred_account_id: None,
            places: Vec::new(),
            avatar_hue: None,
            settings: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::tests_support::{input, service};

    #[tokio::test]
    async fn f_ag_01_3_a_fresh_profile_has_a_pinned_lead_with_the_default_brief() {
        let (_d, svc) = service(None).await;
        let list = svc.list().await.expect("list");
        assert_eq!(list.len(), 1);
        let lead = &list[0].teammate;
        assert_eq!(lead.name, "Lead");
        assert!(lead.pinned && lead.is_lead);
        assert_eq!(lead.brief, rules::LEAD_BRIEF);
        let home = PathBuf::from(&lead.home);
        assert!(home.join("memory/NOTES.md").is_file());
        assert!(home.join("memory/ABOUT_YOU.md").is_file());
        assert!(home.join("skills").is_dir());
        // A deleted Lead isn't made again.
        svc.delete(&lead.id).await.expect("delete");
        let (_d2, again) = (_d, svc.clone());
        assert!(again.list().await.expect("list").is_empty());
    }

    #[tokio::test]
    async fn f_ag_01_1_scout_then_scout_is_refused_with_name_already_used() {
        let (_d, svc) = service(None).await;
        svc.create(input("Scout", "claude")).await.expect("scout");
        let err = svc.create(input("scout", "claude")).await.expect_err("dup");
        assert_eq!(err.code, ErrorCode::Conflict);
        assert_eq!(err.message, "Name already used");
        // On Free, at the limit, the name is still what's wrong.
        let (_d2, free) = service(Some(1)).await;
        free.create(input("Scout", "claude")).await.expect("scout");
        let err = free
            .create(input("SCOUT", "claude"))
            .await
            .expect_err("dup");
        assert_eq!(err.message, "Name already used");
        let err = svc.create(input("  ", "claude")).await.expect_err("empty");
        assert_eq!(err.code, ErrorCode::Invalid);
        let err = svc
            .create(input(&"n".repeat(41), "claude"))
            .await
            .expect_err("long");
        assert_eq!(err.code, ErrorCode::Invalid);
    }

    #[tokio::test]
    async fn f_ag_04_1_an_uninstalled_engine_cant_make_a_teammate() {
        let (_d, svc) = service(None).await;
        let err = svc
            .create(input("Ops", "codex"))
            .await
            .expect_err("missing");
        assert_eq!(err.code, ErrorCode::EngineMissing);
        // A terminal-only engine is allowed, without chats.
        svc.create(input("Old school", "aider"))
            .await
            .expect("terminal");
        let list = svc.list().await.expect("list");
        let old = list
            .iter()
            .find(|s| s.teammate.name == "Old school")
            .expect("row");
        assert!(!old.chat_capable);
    }

    #[tokio::test]
    async fn f_ag_01_the_free_plan_has_one_teammate_besides_the_lead() {
        let (_d, svc) = service(Some(1)).await;
        svc.create(input("Scout", "claude")).await.expect("first");
        let err = svc
            .create(input("Ranger", "claude"))
            .await
            .expect_err("limit");
        assert_eq!(err.code, ErrorCode::LimitReached);
        assert_eq!(err.message, "Free includes one teammate besides Lead.");
    }

    #[tokio::test]
    async fn f_ag_02_delete_moves_home_to_the_trash_and_frees_the_name() {
        let (d, svc) = service(None).await;
        let scout = svc.create(input("Scout", "claude")).await.expect("scout");
        let home = PathBuf::from(&scout.home);
        std::fs::write(home.join("memory/NOTES.md"), "- uses pnpm\n").expect("notes");
        svc.delete(&scout.id).await.expect("delete");
        assert!(!home.exists());
        let trashed: Vec<_> = std::fs::read_dir(d.path().join("trash/teammates"))
            .expect("trash")
            .flatten()
            .collect();
        assert_eq!(trashed.len(), 1);
        assert_eq!(
            std::fs::read_to_string(trashed[0].path().join("memory/NOTES.md")).expect("kept"),
            "- uses pnpm\n"
        );
        svc.create(input("scout", "claude"))
            .await
            .expect("name is free");
    }

    #[tokio::test]
    async fn f_ag_02_duplicate_copies_memory_and_skills_under_a_new_name() {
        let (_d, svc) = service(None).await;
        let scout = svc.create(input("Scout", "claude")).await.expect("scout");
        let home = PathBuf::from(&scout.home);
        std::fs::write(home.join("memory/NOTES.md"), "- one\n").expect("notes");
        std::fs::create_dir_all(home.join("skills/test-first")).expect("skill");
        std::fs::write(home.join("skills/test-first/SKILL.md"), "x").expect("skill");
        let copy = svc.duplicate(&scout.id).await.expect("dup");
        assert_eq!(copy.name, "Scout copy");
        assert_ne!(copy.home, scout.home);
        let to = PathBuf::from(&copy.home);
        assert_eq!(
            std::fs::read_to_string(to.join("memory/NOTES.md")).expect("n"),
            "- one\n"
        );
        assert!(to.join("skills/test-first/SKILL.md").is_file());
        let again = svc.duplicate(&scout.id).await.expect("dup");
        assert_eq!(again.name, "Scout copy 2");
    }

    #[tokio::test]
    async fn f_ag_05_1_a_new_engine_leaves_memory_alone_and_pin_is_one_hero() {
        let (_d, svc) = service(None).await;
        let lead = svc.teammates().await.expect("list")[0].clone();
        let scout = svc.create(input("Scout", "claude")).await.expect("scout");
        let notes = PathBuf::from(&scout.home).join("memory/NOTES.md");
        std::fs::write(&notes, "- keep me\n").expect("notes");
        let err = svc
            .update(
                &scout.id,
                TeammatePatch {
                    engine_id: Some("codex".into()),
                    ..Default::default()
                },
            )
            .await
            .expect_err("not installed");
        assert_eq!(err.code, ErrorCode::EngineMissing);
        let pinned = svc
            .update(
                &scout.id,
                TeammatePatch {
                    pinned: Some(true),
                    model: Some(Some("big".into())),
                    ..Default::default()
                },
            )
            .await
            .expect("pin");
        assert!(pinned.pinned);
        assert_eq!(pinned.model.as_deref(), Some("big"));
        assert!(!svc.get(&lead.id).await.expect("lead").pinned);
        let cleared = svc
            .update(
                &scout.id,
                TeammatePatch {
                    model: Some(None),
                    ..Default::default()
                },
            )
            .await
            .expect("clear");
        assert_eq!(cleared.model, None);
        assert_eq!(
            std::fs::read_to_string(&notes).expect("notes"),
            "- keep me\n"
        );
    }

    #[tokio::test]
    async fn f_ag_06_1_a_teammate_with_3_skills_is_briefed_with_fenced_memory_and_their_index() {
        let (_d, svc) = service(None).await;
        let scout = svc.create(input("Scout", "claude")).await.expect("scout");
        let home = PathBuf::from(&scout.home);
        std::fs::write(
            home.join("memory/NOTES.md"),
            "- Release branch is cut on Thursdays.\n",
        )
        .expect("notes");
        for slug in ["release-notes", "test-first", "concise-diffs"] {
            tandem_store::skills::add_from_library(&home, slug).expect("skill");
        }
        svc.mark_skills_seen(
            &scout.id,
            &[
                "release-notes".into(),
                "test-first".into(),
                "concise-diffs".into(),
            ],
        )
        .await;
        let b = svc
            .briefing(
                &scout,
                Path::new("/work"),
                tandem_core::access::Access::Edits,
                false,
                "2026-10-09",
            )
            .await;
        assert!(b.text.contains(&format!(
            "<memory>{}\n<notes>- Release branch is cut on Thursdays.\n</notes>",
            tandem_core::untrusted::label("this teammate's memory")
        )));
        for slug in ["release-notes", "test-first", "concise-diffs"] {
            assert!(b.text.contains(&format!("\n- {slug}: ")), "{slug} indexed");
        }
        // The index, not the bodies.
        assert!(!b.text.contains("## Steps"));
        // May message: the roster names the Lead.
        assert!(b.text.contains(&format!(
            "<roster>{}\nLead — You coordinate the other teammates.",
            tandem_core::untrusted::label("the teammate roster")
        )));
    }

    #[tokio::test]
    async fn f_ag_07_writes_are_capped_credential_free_and_undoable() {
        use tandem_store::memory::MemoryFile;
        let (_d, svc) = service(None).await;
        let mut scout = svc.create(input("Scout", "claude")).await.expect("scout");
        scout.settings.memory_cap = 2048;
        let first = svc
            .write_memory(&scout, MemoryFile::Notes, "- one\n")
            .await
            .expect("fits");
        let over = svc
            .write_memory(&scout, MemoryFile::Notes, &"x".repeat(2049))
            .await
            .expect_err("over");
        assert_eq!(over.code, ErrorCode::LimitReached);
        assert_eq!(
            over.message,
            "Memory is over its 2048 character budget. Condense it and try again."
        );
        let secret = svc
            .write_memory(
                &scout,
                MemoryFile::Notes,
                &format!("- token=ghp_{}", "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8"),
            )
            .await
            .expect_err("secret");
        assert_eq!(secret.message, "Memory can't store credentials");
        assert_eq!(svc.memory(&scout.id).await.expect("m").notes, "- one\n");
        let second = svc
            .write_memory(&scout, MemoryFile::Notes, "- one\n- two\n")
            .await
            .expect("two");
        // The older change can't be undone over a newer one.
        assert_eq!(
            svc.undo_memory(&first).await.expect_err("stale").code,
            ErrorCode::Conflict
        );
        svc.undo_memory(&second).await.expect("undo");
        assert_eq!(svc.memory(&scout.id).await.expect("m").notes, "- one\n");
    }

    #[tokio::test]
    async fn f_ag_06_read_skill_returns_the_skill_fenced_as_data() {
        let (_d, svc) = service(None).await;
        let scout = svc.create(input("Scout", "claude")).await.expect("scout");
        tandem_store::skills::add_from_library(&PathBuf::from(&scout.home), "test-first")
            .expect("skill");
        let text = svc
            .read_skill_for_agent(&scout, "test-first")
            .await
            .expect("read");
        assert!(text.starts_with(&format!(
            "{}\n<skill name=\"test-first\">",
            tandem_core::untrusted::label("the skill test-first")
        )));
        assert!(text.contains("## Steps"));
        assert_eq!(
            svc.read_skill_for_agent(&scout, "nope")
                .await
                .expect_err("missing")
                .code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn f_ag_03_three_suggestion_chips_come_from_the_brief() {
        assert_eq!(
            suggestions("You draft our weekly changelog from merged PRs. Verify every claim against the repo. Never publish without approval."),
            vec![
                "Draft our weekly changelog from merged PRs".to_string(),
                "Verify every claim against the repo".to_string(),
                "What can you take off my plate today?".to_string(),
            ]
        );
        assert_eq!(suggestions("").len(), 3);
        assert_eq!(
            suggestions(rules::LEAD_BRIEF)[0],
            "Coordinate the other teammates"
        );
    }

    #[test]
    fn copies_are_named_within_forty_characters() {
        assert_eq!(copy_name("Scout", &["Scout".into()]), "Scout copy");
        let long = "x".repeat(40);
        let c = copy_name(&long, std::slice::from_ref(&long));
        assert_eq!(c.chars().count(), 40);
        assert!(c.ends_with(" copy"));
    }
}
