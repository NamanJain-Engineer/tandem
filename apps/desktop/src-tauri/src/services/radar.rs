//! Intent radar (F-WT-10): each local worktree's changed symbols, kept from
//! its last parse, refreshed after each checkpoint in its folder, when its
//! overlap is checked, and (once parsed) when its files change on disk: a
//! watcher for each of up to [`MAX_WATCHED_WORKTREES`] worktrees with a
//! checkpoint or Check overlap… of their own, debounced, re-parses only
//! what changed, one parse at a time, never while the worktree is being
//! discarded ([`hold`]). A pair of sibling worktrees that newly share a
//! symbol raises `radar:overlap` once, and any refresh that changes what
//! worktrees share raises `radar:changed`. Warn both agents types
//! `lane_radar_note`'s text, and Claim these files claims through F-TK-05,
//! each only on a click.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::files::watch::{self, DirWatcher, Strategy, WatchOptions};
use tandem_core::ids::WorkspaceId;
use tandem_core::{untrusted, DataDir, ErrorCode, IpcError};
use tandem_git::symbols::{self, SharedSymbol, SymbolChange, SymbolIndex};
use tandem_git::Git;
use tandem_store::repo::workspaces::{self, Workspace};
use tandem_store::Store;

use crate::services::lanes::git_for;
use crate::services::terminal::TerminalService;

/// A worktree's last index.
#[derive(Debug, Clone)]
struct Entry {
    parent: String,
    /// When its parse started: a later one's index is kept over it.
    started: u64,
    /// Shared, not copied: what worktrees share is read again at each
    /// refresh and rail reload (the review of d06b9e3d, SF-1).
    index: Arc<SymbolIndex>,
    /// Parses by file: a refresh parses only what changed since. Shared,
    /// so taking it under the lock copies nothing; the refresh copies it
    /// outside (the review of d3b838de).
    cache: Arc<symbols::ParseCache>,
}

static INDEXES: LazyLock<Mutex<HashMap<String, Entry>>> = LazyLock::new(Mutex::default);
/// Numbers each parse as it starts, for [`keep`].
static STARTED: AtomicU64 = AtomicU64::new(0);
/// The parse number given out when each worktree was last forgotten: a
/// parse that started before isn't kept after. One that began before a
/// discard or Remove repository and finished after brought the gone
/// worktree's index back for good (the review of 30d5bcad). A worktree
/// gone for good keeps `u64::MAX` ([`forget_for_good`]) until a worktree
/// is made again with its id ([`made`]), a few bytes each; any other fence
/// goes once a newer parse is kept, which wins over older ones anyway. A
/// fence is only ever raised, except by [`made`].
static FORGOTTEN: LazyLock<Mutex<HashMap<String, u64>>> = LazyLock::new(Mutex::default);
/// A watcher per parsed worktree (F-WT-10 slice 2), for up to
/// [`MAX_WATCHED_WORKTREES`] of them, until it's forgotten.
static WATCHERS: LazyLock<Mutex<HashMap<String, Watched>>> = LazyLock::new(Mutex::default);
/// Worktrees being discarded, with how many discards hold each: not parsed
/// or watched again meanwhile.
static HELD: LazyLock<Mutex<HashMap<String, usize>>> = LazyLock::new(Mutex::default);
/// Worktrees a watched parse runs for, and whether changes came during it.
static RUNNING: LazyLock<Mutex<HashMap<String, bool>>> = LazyLock::new(Mutex::default);
/// Worktrees with a follow-up refresh ([`follow_up`]), from its wait to
/// the end of its last round, and whether another refresh ran out of time
/// meanwhile: one each, which goes round again then. Every refresh that ran
/// out of time started its own, and they ran side by side, parsing the same
/// files (the review of d3b838de, SF-B); held only while it waited, a
/// refresh that ran out of time during its run still started another (the
/// review of c30bb8bf, SF-3).
static FOLLOWING: LazyLock<Mutex<HashMap<String, bool>>> = LazyLock::new(Mutex::default);
/// The base each worktree row was made with ([`made`]): a parse with
/// another base read the old worktree's list, before the row came back
/// with its id (the review of c30bb8bf, SF-1). A leaf lock, taken under
/// [`INDEXES`].
static PINNED: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Mutex::default);
/// What a watched change needs to parse again (set by [`run`]).
static CONTEXT: OnceLock<Context> = OnceLock::new();

/// What a watched worktree's parses need: the store, the bus, and git (the
/// login environment's, or a fixed one).
#[derive(Clone)]
pub(crate) struct Context {
    store: Store,
    bus: EventBus,
    git: GitSource,
}

#[derive(Clone)]
enum GitSource {
    Login(DataDir, Arc<TerminalService>),
    #[cfg(test)]
    Fixed(Git),
}

impl Context {
    async fn git(&self) -> Option<Git> {
        match &self.git {
            GitSource::Login(data, terminals) => git_for(data, &*terminals.login_env().await).ok(),
            #[cfg(test)]
            GitSource::Fixed(git) => Some(git.clone()),
        }
    }
}

/// How long after a watched change the worktree is parsed again. The wait
/// starts at the first change, so while changes go on it's parsed every
/// `SETTLE`, which warns sooner than waiting for quiet would (the review of
/// 9a4e93ab, S1).
pub const SETTLE: Duration = Duration::from_secs(2);
/// At most this many folders are watched per worktree (inotify watches are
/// a per-user budget, shared with the Files tab), and fewer when the user's
/// budget is small ([`dirs_each`]).
pub const MAX_WATCHED_DIRS: usize = 5_000;
/// At most this many worktrees are watched: on Linux each watch is an
/// inotify instance and two threads, and idle worktrees kept theirs until
/// discarded (the review of 9a4e93ab, S8). One that isn't watched is still
/// read at each checkpoint and Check overlap…, as before slice 2.
pub const MAX_WATCHED_WORKTREES: usize = 8;
/// A watched worktree gives its place to another only once it has gone
/// this long without a parse of its own. Giving it to whichever was parsed
/// last made nine worktrees read in turn rebuild every watcher each round
/// (the review of 30d5bcad, S-3).
pub const WATCH_IDLE: Duration = Duration::from_secs(5 * 60);
/// A watched worktree's share never drops below this many folders.
const MIN_WATCHED_DIRS: usize = 64;
/// A watched batch of more paths than this is an overflow, which parses the
/// worktree again whatever the paths were.
const WATCH_MAX_PATHS: usize = 20_000;

/// A worktree's watcher, and when it was last parsed for itself.
struct Watched {
    _watcher: DirWatcher,
    at: Instant,
}
/// Pairs and symbols already announced, so a notice isn't repeated.
static WARNED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Mutex::default);

/// `lane_radar`: what a worktree shares with its siblings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RadarMark {
    pub lane_id: String,
    /// The shared symbols' qualified names, sorted.
    pub symbols: Vec<String>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether the radar covers this workspace: a local worktree with a base.
fn covered(ws: &Workspace) -> bool {
    ws.kind == "local"
        && ws.parent_id.is_some()
        && ws.lane_branch.is_some()
        // A multi-repo worktree (F-CD-22) has no single base: not covered yet.
        && ws.lane_base.as_deref().is_some_and(|b| !b.is_empty())
        && Path::new(&ws.root).is_dir()
        // A jj workspace (F-CD-24, `.jj` and no `.git`): not covered yet.
        && !tandem_git::jj::jj_without_git(Path::new(&ws.root))
}

/// Parses the worktree again and keeps the result: for its own checkpoint
/// or Check overlap…, so it's watched from then on.
pub async fn refresh(git: &Git, lane: &Workspace) -> Result<Arc<SymbolIndex>, IpcError> {
    refresh_on(git, lane, true, None).await
}

/// [`refresh`] for a sibling read to compare with: it isn't watched for
/// it, so Check overlap… in one of nine worktrees doesn't take the others'
/// watchers in turn (the review of 30d5bcad, S-3).
pub async fn refresh_sibling(git: &Git, lane: &Workspace) -> Result<Arc<SymbolIndex>, IpcError> {
    refresh_on(git, lane, false, None).await
}

/// Parses `lane` again and keeps the result, watching it when `watch`.
/// When that changes what its repository's worktrees share, `radar:changed`
/// goes out on `bus` (the app's when `None`), whichever refresh it was: a
/// change a checkpoint read first, before a watched parse that then found
/// nothing new, never reached the rail (the review of 30d5bcad, S-2). A
/// refresh that ran out of time is followed by another ([`follow_up`]).
async fn refresh_on(
    git: &Git,
    lane: &Workspace,
    watch: bool,
    bus: Option<&EventBus>,
) -> Result<Arc<SymbolIndex>, IpcError> {
    let (Some(parent), Some(base)) = (&lane.parent_id, &lane.lane_base) else {
        return Ok(Arc::default());
    };
    // Numbered before the hold is checked: a discard that holds and
    // forgets the worktree after this check fences this parse out, and one
    // that ended before it is seen here (the review of d06b9e3d, N-1).
    let started = STARTED.fetch_add(1, Ordering::Relaxed) + 1;
    if !covered(lane) || is_held(lane.id.as_str()) {
        return Ok(Arc::default());
    }
    let cached = lock(&INDEXES)
        .get(lane.id.as_str())
        .map(|e| e.cache.clone());
    let mut cache = cached.map(|c| (*c).clone()).unwrap_or_default();
    let done = symbols::index_cached(git, Path::new(&lane.root), base, &mut cache).await?;
    tracing::debug!(
        code = "radar.refreshed",
        parsed = done.parsed,
        more = done.more,
        "radar index refreshed"
    );
    let id = lane.id.as_str();
    let before = shares_of(parent.as_str(), id);
    let Some(index) = keep(id, parent.as_str(), started, base, done.index, cache) else {
        return Ok(Arc::default());
    };
    if shares_of(parent.as_str(), id) != before {
        tell_changed(bus, id);
    }
    if watch {
        ensure_watched(lane).await;
    }
    if done.more {
        follow_up(lane.clone(), watch);
    }
    Ok(index)
}

