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
    pub name: Option<String>,
    #[serde(default)]
    pub brief: Option<String>,
    #[serde(default)]
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
    pub places: Option<Vec<String>>,
    #[serde(default)]
    pub settings: Option<TeammateSettings>,
    #[serde(default)]
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

    /// `teammate_reorder`: the rail's custom order.
    pub async fn reorder(&self, ids: &[TeammateId]) -> Result<(), IpcError> {
        repo::reorder(&self.store, ids).await.map_err(store_err)?;
        self.publish().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Engines;

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

    async fn service(limit: Option<u32>) -> (tempfile::TempDir, Arc<TeammatesService>) {
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

    fn input(name: &str, engine: &str) -> TeammateInput {
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

    #[test]
    fn copies_are_named_within_forty_characters() {
        assert_eq!(copy_name("Scout", &["Scout".into()]), "Scout copy");
        let long = "x".repeat(40);
        let c = copy_name(&long, std::slice::from_ref(&long));
        assert_eq!(c.chars().count(), 40);
        assert!(c.ends_with(" copy"));
    }
}