/// What `lane_id` shares with each of its repository's other kept
/// worktrees. Only its own pairs: its refresh changes nothing else, and
/// every pair of the repository was compared, each index copied, at every
/// refresh (the review of d06b9e3d, SF-1).
fn shares_of(parent: &str, lane_id: &str) -> BTreeMap<String, Vec<SharedSymbol>> {
    let (mine, others): (Arc<SymbolIndex>, Vec<(String, Arc<SymbolIndex>)>) = {
        let indexes = lock(&INDEXES);
        let Some(mine) = indexes.get(lane_id).map(|e| e.index.clone()) else {
            return BTreeMap::new();
        };
        let others: Vec<(String, Arc<SymbolIndex>)> = indexes
            .iter()
            .filter(|(id, e)| e.parent == parent && id.as_str() != lane_id)
            .map(|(id, e)| (id.clone(), e.index.clone()))
            .collect();
        (mine, others)
    };
    let me = symbols::Comparable::new(&mine);
    others
        .into_iter()
        .filter_map(|(id, theirs)| {
            #[cfg(test)]
            COMPARED.with(|n| n.set(n.get() + 1));
            let found = symbols::shared_between(&me, &symbols::Comparable::new(&theirs));
            (!found.is_empty()).then_some((id, found))
        })
        .collect()
}

#[cfg(test)]
thread_local! {
    /// Pairs [`shares_of`] compared on this thread, for its test.
    static COMPARED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A refresh that ran out of time left files for the next one: that one
/// comes after [`SETTLE`], since nothing else would read them while the
/// worktree stays as it is (the review of d06b9e3d, N-2). Each refresh
/// parses at least one more file, so the rounds end. One at a time per
/// worktree ([`FOLLOWING`]): a refresh that runs out of time while one is
/// on its way or running gives it one more round instead.
fn follow_up(lane: Workspace, watch: bool) {
    let (Some(ctx), Ok(handle)) = (
        CONTEXT.get().cloned(),
        tokio::runtime::Handle::try_current(),
    ) else {
        return;
    };
    if !follows(lane.id.as_str()) {
        return;
    }
    handle.spawn(refresh_later(lane, watch, ctx));
}

/// Whether `lane_id` takes a new follow-up now: not while one is on its
/// way or running, which goes round once more instead.
fn follows(lane_id: &str) -> bool {
    let mut following = lock(&FOLLOWING);
    match following.get_mut(lane_id) {
        Some(again) => {
            *again = true;
            false
        }
        None => {
            following.insert(lane_id.to_string(), false);
            true
        }
    }
}

/// Whether `lane_id`'s follow-up goes round again: a refresh ran out of
/// time during its round. When not, the follow-up ends and is off the list.
fn again(lane_id: &str) -> bool {
    let mut following = lock(&FOLLOWING);
    match following.get_mut(lane_id) {
        Some(again) if *again => {
            *again = false;
            true
        }
        _ => {
            following.remove(lane_id);
            false
        }
    }
}

/// The follow-up's rounds, each after [`SETTLE`], as a boxed `Send`
/// future: a round's refresh can follow itself up, and the box ends that
/// cycle. A worktree's own refresh goes round as a watched change does
/// ([`on_watched`]: one parse at a time with its watched rescans, and a
/// pair newly sharing in the files it read gets its notice, the review of
/// d3b838de). A sibling's only reads the row again (gone or changed, it's
/// the row now that counts) and parses on: a sibling read finished in time
/// raises no notice either (the review of c30bb8bf, N-4).
fn refresh_later(
    lane: Workspace,
    watch: bool,
    ctx: Context,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(async move {
        let id = lane.id.as_str().to_string();
        loop {
            tokio::time::sleep(SETTLE).await;
            if watch {
                on_watched(id.clone(), ctx.clone()).await;
            } else if let (Some(git), Ok(all)) =
                (ctx.git().await, workspaces::list(&ctx.store).await)
            {
                if let Some(now) = all.iter().find(|w| w.id == lane.id && covered(w)) {
                    let _ = refresh_on(&git, now, false, Some(&ctx.bus)).await;
                }
            }
            if !again(&id) {
                return;
            }
        }
    })
}

/// `radar:changed` for `lane_id`, on `bus` or the app's.
fn tell_changed(bus: Option<&EventBus>, lane_id: &str) {
    if let Some(bus) = bus.or_else(|| CONTEXT.get().map(|c| &c.bus)) {
        bus.publish(BusEvent::new(
            Topic::App,
            "radar:changed",
            serde_json::json!({ "laneIds": [lane_id] }),
        ));
    }
}

/// Keeps a worktree's parse, unless one that started later was kept first:
/// that one read the files after it did. Checkpoints, Check overlap… and
/// watched changes refresh alongside each other, and the last to finish
/// overwrote a newer index with an older one (the review of 9a4e93ab, S2).
/// The index kept, or `None` when the worktree is being discarded.
fn keep(
    lane_id: &str,
    parent: &str,
    started: u64,
    base: &str,
    index: SymbolIndex,
    cache: symbols::ParseCache,
) -> Option<Arc<SymbolIndex>> {
    // A discard that started meanwhile wins: nothing is kept or watched.
    // Checked under the lock [`hold`]'s `forget` takes after marking the
    // worktree held, so either this sees the hold or `forget` removes what
    // this keeps.
    let mut indexes = lock(&INDEXES);
    if is_held(lane_id) {
        return None;
    }
    // Parsed from the old worktree's row: its base isn't the one this row
    // was made with, whenever the parse began (the review of c30bb8bf,
    // SF-1).
    if lock(&PINNED)
        .get(lane_id)
        .is_some_and(|pinned| pinned != base)
    {
        return None;
    }
    // Begun before the worktree was forgotten (a discard, Remove
    // repository), or it's gone for good.
    let mut forgotten = lock(&FORGOTTEN);
    if forgotten.get(lane_id).is_some_and(|at| started <= *at) {
        return None;
    }
    if let Some(newer) = indexes.get(lane_id).filter(|e| e.started > started) {
        return Some(newer.index.clone());
    }
    // This parse began after the fence, and wins over any that began
    // before: the fence has done its work.
    forgotten.remove(lane_id);
    let index = Arc::new(index);
    indexes.insert(
        lane_id.to_string(),
        Entry {
            parent: parent.to_string(),
            started,
            index: index.clone(),
            cache: Arc::new(cache),
        },
    );
    Some(index)
}

fn is_held(lane_id: &str) -> bool {
    lock(&HELD).contains_key(lane_id)
}

/// Whether the radar keeps an index for `lane_id`, for tests elsewhere.
#[cfg(test)]
pub(crate) fn has_index(lane_id: &str) -> bool {
    lock(&INDEXES).contains_key(lane_id)
}

/// While it lives, the worktree is neither parsed nor watched (it's being
/// discarded); its index and watcher go at once.
#[must_use = "the radar stays off the worktree only while the hold lives"]
pub struct Hold(String);

impl Drop for Hold {
    fn drop(&mut self) {
        let mut held = lock(&HELD);
        if let Some(n) = held.get_mut(&self.0) {
            *n -= 1;
            if *n == 0 {
                held.remove(&self.0);
            }
        }
    }
}

/// Holds the radar off a worktree being discarded. Holds are counted: with
/// two discards of one worktree at once, the first to end doesn't free it
/// while the other still runs.
pub fn hold(lane_id: &str) -> Hold {
    *lock(&HELD).entry(lane_id.to_string()).or_insert(0) += 1;
    forget(lane_id);
    Hold(lane_id.to_string())
}

/// Each watched worktree's share of folders: a quarter of the user's
/// inotify watches (8,192 by default before Linux 5.11, shared with the
/// Files tab and every other app) split between [`MAX_WATCHED_WORKTREES`],
/// within [`MIN_WATCHED_DIRS`]..=[`MAX_WATCHED_DIRS`]. Elsewhere a worktree
/// is one recursive watch, and the share isn't used.
fn dirs_each() -> usize {
    static EACH: OnceLock<usize> = OnceLock::new();
    *EACH.get_or_init(|| share_of(user_watches()))
}

/// The user's inotify watches.
#[cfg(target_os = "linux")]
fn user_watches() -> Option<usize> {
    std::fs::read_to_string("/proc/sys/fs/inotify/max_user_watches")
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Not Linux: a worktree is one recursive watch.
#[cfg(not(target_os = "linux"))]
fn user_watches() -> Option<usize> {
    None
}

fn share_of(user_watches: Option<usize>) -> usize {
    user_watches.map_or(MAX_WATCHED_DIRS, |n| {
        (n / 4 / MAX_WATCHED_WORKTREES).clamp(MIN_WATCHED_DIRS, MAX_WATCHED_DIRS)
    })
}

/// Notes a parse of `id` for itself at `now`; false when it isn't watched.
fn seen(watchers: &mut HashMap<String, Watched>, id: &str, now: Instant) -> bool {
    match watchers.get_mut(id) {
        Some(w) => {
            w.at = w.at.max(now);
            true
        }
        None => false,
    }
}

/// Whether another worktree can be watched at `now`: fewer than `max` are,
/// or one has gone [`WATCH_IDLE`] without a parse.
fn room(watchers: &HashMap<String, Watched>, max: usize, now: Instant) -> bool {
    watchers.len() < max
        || watchers
            .values()
            .any(|w| now.saturating_duration_since(w.at) >= WATCH_IDLE)
}

/// Keeps `watcher` for `id`, parsed at `now`. Past `max`, the worktree idle
/// longest stops being watched if it has been idle [`WATCH_IDLE`];
/// otherwise `id` isn't watched.
fn admit(
    watchers: &mut HashMap<String, Watched>,
    id: String,
    watcher: DirWatcher,
    max: usize,
    now: Instant,
) {
    if watchers.len() >= max {
        let Some((idlest, at)) = watchers
            .iter()
            .map(|(k, w)| (k.clone(), w.at))
            .min_by_key(|(_, at)| *at)
        else {
            return;
        };
        if now.saturating_duration_since(at) < WATCH_IDLE {
            return;
        }
        watchers.remove(&idlest);
        tracing::debug!(
            code = "radar.watch_dropped",
            "the worktree idle longest isn't watched any more"
        );
    }
    watchers.insert(
        id,
        Watched {
            _watcher: watcher,
            at: now,
        },
    );
}

/// Whether a watched change can move what the radar read: a file in a
/// parsed language, or a folder renamed or removed, which moves every file
/// in it (`mv src lib` went unseen; the review of 9a4e93ab). Any path
/// that's gone may have been a folder, whatever its name (`mv gen.d ..`;
/// the review of 30d5bcad): parsing again reads only what changed.
///
/// Only in a folder `.gitignore` keeps: the radar reads git's changes,
/// which never list an ignored file, and a build's files coming and going
/// in `target/` or `.next/` parsed a worktree every 2 s on macOS and
/// Windows, whose watches report ignored folders too (the review of
/// d06b9e3d, N-6). `kept` holds each folder's answer for one batch.
fn moves_code(root: &Path, rel: &str, kept: &mut HashMap<PathBuf, bool>) -> bool {
    let code = symbols::Lang::of(rel).is_some()
        || std::fs::symlink_metadata(root.join(rel)).map_or(true, |m| m.is_dir());
    if !code {
        return false;
    }
    let folder = Path::new(rel)
        .parent()
        .unwrap_or(Path::new(""))
        .to_path_buf();
    *kept
        .entry(folder)
        .or_insert_with_key(|f| tandem_core::files::folder_kept(root, f))
}

/// Starts watching a parsed worktree's folder, once, with the app's
/// context ([`run`] sets it).
async fn ensure_watched(lane: &Workspace) {
    if let Some(ctx) = CONTEXT.get().cloned() {
        watch_with(lane, ctx).await;
    }
}

/// Watches `lane`'s folder, once, when there's room for it ([`room`]): a
/// change to a file in a parsed language parses it again after [`SETTLE`].
pub(crate) async fn watch_with(lane: &Workspace, ctx: Context) {
    let id = lane.id.as_str().to_string();
    {
        let mut watchers = lock(&WATCHERS);
        let now = Instant::now();
        // No room: no folder walk for a watcher that wouldn't be kept.
        if is_held(&id)
            || seen(&mut watchers, &id, now)
            || !room(&watchers, MAX_WATCHED_WORKTREES, now)
        {
            return;
        }
    }
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let root = std::fs::canonicalize(&lane.root).unwrap_or_else(|_| PathBuf::from(&lane.root));
    let opts = WatchOptions {
        debounce: SETTLE,
        strategy: Strategy::for_platform(),
        max_dirs: dirs_each(),
        // A build writes thousands of files into folders git ignores: past
        // 500 a batch was an overflow, with its paths gone, and parsed the
        // worktree every 2 s (the review of d3b838de, N-6). Past this many
        // it still is, as is a batch the system says it dropped events
        // from.
        max_paths: WATCH_MAX_PATHS,
    };
    let lane_id = id.clone();
    // Walking the folders to watch is blocking IO.
    let made = tokio::task::spawn_blocking(move || {
        let base = root.clone();
        watch::watch(
            &root,
            None,
            opts,
            Box::new(move |change| {
                let mut kept = HashMap::new();
                let parsed =
                    change.overflow || change.paths.iter().any(|p| moves_code(&base, p, &mut kept));
                if parsed {
                    handle.spawn(watched(lane_id.clone(), ctx.clone()));
                }
            }),
        )
    })
    .await;
    match made {
        Ok(Ok(w)) => {
            // A discard or another watcher may have come first (the hold is
            // checked under the lock, as in [`refresh`]), or the worktree
            // was forgotten during the walk: watched only while its index
            // is kept (the review of d3b838de, SF-A). Nothing takes these
            // two locks the other way round.
            let mut watchers = lock(&WATCHERS);
            let now = Instant::now();
            if !is_held(&id) && !seen(&mut watchers, &id, now) && lock(&INDEXES).contains_key(&id) {
                admit(&mut watchers, id, w, MAX_WATCHED_WORKTREES, now);
            }
        }
        Ok(Err(e)) => {
            tracing::debug!(code = "radar.watch_failed", error = %e, "a worktree isn't watched")
        }
        Err(_) => {}
    }
}

/// [`on_watched`] as a boxed `Send` future: a watched parse can start a
/// watcher, whose callback spawns this again, and the box ends that cycle.
fn watched(
    lane_id: String,
    ctx: Context,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(on_watched(lane_id, ctx))
}

/// A watched change in a worktree: parse it again (once at a time; changes
/// during a parse are taken by one more after it), then announce.
async fn on_watched(lane_id: String, ctx: Context) {
    {
        let mut running = lock(&RUNNING);
        if let Some(again) = running.get_mut(&lane_id) {
            *again = true;
            return;
        }
        running.insert(lane_id.clone(), false);
    }
    loop {
        if !is_held(&lane_id) {
            rescan(&lane_id, &ctx).await;
        }
        let mut running = lock(&RUNNING);
        match running.get_mut(&lane_id) {
            Some(again) if *again => *again = false,
            _ => {
                running.remove(&lane_id);
                return;
            }
        }
    }
}

/// Parses one worktree again from a watched change and announces new pairs.
async fn rescan(lane_id: &str, ctx: &Context) {
    let Ok(all) = workspaces::list(&ctx.store).await else {
        return;
    };
    let Some(lane) = all.iter().find(|w| w.id.as_str() == lane_id && covered(w)) else {
        forget(lane_id);
        return;
    };
    let Some(git) = ctx.git().await else {
        return;
    };
    // `radar:changed` comes from the refresh, only when what's shared
    // changed: each event reloads an open Check overlap…, which parses
    // every sibling again (the review of 9a4e93ab, S3).
    announce(&ctx.store, &git, &ctx.bus, lane, &all, true).await;
}

/// The last index, parsing it first (not watching it) when there's none.
pub async fn index_of(git: &Git, lane: &Workspace) -> Result<Arc<SymbolIndex>, IpcError> {
    index_on(git, lane, None).await
}

/// [`index_of`], telling a change on `bus`.
async fn index_on(
    git: &Git,
    lane: &Workspace,
    bus: Option<&EventBus>,
) -> Result<Arc<SymbolIndex>, IpcError> {
    if let Some(e) = lock(&INDEXES).get(lane.id.as_str()) {
        return Ok(e.index.clone());
    }
    refresh_on(git, lane, false, bus).await
}

/// What `here` shares with `there` (F-WT-10).
pub fn shared(here: &SymbolIndex, there: &SymbolIndex) -> Vec<SharedSymbol> {
    symbols::shared_symbols(here, there)
}

/// `lane_radar`: from the kept indexes only; nothing is parsed. Only the
/// given worktrees are compared, so one since discarded doesn't count.
pub fn marks(lane_ids: &[String]) -> Vec<RadarMark> {
    let wanted: HashSet<&String> = lane_ids.iter().collect();
    // Parents and indexes only: the parse caches hold every changed file's
    // definitions, and the rail reads marks again after each watched parse.
    let all: HashMap<String, (String, Arc<SymbolIndex>)> = lock(&INDEXES)
        .iter()
        .filter(|(id, _)| wanted.contains(id))
        .map(|(id, e)| (id.clone(), (e.parent.clone(), e.index.clone())))
        .collect();
    // Each index read once for all its pairs: they're compared again on
    // every `radar:changed` (the review of d3b838de).
    let read: HashMap<&String, (&String, symbols::Comparable<'_>)> = all
        .iter()
        .map(|(id, (parent, index))| (id, (parent, symbols::Comparable::new(index))))
        .collect();
    let mut out = Vec::new();
    for id in lane_ids {
        let Some((parent, mine)) = read.get(id) else {
            continue;
        };
        let mut names = BTreeSet::new();
        for (other_id, (other_parent, other)) in &read {
            if *other_id != id && other_parent == parent {
                names.extend(
                    symbols::shared_between(mine, other)
                        .into_iter()
                        .map(|s| s.qualified),
                );
            }
        }
        if !names.is_empty() {
            out.push(RadarMark {
                lane_id: id.clone(),
                symbols: names.into_iter().collect(),
            });
        }
    }
    out
}

/// Forgets a worktree's index and stops watching it (it's being
/// discarded, or isn't covered any more). A parse already under way isn't
/// kept when it ends ([`FORGOTTEN`]).
pub fn forget(lane_id: &str) {
    fence(lane_id, STARTED.load(Ordering::Relaxed));
}

/// [`forget`] for a worktree that's gone (discarded, or its repository
/// removed): no parse of it is kept again, even one that starts later from
/// a list read before it went (Check overlap… walking its siblings, or a
/// watched rescan), which kept its index and could watch it again (the
/// review of d06b9e3d, N-1).
pub fn forget_for_good(lane_id: &str) {
    {
        let _indexes = lock(&INDEXES);
        lock(&PINNED).remove(lane_id);
    }
    fence(lane_id, u64::MAX);
    unwarn(lane_id);
}

/// A worktree row made with `base`, or made again at a folder used before,
/// which the store gives its old id: a fence left from [`forget_for_good`]
/// kept the new worktree's radar off until the app restarted (the review of
/// d3b838de, MF-A). The fence comes down to now, not away: a parse begun
/// before still read the old worktree's files. And the row's base is
/// pinned: a parse that begins later from a list read before the row came
/// back carries the old worktree's base, and is refused for it (the review
/// of c30bb8bf, SF-1). The old worktree's notices are forgotten, so the
/// same overlap in the new one is told again (SF-2).
pub fn made(lane_id: &str, base: Option<&str>) {
    {
        // Under the lock [`keep`] checks the fence and the pin in.
        let _indexes = lock(&INDEXES);
        if let Some(at) = lock(&FORGOTTEN).get_mut(lane_id) {
            *at = STARTED.load(Ordering::Relaxed);
        }
        let mut pinned = lock(&PINNED);
        match base.filter(|b| !b.is_empty()) {
            Some(b) => {
                pinned.insert(lane_id.to_string(), b.to_string());
            }
            None => {
                pinned.remove(lane_id);
            }
        }
    }
    unwarn(lane_id);
}

/// Forgets the notices raised for `lane_id`'s pairs: nothing removed them,
/// so a worktree made again with its old id never told the same overlap
/// again (the review of c30bb8bf, SF-2).
fn unwarn(lane_id: &str) {
    lock(&WARNED).retain(|key| {
        let mut ids = key.splitn(3, '|');
        ids.next() != Some(lane_id) && ids.next() != Some(lane_id)
    });
}

fn fence(lane_id: &str, at: u64) {
    {
        // Under the lock [`keep`] checks the fence in. Only ever raised: a
        // later [`forget`] (a watched rescan that no longer finds a removed
        // worktree) brought one gone for good back to a number a later
        // parse passes (the review of d3b838de, SF-A); [`made`] alone
        // lowers it.
        let mut indexes = lock(&INDEXES);
        let mut forgotten = lock(&FORGOTTEN);
        let fence = forgotten.entry(lane_id.to_string()).or_insert(at);
        *fence = (*fence).max(at);
        indexes.remove(lane_id);
    }
    lock(&WATCHERS).remove(lane_id);
}

fn pair_key(a: &str, b: &str, symbols: &[String]) -> String {
    let (x, y) = if a < b { (a, b) } else { (b, a) };
    format!("{x}|{y}|{}", symbols.join(","))
}

/// After a checkpoint in `folder`: refresh the worktree there, parse the
/// siblings not parsed yet, and announce pairs newly sharing symbols.
pub async fn on_checkpoint(store: &Store, git: &Git, bus: &EventBus, folder: &str) {
    let folder = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.into());
    let Ok(all) = workspaces::list(store).await else {
        return;
    };
    let same = |ws: &Workspace| std::fs::canonicalize(&ws.root).is_ok_and(|r| r == folder);
    let Some(lane) = all.iter().find(|w| covered(w) && same(w)) else {
        return;
    };
    announce(store, git, bus, lane, &all, true).await;
}

/// Refreshes `lane`, compares it with its siblings, and raises
/// `radar:overlap` for pairs newly sharing symbols. False when it wasn't
/// parsed (a multi-repo worktree, a parse that failed, one being discarded).
async fn announce(
    store: &Store,
    git: &Git,
    bus: &EventBus,
    lane: &Workspace,
    all: &[Workspace],
    watch: bool,
) -> bool {
    // F-CD-22: no radar for multi-repo worktrees yet.
    if crate::services::lanes::is_multi(store, lane)
        .await
        .unwrap_or(false)
        || is_held(lane.id.as_str())
    {
        return false;
    }
    let Ok(mine) = refresh_on(git, lane, watch, Some(bus)).await else {
        tracing::warn!(code = "radar.parse_failed", "radar index skipped");
        return false;
    };
    let parent = lane.parent_id.clone();
    for sib in all
        .iter()
        .filter(|w| w.id != lane.id && w.parent_id == parent && covered(w))
    {
        if is_held(sib.id.as_str()) {
            continue;
        }
        let Ok(theirs) = index_on(git, sib, Some(bus)).await else {
            continue;
        };
        let found = shared(&mine, &theirs);
        if found.is_empty() {
            continue;
        }
        let names: Vec<String> = found.iter().map(SharedSymbol::display).collect();
        if !lock(&WARNED).insert(pair_key(lane.id.as_str(), sib.id.as_str(), &names)) {
            continue;
        }
        tracing::info!(
            code = "radar.overlap",
            symbols = names.len(),
            "worktrees share symbols"
        );
        bus.publish(BusEvent::new(
            Topic::App,
            "radar:overlap",
            serde_json::json!({
                "repoId": parent.as_ref().map(|p| p.as_str()),
                "a": { "laneId": lane.id.as_str(), "branch": lane.lane_branch },
                "b": { "laneId": sib.id.as_str(), "branch": sib.lane_branch },
                "symbols": names,
            }),
        ));
    }
    true
}

/// Listens for checkpoints for as long as the app runs.
pub async fn run(store: Store, data: DataDir, bus: EventBus, terminals: Arc<TerminalService>) {
    let _ = CONTEXT.set(Context {
        store: store.clone(),
        bus: bus.clone(),
        git: GitSource::Login(data.clone(), terminals.clone()),
    });
    let mut rx = bus.subscribe(Topic::App);
    loop {
        match rx.recv().await {
            Ok(ev) if ev.name == "checkpoint:created" => {
                let Some(folder) = ev.payload.get("folder").and_then(|f| f.as_str()) else {
                    continue;
                };
                let login = terminals.login_env().await;
                if let Ok(git) = git_for(&data, &login) {
                    on_checkpoint(&store, &git, &bus, folder).await;
                }
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(_) => return,
        }
    }
}

fn what(c: SymbolChange, renamed_to: Option<&str>) -> String {
    match (c, renamed_to) {
        (SymbolChange::Renamed, Some(to)) => format!("renamed to {to}"),
        (SymbolChange::Added, _) => "added".into(),
        (SymbolChange::Changed, _) => "changed".into(),
        (SymbolChange::Removed | SymbolChange::Renamed, _) => "renamed or removed".into(),
        (SymbolChange::Used, _) => "used".into(),
    }
}

/// The note Warn both agents types into `lane`'s agent: what the sibling
/// changed that this worktree also touches. Names and branches come from
/// code and agents, so they're data behind the fence, and no `@` there names
/// a file for the agent to attach: TypeScript names a module or a method
/// with any string (D-3022).
pub fn note_text(other_branch: &str, found: &[SharedSymbol]) -> String {
    let mut data = format!("Other worktree: {other_branch}\n");
    for s in found {
        data.push_str(&format!(
            "- {}: {} here, {} there\n",
            s.qualified,
            what(s.here, s.renamed_to.as_deref()),
            what(s.there, s.renamed_to.as_deref())
        ));
    }
    format!(
        "Heads-up from Vibezeo: another agent, working in a parallel worktree of this \
         repository, is changing code that your changes also touch. Check that your work \
         still fits theirs (a renamed or changed symbol you use, or the same one changed \
         twice), and adjust before you finish.\n\n{}\n{}",
        untrusted::label("the overlapping symbols"),
        untrusted::fence_defanged("text", data.trim_end())
    )
}

/// `lane_radar_note`.
pub async fn note(
    store: &Store,
    git: &Git,
    lane_id: &WorkspaceId,
    other_id: &WorkspaceId,
) -> Result<String, IpcError> {
    let lane = workspaces::get(store, lane_id).await?;
    let other = workspaces::get(store, other_id).await?;
    if lane.parent_id.is_none() || lane.parent_id != other.parent_id || lane.id == other.id {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "Those aren't two worktrees of one repository",
        ));
    }
    let found = shared(
        &*index_of(git, &lane).await?,
        &*index_of(git, &other).await?,
    );
    if found.is_empty() {
        return Err(IpcError::new(
            ErrorCode::NotFound,
            "These worktrees don't share symbols any more",
        ));
    }
    Ok(note_text(
        other.lane_branch.as_deref().unwrap_or(&other.name),
        &found,
    ))
}

/// `lane_radar_claim`: claims, for `lane_id`'s worktree (F-TK-05), the
/// files where it changed, renamed or removed the definitions it shares
/// with `other_id`, with an intent naming them and the sibling. Returns the
/// paths claimed.
pub async fn claim(
    store: &Store,
    git: &Git,
    tasks: Arc<crate::services::tasks::TasksService>,
    lane_id: &WorkspaceId,
    other_id: &WorkspaceId,
) -> Result<Vec<String>, IpcError> {
    let lane = workspaces::get(store, lane_id).await?;
    let other = workspaces::get(store, other_id).await?;
    if lane.parent_id.is_none() || lane.parent_id != other.parent_id || lane.id == other.id {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "Those aren't two worktrees of one repository",
        ));
    }
    let mine = index_of(git, &lane).await?;
    let found = shared(&mine, &*index_of(git, &other).await?);
    let (paths, intent) = claim_of(
        &mine,
        &found,
        other.lane_branch.as_deref().unwrap_or(&other.name),
    );
    if paths.is_empty() {
        return Err(IpcError::new(
            ErrorCode::NotFound,
            "This worktree didn't change the symbols it shares any more",
        ));
    }
    let folder = PathBuf::from(&lane.root);
    tokio::task::spawn_blocking(move || tasks.claim_for_tree(&folder, &paths, &intent))
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?
}

/// The files `mine` changed the `found` definitions in (sorted, at most
/// F-TK-05's [`tandem_tasks::CLAIM_PATHS_MAX`]), and the claim's intent:
/// "Intent radar: renamed parse → parseLine, changed Session.refresh, which
/// tandem/b uses" ("changes too" when the sibling changed them as well).
pub fn claim_of(
    mine: &SymbolIndex,
    found: &[SharedSymbol],
    other_branch: &str,
) -> (Vec<String>, String) {
    let names: HashSet<&str> = found.iter().map(|s| s.name.as_str()).collect();
    // What the sibling did to the symbols this worktree changed.
    let theirs = found.iter().filter(|s| s.here != SymbolChange::Used);
    let (uses, changes) = theirs.fold((false, false), |(u, c), s| {
        (
            u || s.there == SymbolChange::Used,
            c || s.there != SymbolChange::Used,
        )
    });
    let tail = match (uses, changes) {
        (false, true) => "changes too",
        (true, true) => "uses or changes too",
        _ => "uses",
    };
    let mut paths = BTreeSet::new();
    let mut did = Vec::new();
    for s in &mine.symbols {
        if !names.contains(s.name.as_str())
            || !matches!(
                s.change,
                SymbolChange::Changed | SymbolChange::Removed | SymbolChange::Renamed
            )
        {
            continue;
        }
        paths.insert(s.path.clone());
        // A member renamed with its type is said by the type's rename
        // (`renamed Session.Session → UserSession.UserSession` was listed
        // too; the review of 30d5bcad), when the type is listed: when it
        // isn't shared, the member's rename is the one to say (the intent
        // read "Intent radar: , which tandem/b uses"; the review of
        // d06b9e3d, SF-2).
        if s.change == SymbolChange::Renamed
            && symbols::renamed_with_its_type(&mine.symbols, s)
                .is_some_and(|t| names.contains(t.name.as_str()))
        {
            continue;
        }
        let words = match (&s.change, &s.renamed_to) {
            (SymbolChange::Renamed, Some(to)) => format!("renamed {} → {to}", s.qualified),
            (SymbolChange::Changed, _) => format!("changed {}", s.qualified),
            _ => format!("removed {}", s.qualified),
        };
        if !did.contains(&words) {
            did.push(words);
        }
    }
    let mut intent = format!(
        "Intent radar: {}, which {other_branch} {tail}",
        did.join(", ")
    );
    if intent.chars().count() > tandem_tasks::CLAIM_INTENT_MAX {
        intent = intent
            .chars()
            .take(tandem_tasks::CLAIM_INTENT_MAX - 1)
            .collect::<String>()
            + "…";
    }
    (
        paths
            .into_iter()
            .take(tandem_tasks::CLAIM_PATHS_MAX)
            .collect(),
        intent,
    )
}
#[cfg(test)]
mod tests {
    use super::*;

    /// The overlap notice among what was published, the rest dropped.
    fn overlap_in(rx: &mut tokio::sync::broadcast::Receiver<BusEvent>) -> Option<BusEvent> {
        std::iter::from_fn(|| rx.try_recv().ok()).find(|e| e.name == "radar:overlap")
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    struct Fx {
        _tmp: tempfile::TempDir,
        store: Store,
        git: Git,
        lanes: Vec<Workspace>,
    }

    /// `shop` with `src/session.ts`, and one worktree per name.
    async fn fixture(names: &[&str]) -> Fx {
        fixture_with(names, &[]).await
    }

    /// [`fixture`], with more files in the base.
    async fn fixture_with(names: &[&str], files: &[(&str, &str)]) -> Fx {
        let tmp = tempfile::tempdir().expect("tmp");
        let data = DataDir::at(tmp.path().join("data")).expect("data");
        let main = tmp.path().join("shop");
        std::fs::create_dir_all(main.join("src")).expect("mkdir");
        git(&main, &["init", "-q", "-b", "main"]);
        for (k, v) in [
            ("user.name", "t"),
            ("user.email", "t@t"),
            ("commit.gpgsign", "false"),
        ] {
            git(&main, &["config", k, v]);
        }
        std::fs::write(
            main.join("src/session.ts"),
            "export class Session {\n  refresh(): void {}\n}\n",
        )
        .expect("write");
        for (path, text) in files {
            std::fs::write(main.join(path), text).expect("write");
        }
        git(&main, &["add", "."]);
        git(&main, &["commit", "-q", "-m", "init"]);
        let base = git(&main, &["rev-parse", "HEAD"]);
        let store = Store::open(&data).expect("store");
        let (parent, _) = workspaces::add(
            &store,
            workspaces::NewWorkspace {
                kind: "local".into(),
                host_id: "local".into(),
                root: main.to_string_lossy().into_owned(),
                name: "shop".into(),
                wsl_distro: None,
            },
            None,
        )
        .await
        .expect("parent");
        let mut lanes = Vec::new();
        for name in names {
            let wt = main.join(".tandem/worktrees").join(name);
            let branch = format!("tandem/{name}");
            git(
                &main,
                &[
                    "worktree",
                    "add",
                    "-q",
                    wt.to_str().expect("utf8"),
                    "-b",
                    &branch,
                ],
            );
            lanes.push(
                workspaces::add_lane(
                    &store,
                    workspaces::NewLane {
                        parent_id: parent.id.clone(),
                        root: wt.to_string_lossy().into_owned(),
                        name: (*name).into(),
                        branch,
                        base: base.clone(),
                    },
                )
                .await
                .expect("lane"),
            );
        }
        let hooks = tmp.path().join("hooks");
        std::fs::create_dir_all(&hooks).expect("hooks");
        Fx {
            git: Git::new("git", hooks),
            _tmp: tmp,
            store,
            lanes,
        }
    }

    #[tokio::test]
    async fn f_wt_10_7_removing_the_repository_forgets_its_worktrees() {
        // Their indexes and watchers outlived the repository (the review of
        // 9a4e93ab, S6).
        let fx = fixture(&["one", "two"]).await;
        for lane in &fx.lanes {
            refresh(&fx.git, lane).await.expect("refresh");
        }
        // One lock at a time: the watcher's admit takes WATCHERS, then
        // INDEXES.
        let kept = |id: &str| {
            let indexed = lock(&INDEXES).contains_key(id);
            indexed || lock(&WATCHERS).contains_key(id)
        };
        assert!(fx.lanes.iter().all(|l| kept(l.id.as_str())));
        let parent = fx.lanes[0].parent_id.clone().expect("parent");
        let terms = crate::services::terminal::TerminalService::new(
            tandem_pty::PtyManager::new(),
            fx.store.clone(),
        );
        crate::services::workspaces::remove(&fx.store, &terms, &parent)
            .await
            .expect("remove");
        assert!(fx.lanes.iter().all(|l| !kept(l.id.as_str())));
    }

    #[tokio::test]
    async fn f_wt_10_1_4_a_rename_and_a_use_are_announced_once_and_noted_in_a_fence() {
        let fx = fixture(&["rename", "caller"]).await;
        let (a, b) = (&fx.lanes[0], &fx.lanes[1]);
        std::fs::write(
            Path::new(&a.root).join("src/session.ts"),
            "export class UserSession {\n  refresh(): void {}\n}\n",
        )
        .expect("write");
        std::fs::write(
            Path::new(&b.root).join("src/use.ts"),
            "import { Session } from './session';\nnew Session().refresh();\n",
        )
        .expect("write");
        let bus = EventBus::new();
        let mut rx = bus.subscribe(Topic::App);
        on_checkpoint(&fx.store, &fx.git, &bus, &b.root).await;
        let ev = overlap_in(&mut rx).expect("an overlap notice");
        assert_eq!(ev.payload["a"]["branch"], "tandem/caller");
        assert_eq!(ev.payload["b"]["branch"], "tandem/rename");
        assert_eq!(
            ev.payload["symbols"],
            serde_json::json!(["Session", "Session.refresh"])
        );
        // The same overlap again: no second notice.
        on_checkpoint(&fx.store, &fx.git, &bus, &b.root).await;
        assert!(rx.try_recv().is_err());
        // Both worktrees are marked.
        let ids: Vec<String> = fx.lanes.iter().map(|l| l.id.as_str().to_string()).collect();
        let m = marks(&ids);
        assert_eq!(m.len(), 2);
        assert!(m
            .iter()
            .all(|m| m.symbols == ["Session", "Session.refresh"]));
        // The note for the caller's agent: the names inside the fence.
        let text = note(&fx.store, &fx.git, &b.id, &a.id).await.expect("note");
        assert!(text.starts_with("Heads-up from Vibezeo"));
        assert!(text.contains(&untrusted::label("the overlapping symbols")));
        let fence = text.find("```text\n").expect("fence");
        assert!(text[fence..].contains("Other worktree: tandem/rename"));
        assert!(text[fence..].contains("- Session: used here, renamed or removed there"));
        for id in &ids {
            forget(id);
        }
    }

    #[test]
    fn f_wt_10_4_no_at_in_a_symbols_name_names_a_file_for_the_agent() {
        // TypeScript names a module or a method with any string, spaces
        // too, and another agent wrote it: Claude Code attaches the file an
        // `@path` after a space names (D-3022, D-3023).
        let defs = symbols::definitions(
            symbols::Lang::TypeScript,
            "declare module \"see @~/.ssh/id_rsa\" {\n  export function f(): void;\n}\n\
             class A {\n  \"read @/etc/passwd\"() {}\n}\n",
        );
        let found: Vec<SharedSymbol> = defs
            .iter()
            .map(|d| SharedSymbol {
                name: d.name.clone(),
                qualified: d.qualified.clone(),
                here: SymbolChange::Used,
                there: SymbolChange::Changed,
                renamed_to: None,
            })
            .collect();
        assert!(
            found.iter().any(|s| s.qualified.contains("@~/.ssh/id_rsa"))
                && found.iter().any(|s| s.qualified.contains("@/etc/passwd")),
            "{found:?}"
        );
        let text = note_text("tandem/other", &found);
        assert!(!text.contains("@~/") && !text.contains("@/etc"), "{text}");
        assert!(text.contains("\u{FF20}~/.ssh/id_rsa") && text.contains("\u{FF20}/etc/passwd"));
        assert!(text.ends_with(untrusted::DEFANGED), "{text}");
        // Plain names: just the fence, with no note.
        let plain = SharedSymbol {
            name: "refresh".into(),
            qualified: "Session.refresh".into(),
            here: SymbolChange::Used,
            there: SymbolChange::Changed,
            renamed_to: None,
        };
        assert!(!note_text("tandem/other", &[plain]).contains(untrusted::DEFANGED));
    }

    #[tokio::test]
    async fn f_wt_10_3_unrelated_work_shares_nothing() {
        let fx = fixture(&["one", "two"]).await;
        for (lane, name) in fx.lanes.iter().zip(["alpha", "beta"]) {
            std::fs::write(
                Path::new(&lane.root).join(format!("src/{name}.ts")),
                format!("export function {name}() {{ return 1; }}\n"),
            )
            .expect("write");
        }
        let bus = EventBus::new();
        let mut rx = bus.subscribe(Topic::App);
        on_checkpoint(&fx.store, &fx.git, &bus, &fx.lanes[0].root).await;
        assert!(rx.try_recv().is_err());
        let e = note(&fx.store, &fx.git, &fx.lanes[0].id, &fx.lanes[1].id)
            .await
            .expect_err("nothing shared");
        assert_eq!(e.code, ErrorCode::NotFound);
        for l in &fx.lanes {
            forget(l.id.as_str());
        }
    }

    const PARSE: &str =
        "export function parse(line: string): string {\n  return line.trim().toLowerCase();\n}\n";
    const PARSE_LINE: &str = "export function parseLine(line: string): string {\n  return line.trim().toLowerCase();\n}\n";

    /// Two worktrees, with `src/parse.ts` in their base.
    async fn parse_fixture() -> Fx {
        fixture_with(&["renamer", "user"], &[("src/parse.ts", PARSE)]).await
    }

    #[tokio::test]
    async fn f_wt_10_6_a_rename_is_announced_and_noted_as_a_rename() {
        let fx = parse_fixture().await;
        let (a, b) = (&fx.lanes[0], &fx.lanes[1]);
        std::fs::write(Path::new(&a.root).join("src/parse.ts"), PARSE_LINE).expect("write");
        std::fs::write(
            Path::new(&b.root).join("src/main.ts"),
            "import { parse } from './parse';\nexport const out = parse(' X ');\n",
        )
        .expect("write");
        let bus = EventBus::new();
        let mut rx = bus.subscribe(Topic::App);
        on_checkpoint(&fx.store, &fx.git, &bus, &a.root).await;
        let ev = overlap_in(&mut rx).expect("an overlap notice");
        assert_eq!(
            ev.payload["symbols"],
            serde_json::json!(["parse → parseLine"])
        );
        let text = note(&fx.store, &fx.git, &b.id, &a.id).await.expect("note");
        assert!(
            text.contains("- parse: used here, renamed to parseLine there"),
            "{text}"
        );
        // What Claim these files claims for the renamer, and why.
        let mine = index_of(&fx.git, a).await.expect("index");
        let theirs = index_of(&fx.git, b).await.expect("index");
        let (paths, intent) = claim_of(&mine, &shared(&mine, &theirs), "tandem/user");
        assert_eq!(paths, ["src/parse.ts"]);
        assert_eq!(
            intent,
            "Intent radar: renamed parse → parseLine, which tandem/user uses"
        );
        // The user's worktree changed no shared definition: nothing to claim.
        let (paths, _) = claim_of(&theirs, &shared(&theirs, &mine), "tandem/renamer");
        assert!(paths.is_empty());
        for l in &fx.lanes {
            forget(l.id.as_str());
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn f_wt_10_7_a_watched_edit_is_announced_without_a_checkpoint() {
        let fx = parse_fixture().await;
        let (a, b) = (&fx.lanes[0], &fx.lanes[1]);
        let bus = EventBus::new();
        let mut rx = bus.subscribe(Topic::App);
        // Both parsed (nothing shared yet), then watched.
        on_checkpoint(&fx.store, &fx.git, &bus, &a.root).await;
        assert!(rx.try_recv().is_err());
        let ctx = Context {
            store: fx.store.clone(),
            bus: bus.clone(),
            git: GitSource::Fixed(fx.git.clone()),
        };
        for lane in [a, b] {
            watch_with(lane, ctx.clone()).await;
            assert!(lock(&WATCHERS).contains_key(lane.id.as_str()));
        }
        // An edit in another language parses nothing.
        std::fs::write(Path::new(&a.root).join("NOTES.md"), "notes\n").expect("write");
        let quiet = tokio::time::timeout(Duration::from_secs(4), async {
            loop {
                if let Ok(ev) = rx.recv().await {
                    if ev.name.starts_with("radar:") {
                        return ev.name;
                    }
                }
            }
        })
        .await;
        assert!(quiet.is_err(), "{quiet:?}");
        // Mid-turn edits, no checkpoint: the rename and the use.
        std::fs::write(Path::new(&a.root).join("src/parse.ts"), PARSE_LINE).expect("write");
        std::fs::write(
            Path::new(&b.root).join("src/main.ts"),
            "import { parse } from './parse';\nexport const out = parse(' X ');\n",
        )
        .expect("write");
        let mut seen = (false, false);
        let found = tokio::time::timeout(Duration::from_secs(20), async {
            while !(seen.0 && seen.1) {
                if let Ok(ev) = rx.recv().await {
                    if ev.name == "radar:overlap" {
                        assert_eq!(
                            ev.payload["symbols"],
                            serde_json::json!(["parse → parseLine"])
                        );
                        seen.0 = true;
                    }
                    if ev.name == "radar:changed" {
                        seen.1 = true;
                    }
                }
            }
        })
        .await;
        assert!(found.is_ok(), "no watched notice: {seen:?}");
        for l in &fx.lanes {
            forget(l.id.as_str());
        }
        assert!(!lock(&WATCHERS).contains_key(a.id.as_str()));
    }

    #[test]
    fn f_wt_10_3_an_older_parse_that_finishes_last_keeps_the_newer_index() {
        // Checkpoints, Check overlap… and watched changes refresh alongside
        // each other, and the last to finish overwrote a newer index with an
        // older one (the review of 9a4e93ab, S2).
        let id = "radar-test-older-parse-finishes-last";
        let newer = SymbolIndex {
            unparsed: 2,
            ..SymbolIndex::default()
        };
        let older = SymbolIndex {
            unparsed: 1,
            ..SymbolIndex::default()
        };
        let newer = Arc::new(newer);
        assert_eq!(
            keep(id, "repo", 2, "base", (*newer).clone(), Default::default()),
            Some(newer.clone())
        );
        assert_eq!(
            keep(id, "repo", 1, "base", older, Default::default()),
            Some(newer.clone()),
            "the later parse's index stays"
        );
        assert_eq!(lock(&INDEXES).get(id).map(|e| e.index.clone()), Some(newer));
        forget(id);
    }

    #[tokio::test]
    async fn f_wt_10_3_a_watched_parse_that_changed_nothing_is_not_announced() {
        // A file rewritten with the same bytes still sent `radar:changed`,
        // and each one reloads an open Check overlap… (the review of
        // 9a4e93ab, S3).
        let fx = parse_fixture().await;
        let a = &fx.lanes[0];
        let bus = EventBus::new();
        let mut rx = bus.subscribe(Topic::App);
        let ctx = Context {
            store: fx.store.clone(),
            bus: bus.clone(),
            git: GitSource::Fixed(fx.git.clone()),
        };
        let mut changed = || {
            let mut n = 0;
            while let Ok(ev) = rx.try_recv() {
                n += usize::from(ev.name == "radar:changed");
            }
            n
        };
        // What the rail and Check overlap… show is what's shared: a
        // sibling uses `parse`.
        std::fs::write(
            Path::new(&fx.lanes[1].root).join("src/main.ts"),
            "import { parse } from './parse';\nexport const out = parse(' X ');\n",
        )
        .expect("write");
        std::fs::write(Path::new(&a.root).join("src/parse.ts"), PARSE_LINE).expect("write");
        rescan(a.id.as_str(), &ctx).await;
        assert_eq!(changed(), 1);
        std::fs::write(Path::new(&a.root).join("src/parse.ts"), PARSE_LINE).expect("write");
        rescan(a.id.as_str(), &ctx).await;
        assert_eq!(changed(), 0, "the same bytes changed nothing");
        for l in &fx.lanes {
            forget(l.id.as_str());
        }
    }

    #[tokio::test]
    async fn f_wt_10_9_a_worktree_being_discarded_isnt_parsed_or_watched() {
        let fx = parse_fixture().await;
        let a = &fx.lanes[0];
        std::fs::write(Path::new(&a.root).join("src/parse.ts"), PARSE_LINE).expect("write");
        assert!(!refresh(&fx.git, a).await.expect("index").symbols.is_empty());
        let held = hold(a.id.as_str());
        // Two discards at once each hold it: holds are counted.
        let again = hold(a.id.as_str());
        assert!(
            !lock(&INDEXES).contains_key(a.id.as_str()),
            "its index went"
        );
        assert!(refresh(&fx.git, a).await.expect("held").symbols.is_empty());
        assert!(
            !lock(&INDEXES).contains_key(a.id.as_str()),
            "nothing kept while held"
        );
        let ctx = Context {
            store: fx.store.clone(),
            bus: EventBus::new(),
            git: GitSource::Fixed(fx.git.clone()),
        };
        watch_with(a, ctx).await;
        assert!(
            !lock(&WATCHERS).contains_key(a.id.as_str()),
            "not watched while held"
        );
        drop(held);
        assert!(
            refresh(&fx.git, a).await.expect("held").symbols.is_empty(),
            "the other discard still holds it"
        );
        drop(again);
        assert!(!refresh(&fx.git, a).await.expect("index").symbols.is_empty());
        for l in &fx.lanes {
            forget(l.id.as_str());
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn f_wt_10_9_a_worktree_forgotten_during_its_watchers_walk_isnt_watched() {
        // The watcher was admitted after its folder walk whatever happened
        // meanwhile (the review of d3b838de, SF-A).
        let fx = parse_fixture().await;
        let a = &fx.lanes[0];
        let ctx = Context {
            store: fx.store.clone(),
            bus: EventBus::new(),
            git: GitSource::Fixed(fx.git.clone()),
        };
        forget(a.id.as_str());
        watch_with(a, ctx.clone()).await;
        assert!(!lock(&WATCHERS).contains_key(a.id.as_str()));
        // With its index kept, it's watched.
        refresh_sibling(&fx.git, a).await.expect("index");
        watch_with(a, ctx).await;
        assert!(lock(&WATCHERS).contains_key(a.id.as_str()));
        for l in &fx.lanes {
            forget(l.id.as_str());
        }
    }

    #[test]
    fn f_wt_10_8_the_claims_intent_says_what_the_sibling_did() {
        // "which X uses" was said when X changed the symbols too (the review
        // of 9a4e93ab).
        use SymbolChange::*;
        let changed = |name: &str| tandem_git::symbols::ChangedSymbol {
            name: name.into(),
            qualified: name.into(),
            kind: "function".into(),
            path: format!("src/{name}.ts"),
            line: 1,
            change: Changed,
            renamed_to: None,
        };
        let mine = SymbolIndex {
            symbols: vec![changed("parse"), changed("format")],
            ..SymbolIndex::default()
        };
        let pair = |name: &str, there| SharedSymbol {
            name: name.into(),
            qualified: name.into(),
            here: Changed,
            there,
            renamed_to: None,
        };
        let intent = |found: &[SharedSymbol]| claim_of(&mine, found, "tandem/b").1;
        assert_eq!(
            intent(&[pair("parse", Used)]),
            "Intent radar: changed parse, which tandem/b uses"
        );
        assert_eq!(
            intent(&[pair("parse", Changed)]),
            "Intent radar: changed parse, which tandem/b changes too"
        );
        assert_eq!(
            intent(&[pair("parse", Used), pair("format", Removed)]),
            "Intent radar: changed parse, changed format, which tandem/b uses or changes too"
        );
        // A renamed class's constructor moved with it: said once, by the
        // class (the review of 30d5bcad).
        let renamed = |qualified: &str, to: &str| tandem_git::symbols::ChangedSymbol {
            name: qualified.rsplit('.').next().unwrap_or(qualified).into(),
            qualified: qualified.into(),
            kind: "class".into(),
            path: "src/Session.java".into(),
            line: 1,
            change: Renamed,
            renamed_to: Some(to.into()),
        };
        let mine = SymbolIndex {
            symbols: vec![
                renamed("Session", "UserSession"),
                tandem_git::symbols::ChangedSymbol {
                    kind: "constructor".into(),
                    ..renamed("Session.Session", "UserSession.UserSession")
                },
            ],
            ..SymbolIndex::default()
        };
        let (paths, intent) = claim_of(
            &mine,
            &[SharedSymbol {
                name: "Session".into(),
                qualified: "Session".into(),
                here: Renamed,
                there: Used,
                renamed_to: Some("UserSession".into()),
            }],
            "tandem/b",
        );
        assert_eq!(paths, ["src/Session.java"]);
        assert_eq!(
            intent,
            "Intent radar: renamed Session → UserSession, which tandem/b uses"
        );
        // Only the member is shared (the sibling calls `s.refresh()` and
        // never names `Session`): its rename is the one said. The intent
        // read "Intent radar: , which tandem/b uses" (the review of
        // d06b9e3d, SF-2).
        let shared = |name: &str, qualified: &str, to: &str| SharedSymbol {
            name: name.into(),
            qualified: qualified.into(),
            here: Renamed,
            there: Used,
            renamed_to: Some(to.into()),
        };
        let mine = SymbolIndex {
            symbols: vec![
                renamed("Session", "UserSession"),
                renamed("Session.refresh", "UserSession.refresh"),
            ],
            ..SymbolIndex::default()
        };
        let refresh = shared("refresh", "Session.refresh", "UserSession.refresh");
        assert_eq!(
            claim_of(&mine, &[refresh], "tandem/b").1,
            "Intent radar: renamed Session.refresh → UserSession.refresh, which tandem/b uses"
        );
        // Renamed on its own as well: said beside its type's rename.
        let mine = SymbolIndex {
            symbols: vec![
                renamed("Session", "UserSession"),
                renamed("Session.refresh", "UserSession.reload"),
            ],
            ..SymbolIndex::default()
        };
        let found = [
            shared("Session", "Session", "UserSession"),
            shared("refresh", "Session.refresh", "UserSession.reload"),
        ];
        assert_eq!(
            claim_of(&mine, &found, "tandem/b").1,
            "Intent radar: renamed Session → UserSession, renamed Session.refresh → \
             UserSession.reload, which tandem/b uses"
        );
    }

    #[test]
    fn f_wt_10_9_a_worktree_gone_for_good_is_never_kept_again() {
        // A parse that started after the worktree went, from a list read
        // before (Check overlap… walking its siblings), kept its index and
        // could watch it again (the review of d06b9e3d, N-1).
        let empty = || (SymbolIndex::default(), symbols::ParseCache::default());
        let gone = "radar-gone-for-good";
        forget_for_good(gone);
        let later = STARTED.fetch_add(1, Ordering::Relaxed) + 1;
        let (index, cache) = empty();
        assert!(keep(gone, "repo", later, "base", index, cache).is_none());
        assert!(!lock(&INDEXES).contains_key(gone));
        // Any other fence goes once a newer parse is kept, which wins over
        // the older ones anyway.
        let id = "radar-fence-goes";
        forget(id);
        let newer = STARTED.fetch_add(1, Ordering::Relaxed) + 1;
        let (index, cache) = empty();
        assert!(keep(id, "repo", newer, "base", index, cache).is_some());
        assert!(!lock(&FORGOTTEN).contains_key(id));
        let (index, cache) = empty();
        assert!(keep(id, "repo", newer - 1, "base", index, cache).is_some());
        let kept = lock(&INDEXES).get(id).map(|e| e.started);
        forget(id);
        assert_eq!(kept, Some(newer), "an older parse doesn't replace it");
    }

    #[test]
    fn f_wt_10_7_a_refresh_compares_only_its_own_pairs() {
        // Every pair of the repository was compared, each index copied, at
        // every refresh (the review of d06b9e3d, SF-1).
        let parent = "radar-own-pairs";
        let index = || SymbolIndex {
            symbols: vec![tandem_git::symbols::ChangedSymbol {
                name: "parse".into(),
                qualified: "parse".into(),
                kind: "function".into(),
                path: "src/parse.ts".into(),
                line: 1,
                change: SymbolChange::Changed,
                renamed_to: None,
            }],
            ..SymbolIndex::default()
        };
        let ids = ["radar-own-a", "radar-own-b", "radar-own-c", "radar-own-d"];
        for id in ids {
            let n = STARTED.fetch_add(1, Ordering::Relaxed) + 1;
            keep(id, parent, n, "base", index(), Default::default());
        }
        COMPARED.with(|n| n.set(0));
        let got = shares_of(parent, ids[0]);
        // Its own three pairs, not the repository's six: comparing every
        // pair and keeping its own gave the same answer (the review of
        // d3b838de).
        let compared = COMPARED.with(std::cell::Cell::get);
        for id in ids {
            forget(id);
        }
        assert_eq!(got.keys().collect::<Vec<_>>(), [ids[1], ids[2], ids[3]]);
        assert!(got.values().all(|found| found.len() == 1));
        assert_eq!(compared, 3);
    }

    #[test]
    fn f_wt_10_9_a_worktree_made_again_at_a_folder_used_before_is_read_again() {
        // The store gives a worktree made at a folder used before its old
        // row's id, and the fence gone for good kept its radar off until a
        // restart (the review of d3b838de, MF-A).
        let empty = || (SymbolIndex::default(), symbols::ParseCache::default());
        let id = "radar-made-again";
        forget_for_good(id);
        // Begun before it came back: the old worktree's files.
        let stale = STARTED.fetch_add(1, Ordering::Relaxed) + 1;
        made(id, Some("b2"));
        let (index, cache) = empty();
        assert!(keep(id, "repo", stale, "b2", index, cache).is_none());
        // Begun after it, from a list read before: the old worktree's base
        // (the review of c30bb8bf, SF-1).
        let late = STARTED.fetch_add(1, Ordering::Relaxed) + 1;
        let (index, cache) = empty();
        assert!(keep(id, "repo", late, "b1", index, cache).is_none());
        let fresh = STARTED.fetch_add(1, Ordering::Relaxed) + 1;
        let (index, cache) = empty();
        assert!(keep(id, "repo", fresh, "b2", index, cache).is_some());
        forget_for_good(id);
        assert!(!lock(&PINNED).contains_key(id), "gone with the row");
        // A worktree never forgotten gets no fence from being made.
        let new = "radar-made-new";
        made(new, Some("b1"));
        assert!(!lock(&FORGOTTEN).contains_key(new));
        made(new, None);
        assert!(!lock(&PINNED).contains_key(new));
    }

    #[test]
    fn f_wt_10_5_a_worktree_made_again_tells_its_overlaps_again() {
        // Nothing forgot a pair's notice, so the same task done again in a
        // worktree made with its old id never told the overlap again (the
        // review of c30bb8bf, SF-2). Another pair's notice stays.
        let (x, y, z) = ("radar-warn-x", "radar-warn-y", "radar-warn-z");
        let names = vec!["login".to_string()];
        lock(&WARNED).extend([pair_key(x, y, &names), pair_key(y, z, &names)]);
        forget_for_good(x);
        made(x, Some("b2"));
        assert!(lock(&WARNED).insert(pair_key(x, y, &names)), "told again");
        assert!(!lock(&WARNED).insert(pair_key(y, z, &names)));
        unwarn(x);
        unwarn(z);
        forget(x);
    }

    #[test]
    fn f_wt_10_9_a_later_forget_never_lowers_a_fence_for_good() {
        // Remove repository: the watcher's last batch makes a rescan that no
        // longer finds the worktree, and its forget set the fence back to a
        // number later parses pass (the review of d3b838de, SF-A).
        let id = "radar-for-good-stays";
        forget_for_good(id);
        forget(id);
        let later = STARTED.fetch_add(1, Ordering::Relaxed) + 1;
        assert!(keep(
            id,
            "repo",
            later,
            "base",
            SymbolIndex::default(),
            Default::default()
        )
        .is_none());
        assert_eq!(lock(&FORGOTTEN).get(id), Some(&u64::MAX));
    }

    #[test]
    fn f_wt_10_2_a_worktree_has_one_follow_up_at_a_time() {
        // Every refresh that ran out of time started its own, and they ran
        // side by side (the review of d3b838de, SF-B); held only while it
        // waited, a refresh that ran out of time during its run still
        // started another (the review of c30bb8bf, SF-3).
        let id = "radar-follows-once";
        assert!(follows(id), "the first starts one");
        // Two more run out of time while it waits or runs: one more round.
        assert!(!follows(id));
        assert!(!follows(id));
        assert!(again(id));
        // Its own round ran out of time again: once more.
        assert!(!follows(id));
        assert!(again(id));
        // A round with nothing new ends it, and the next starts afresh.
        assert!(!again(id));
        assert!(!lock(&FOLLOWING).contains_key(id));
        assert!(follows(id));
        assert!(!again(id));
    }

    #[test]
    fn f_wt_10_7_a_watched_worktree_gives_its_place_only_once_idle() {
        // Each watch is an inotify instance and two threads, and idle
        // worktrees kept theirs until discarded (the review of 9a4e93ab,
        // S8). The one parsed least recently gave its place at once, so
        // nine worktrees read in turn rebuilt every watcher each round (the
        // review of 30d5bcad, S-3).
        let n = MAX_WATCHED_WORKTREES + 1;
        let dirs: Vec<tempfile::TempDir> =
            (0..n).map(|_| tempfile::tempdir().expect("tmp")).collect();
        let watcher = |i: usize| {
            let root = std::fs::canonicalize(dirs[i].path()).expect("canon");
            watch::watch(&root, None, WatchOptions::default(), Box::new(|_| {})).expect("watch")
        };
        let id = |i: usize| format!("wt{i}");
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        let mut watchers = HashMap::new();
        let mut built = 0;
        for round in 0..3u64 {
            for i in 0..n {
                let now = at(round * 20 + i as u64);
                if seen(&mut watchers, &id(i), now) || !room(&watchers, MAX_WATCHED_WORKTREES, now)
                {
                    continue;
                }
                built += 1;
                admit(&mut watchers, id(i), watcher(i), MAX_WATCHED_WORKTREES, now);
            }
        }
        assert_eq!(
            built, MAX_WATCHED_WORKTREES,
            "none built twice, the ninth never"
        );
        assert!(!watchers.contains_key(&id(n - 1)));
        // Once one has gone WATCH_IDLE without a parse, the ninth takes its
        // place, and only that one's.
        let later = at(60) + WATCH_IDLE;
        for i in 1..MAX_WATCHED_WORKTREES {
            assert!(seen(&mut watchers, &id(i), later));
        }
        assert!(room(&watchers, MAX_WATCHED_WORKTREES, later));
        admit(
            &mut watchers,
            id(n - 1),
            watcher(n - 1),
            MAX_WATCHED_WORKTREES,
            later,
        );
        assert!(watchers.contains_key(&id(n - 1)) && !watchers.contains_key(&id(0)));
        assert_eq!(watchers.len(), MAX_WATCHED_WORKTREES);
        assert!(!room(&watchers, MAX_WATCHED_WORKTREES, later));
    }

    #[test]
    fn f_wt_10_9_a_parse_begun_before_a_discard_isnt_kept_after_it() {
        // Kept, it brought the gone worktree's index back for good, and on
        // Linux a watcher on the missing folder (the review of 30d5bcad).
        let id = "radar-parse-outlives-discard";
        let started = STARTED.fetch_add(1, Ordering::Relaxed) + 1;
        drop(hold(id));
        let kept = keep(
            id,
            "repo",
            started,
            "base",
            SymbolIndex::default(),
            Default::default(),
        );
        let back = lock(&INDEXES).contains_key(id);
        // One begun after it is kept.
        let later = STARTED.fetch_add(1, Ordering::Relaxed) + 1;
        let kept_later = keep(
            id,
            "repo",
            later,
            "base",
            SymbolIndex::default(),
            Default::default(),
        );
        forget(id);
        assert!(
            kept.is_none() && !back,
            "begun before the discard, kept after it"
        );
        assert!(kept_later.is_some());
    }

    #[tokio::test]
    async fn f_wt_10_7_a_change_a_checkpoint_read_first_still_reaches_the_rail() {
        // Only a watched parse raised `radar:changed`, and only when it
        // changed the index itself: a checkpoint that read the change first
        // left the rail's marks standing (the review of 30d5bcad, S-2).
        let fx = parse_fixture().await;
        let (a, b) = (&fx.lanes[0], &fx.lanes[1]);
        let bus = EventBus::new();
        let mut rx = bus.subscribe(Topic::App);
        let ctx = Context {
            store: fx.store.clone(),
            bus: bus.clone(),
            git: GitSource::Fixed(fx.git.clone()),
        };
        std::fs::write(Path::new(&a.root).join("src/parse.ts"), PARSE_LINE).expect("write");
        std::fs::write(
            Path::new(&b.root).join("src/main.ts"),
            "import { parse } from './parse';\nexport const out = parse(' X ');\n",
        )
        .expect("write");
        on_checkpoint(&fx.store, &fx.git, &bus, &a.root).await;
        let ids: Vec<String> = fx.lanes.iter().map(|l| l.id.as_str().to_string()).collect();
        let marked = marks(&ids).len();
        let told = |rx: &mut tokio::sync::broadcast::Receiver<BusEvent>| {
            std::iter::from_fn(|| rx.try_recv().ok())
                .filter(|e| e.name == "radar:changed")
                .count()
        };
        let first = told(&mut rx);
        // `parse` put back; the next turn's checkpoint reads it before the
        // watched parse, which then finds nothing new.
        std::fs::write(Path::new(&a.root).join("src/parse.ts"), PARSE).expect("write");
        on_checkpoint(&fx.store, &fx.git, &bus, &a.root).await;
        let gone = marks(&ids).is_empty();
        let second = told(&mut rx);
        rescan(a.id.as_str(), &ctx).await;
        let after_rescan = told(&mut rx);
        for l in &fx.lanes {
            forget(l.id.as_str());
        }
        assert_eq!(marked, 2, "both marked");
        assert!(first >= 1, "the first share is told");
        assert!(gone, "nothing is shared any more");
        assert_eq!(
            second, 1,
            "the checkpoint tells the rail its marks are gone"
        );
        assert_eq!(after_rescan, 0, "and the watched parse adds nothing");
    }

    #[test]
    fn f_wt_10_7_each_watched_worktree_gets_its_share_of_folders() {
        // A quarter of the user's inotify watches, split eight ways.
        assert_eq!(share_of(Some(8_192)), 256);
        assert_eq!(share_of(Some(65_536)), 2_048);
        assert_eq!(share_of(Some(1_048_576)), MAX_WATCHED_DIRS);
        assert_eq!(share_of(Some(100)), MIN_WATCHED_DIRS);
        // Not Linux: one recursive watch a worktree.
        assert_eq!(share_of(None), MAX_WATCHED_DIRS);
    }

    #[test]
    fn f_wt_10_7_a_folder_renamed_or_removed_moves_code() {
        // `mv src lib` went unseen: neither path is in a parsed language
        // (the review of 9a4e93ab).
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        std::fs::create_dir(root.join("lib")).expect("mkdir");
        std::fs::create_dir(root.join("src")).expect("mkdir");
        std::fs::write(root.join("README"), "words").expect("write");
        let mut kept = HashMap::new();
        let mut moves = |rel: &str| moves_code(root, rel, &mut kept);
        assert!(moves("lib"), "a folder moved in");
        assert!(moves("gone"), "a folder moved away, or removed");
        assert!(moves("src/parse.ts"), "a parsed file");
        assert!(!moves("README"), "a file in no parsed language");
        // Gone, whatever its name: it may have been a folder (`mv gen.d ..`
        // went unseen; the review of 30d5bcad).
        assert!(moves("gen.d"), "a dotted folder moved away");
        assert!(moves("notes.md"), "a file removed");
    }

    #[test]
    fn f_wt_10_7_changes_in_ignored_folders_move_no_code() {
        // A build's files came and went in `target/` every 2 s, and on macOS
        // and Windows each one parsed the worktree again (the review of
        // d06b9e3d, N-6).
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        std::fs::write(
            root.join(".gitignore"),
            "target/
.next/
",
        )
        .expect("write");
        std::fs::create_dir_all(root.join("target/debug/build/out")).expect("mkdir");
        std::fs::create_dir_all(root.join(".next/types")).expect("mkdir");
        std::fs::create_dir(root.join("src")).expect("mkdir");
        let mut kept = HashMap::new();
        let mut moves = |rel: &str| moves_code(root, rel, &mut kept);
        assert!(!moves("target/debug/build/x.tmp"), "a temporary file gone");
        assert!(!moves("target/debug/build/out"), "a folder made");
        assert!(
            !moves("target/debug/build/out/bindings.rs"),
            "generated code"
        );
        assert!(!moves(".next/types/page.ts"), "generated types");
        // The ignored folder itself, made or removed, at a kept place.
        assert!(moves("target"));
        assert!(moves("src/gone"), "a folder gone from a kept one");
        assert!(moves("src/parse.ts"));
    }

    #[tokio::test]
    async fn f_wt_10_9_a_refused_discard_keeps_the_index() {
        // The hold was taken before the checks that refuse a discard, so a
        // refused one dropped the index and the watcher (the review of
        // 9a4e93ab).
        let fx = parse_fixture().await;
        let a = &fx.lanes[0];
        std::fs::write(Path::new(&a.root).join("src/parse.ts"), PARSE_LINE).expect("write");
        refresh(&fx.git, a).await.expect("index");
        let terms = crate::services::terminal::TerminalService::new(
            tandem_pty::PtyManager::new(),
            fx.store.clone(),
        );
        let refused = crate::services::lanes::remove(&fx.store, &terms, &fx.git, &a.id, None).await;
        assert_eq!(
            refused.map_err(|e| e.code),
            Err(ErrorCode::Conflict),
            "uncommitted changes refuse it"
        );
        assert!(lock(&INDEXES).contains_key(a.id.as_str()));
        for l in &fx.lanes {
            forget(l.id.as_str());
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn f_wt_10_7_a_watched_folder_renamed_is_read_again() {
        let fx = parse_fixture().await;
        let a = &fx.lanes[0];
        let bus = EventBus::new();
        let mut rx = bus.subscribe(Topic::App);
        assert!(refresh(&fx.git, a).await.expect("index").symbols.is_empty());
        let ctx = Context {
            store: fx.store.clone(),
            bus: bus.clone(),
            git: GitSource::Fixed(fx.git.clone()),
        };
        watch_with(a, ctx).await;
        assert!(lock(&WATCHERS).contains_key(a.id.as_str()));
        std::fs::rename(
            Path::new(&a.root).join("src"),
            Path::new(&a.root).join("lib"),
        )
        .expect("mv src lib");
        // Nothing is shared, so nothing is told: the index shows it.
        let read = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let index = lock(&INDEXES).get(a.id.as_str()).map(|e| e.index.clone());
                if index.is_some_and(|i| !i.symbols.is_empty()) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;
        assert!(read.is_ok(), "the renamed folder wasn't read again");
        assert!(rx.try_recv().is_err(), "nothing shared, nothing told");
        for l in &fx.lanes {
            forget(l.id.as_str());
        }
    }
}
