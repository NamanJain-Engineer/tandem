//! The browser (F-CD-17): web pages in sidebar tabs and panes. Each page is
//! a wry webview this service creates itself, not a Tauri webview, so no IPC
//! bridge is ever injected into it (D-206). The UI reports where a browser
//! sits (window coordinates) and whether it's shown; the service fits the
//! page into that place (device preview), decides which addresses may load
//! and reports each page's state as `browser:state`.
//!
//! The webviews live on the main thread behind [`BrowserHost`]
//! ([`wry_host::WryHost`] in the app, a fake in tests).

pub mod picker;
pub mod wry_host;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};
use tandem_core::error::{ErrorCode, IpcError};
use ts_rs::TS;

/// Window coordinates in CSS pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Device {
    #[default]
    Desktop,
    Phone,
    Tablet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum BrowserCmd {
    Back,
    Forward,
    Reload,
    Stop,
}

/// One page as the toolbar shows it (`browser:state`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BrowserState {
    pub browser_id: String,
    /// The committed URL: what the page really is, never what it says.
    pub url: String,
    pub title: String,
    pub can_back: bool,
    pub can_forward: bool,
    pub loading: bool,
    pub device: Device,
    pub rotated: bool,
    /// The page inside its place: the whole place on Desktop, else the
    /// device's size scaled down to fit.
    pub frame: Rect,
    /// The page zoom that fit took (1 on Desktop).
    pub zoom: f64,
    /// The last address the browser refused, for the address bar to say so.
    pub blocked: Option<String>,
    /// The element picker is on (F-CD-18).
    pub picking: bool,
    /// Grows with every change, so a reply or event older than the state
    /// a view already has is dropped (a command's reply can arrive after
    /// the events that followed it).
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Navigated {
    pub url: String,
}

/// What the page's webview reports.
#[derive(Debug, Clone, PartialEq)]
pub enum HostEvent {
    Loading {
        id: String,
    },
    /// The page at `url` replaced the old one (the address bar may show it).
    Committed {
        id: String,
        url: String,
    },
    /// The page moved within itself (`pushState`, a `#fragment`), with no
    /// load. Taken only within the shown page's origin.
    SameDocument {
        id: String,
        url: String,
    },
    Loaded {
        id: String,
        url: String,
        can_back: bool,
        can_forward: bool,
    },
    Title {
        id: String,
        title: String,
    },
    Blocked {
        id: String,
        url: String,
    },
    /// The picker's report: a navigation to `tandem-picker:`, taken and
    /// never loaded (F-CD-18).
    Picker {
        id: String,
        url: String,
    },
}

/// Whether the page may go to a URL (links, redirects, new windows).
pub type Policy = Arc<dyn Fn(&str) -> bool + Send + Sync>;
pub type EventSink = Arc<dyn Fn(HostEvent) + Send + Sync>;
/// Where `browser:state` goes.
pub type Emitter = Arc<dyn Fn(&BrowserState) + Send + Sync>;
/// Where `browser:picker` goes.
pub type PickerEmitter = Arc<dyn Fn(&picker::BrowserPicked) + Send + Sync>;
/// A script's value as JSON, or why there's none.
pub type EvalDone = Box<dyn FnOnce(Result<String, IpcError>) + Send>;

/// The webviews, on whatever thread their platform wants.
pub trait BrowserHost: Send + Sync {
    /// Creates the webview at `frame` (or moves the one `id` has).
    fn open(
        &self,
        id: &str,
        url: &str,
        frame: Rect,
        zoom: f64,
        visible: bool,
    ) -> Result<(), IpcError>;
    fn load(&self, id: &str, url: &str);
    fn command(&self, id: &str, cmd: BrowserCmd);
    fn place(&self, id: &str, frame: Rect, zoom: f64, visible: bool);
    fn close(&self, id: &str);
    /// Page `from` goes on as `to`.
    fn rename(&self, from: &str, to: &str);
    /// Runs `script` in the page (the element picker).
    fn eval(&self, id: &str, script: &str);
    /// Whether a navigation in a page's frame reaches the page policy
    /// (WebView2 reports only the top frame's), so the picker can report
    /// through one (D-210).
    fn sees_frame_navigations(&self) -> bool;
    /// Runs `script` in the page and hands back its value (the E2E harness).
    fn eval_result(&self, id: &str, script: &str, done: EvalDone);
    /// Cookies, storage and cache of the browser profile.
    fn clear_data(&self);
}

pub const PHONE: (f64, f64) = (390.0, 844.0);
pub const TABLET: (f64, f64) = (820.0, 1180.0);
/// Room around a device frame, and above it for its size label.
const DEVICE_MARGIN: f64 = 12.0;
const DEVICE_LABEL: f64 = 24.0;

/// Where the page goes inside `bounds`, and at what zoom: all of it on
/// Desktop; a device's CSS size scaled down (never up) and centred
/// otherwise.
pub fn fit(bounds: Rect, device: Device, rotated: bool) -> (Rect, f64) {
    let size = match device {
        Device::Desktop => return (bounds, 1.0),
        Device::Phone => PHONE,
        Device::Tablet => TABLET,
    };
    let (w, h) = if rotated { (size.1, size.0) } else { size };
    let room_w = (bounds.width - 2.0 * DEVICE_MARGIN).max(1.0);
    let room_h = (bounds.height - DEVICE_LABEL - DEVICE_MARGIN).max(1.0);
    let zoom = (room_w / w).min(room_h / h).min(1.0);
    let (fw, fh) = ((w * zoom).floor(), (h * zoom).floor());
    let frame = Rect {
        x: (bounds.x + (bounds.width - fw) / 2.0).floor(),
        y: bounds.y + DEVICE_LABEL,
        width: fw,
        height: fh,
    };
    (frame, zoom)
}

pub const BLANK: &str = "about:blank";

fn invalid(message: impl Into<String>) -> IpcError {
    IpcError::new(ErrorCode::Invalid, message).with_stage("browser")
}

/// The scheme of `input` if it starts with one (`localhost:5173` is a host
/// and a port, not a scheme).
fn scheme_of(input: &str) -> Option<String> {
    let (head, rest) = input.split_once(':')?;
    let mut chars = head.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic()
        || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return None;
    }
    let port_like = rest
        .split(['/', '?', '#'])
        .next()
        .is_some_and(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    if port_like && !rest.starts_with("//") {
        return None;
    }
    Some(head.to_ascii_lowercase())
}

/// `example.com`, `localhost`, `127.0.0.1:8080/x`, `[::1]:3000`.
fn looks_like_host(input: &str) -> bool {
    if input.chars().any(char::is_whitespace) {
        return false;
    }
    let host_port = input.split(['/', '?', '#']).next().unwrap_or("");
    let host = if host_port.starts_with('[') {
        return host_port.contains(']');
    } else {
        host_port.rsplit_once(':').map_or(host_port, |(h, p)| {
            if p.chars().all(|c| c.is_ascii_digit()) {
                h
            } else {
                host_port
            }
        })
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.contains('.')
        && !host.starts_with('.')
        && !host.ends_with('.')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
}

fn search_url(engine: &str, query: &str) -> String {
    let base = match engine {
        "google" => "https://www.google.com/search?q=",
        "bing" => "https://www.bing.com/search?q=",
        "kagi" => "https://kagi.com/search?q=",
        _ => "https://duckduckgo.com/?q=",
    };
    let q: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    format!("{base}{q}")
}

/// Whether a `file:` URL names something inside one of `roots`.
fn file_inside(url: &url::Url, roots: &[PathBuf]) -> bool {
    let Ok(path) = url.to_file_path() else {
        return false;
    };
    let path = std::fs::canonicalize(&path).unwrap_or(path);
    roots.iter().any(|root| {
        let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.clone());
        path.starts_with(root)
    })
}

/// An iframe's `srcdoc` document: the page's own content, in its origin.
const SRCDOC: &str = "about:srcdoc";

/// Whether the page may go to `url`: http and https, `file:` inside a
/// workspace, and the blank page (and an iframe's `srcdoc`); nothing else
/// (`javascript:`, `data:`, `blob:`, `tandem:`, the app's own schemes…).
pub fn allowed(url: &str, roots: &[PathBuf]) -> bool {
    if url == BLANK || url == SRCDOC {
        return true;
    }
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    match parsed.scheme() {
        "http" | "https" => parsed.host_str().is_some(),
        "file" => file_inside(&parsed, roots),
        _ => false,
    }
}

/// Whether `to` is in the same document's origin as `from`: a web origin,
/// or the same local file.
fn same_origin(from: &str, to: &str) -> bool {
    let (Ok(a), Ok(b)) = (url::Url::parse(from), url::Url::parse(to)) else {
        return false;
    };
    match (a.scheme(), b.scheme()) {
        ("file", "file") => a.path() == b.path(),
        _ => a.origin().is_tuple() && a.origin() == b.origin(),
    }
}

/// What the address bar's `input` opens: an address, a bare host as
/// `http://`, or a search with `engine`. Blocked schemes are refused.
pub fn resolve_input(input: &str, engine: &str, roots: &[PathBuf]) -> Result<String, IpcError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(invalid("Enter an address or something to search for"));
    }
    if let Some(scheme) = scheme_of(input) {
        let parsed = url::Url::parse(input).map_err(|_| invalid("That isn't an address"))?;
        return match scheme.as_str() {
            "http" | "https" if parsed.host_str().is_some() => Ok(parsed.to_string()),
            "http" | "https" => Err(invalid("That isn't an address")),
            "file" if file_inside(&parsed, roots) => Ok(parsed.to_string()),
            "file" => Err(invalid(
                "Only files inside your workspaces open in the browser",
            )),
            other => Err(invalid(format!(
                "{other}: addresses can't be opened in the browser"
            ))),
        };
    }
    if looks_like_host(input) {
        let url = url::Url::parse(&format!("http://{input}"))
            .map_err(|_| invalid("That isn't an address"))?;
        return Ok(url.to_string());
    }
    Ok(search_url(engine, input))
}

struct Entry {
    state: BrowserState,
    bounds: Rect,
    visible: bool,
    /// The picker session's nonce while it's on.
    picker: Option<String>,
    /// What's picked in the page now.
    picked: Vec<picker::PickedElement>,
}

pub struct BrowserService {
    host: Arc<dyn BrowserHost>,
    entries: Mutex<HashMap<String, Entry>>,
    roots: Arc<RwLock<Vec<PathBuf>>>,
    emit: Mutex<Option<Emitter>>,
    emit_picked: Mutex<Option<PickerEmitter>>,
    seq: AtomicU64,
}

impl BrowserService {
    /// `make_host` gets the navigation policy and the event sink the host
    /// must use.
    pub fn new(make_host: impl FnOnce(Policy, EventSink) -> Arc<dyn BrowserHost>) -> Arc<Self> {
        let roots: Arc<RwLock<Vec<PathBuf>>> = Arc::default();
        Arc::new_cyclic(|weak: &std::sync::Weak<BrowserService>| {
            let policy_roots = roots.clone();
            let policy: Policy = Arc::new(move |url: &str| {
                let roots = policy_roots.read().map(|r| r.clone()).unwrap_or_default();
                allowed(url, &roots)
            });
            let me = weak.clone();
            let sink: EventSink = Arc::new(move |event| {
                if let Some(service) = me.upgrade() {
                    service.on_event(event);
                }
            });
            BrowserService {
                host: make_host(policy, sink),
                entries: Mutex::default(),
                roots,
                emit: Mutex::default(),
                emit_picked: Mutex::default(),
                seq: AtomicU64::new(0),
            }
        })
    }

    /// Where `browser:state` goes.
    pub fn set_emitter(&self, emit: Emitter) {
        *self.emit.lock().unwrap_or_else(|e| e.into_inner()) = Some(emit);
    }

    /// Where `browser:picker` goes.
    pub fn set_picker_emitter(&self, emit: PickerEmitter) {
        *self.emit_picked.lock().unwrap_or_else(|e| e.into_inner()) = Some(emit);
    }

    fn publish_picked(&self, id: &str, elements: Vec<picker::PickedElement>) {
        let emit = self
            .emit_picked
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(emit) = emit {
            emit(&picker::BrowserPicked {
                browser_id: id.to_string(),
                elements,
            });
        }
    }

    /// The workspaces' folders: `file:` pages inside them may load.
    pub fn set_roots(&self, roots: Vec<PathBuf>) {
        *self.roots.write().unwrap_or_else(|e| e.into_inner()) = roots;
    }

    fn roots(&self) -> Vec<PathBuf> {
        self.roots.read().map(|r| r.clone()).unwrap_or_default()
    }

    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn publish(&self, state: &BrowserState) {
        let emit = self.emit.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(emit) = emit {
            emit(state);
        }
    }

    fn with_entry<T>(&self, id: &str, f: impl FnOnce(&mut Entry) -> T) -> Result<T, IpcError> {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let entry = entries.get_mut(id).ok_or_else(|| {
            IpcError::new(ErrorCode::NotFound, "That browser tab is closed").with_stage("browser")
        })?;
        Ok(f(entry))
    }

    /// `browser_open`: a new page at `url` (blank without one), or the open
    /// one moved to `bounds`.
    pub fn open(
        &self,
        id: &str,
        url: Option<&str>,
        bounds: Rect,
        visible: bool,
    ) -> Result<BrowserState, IpcError> {
        let existing = {
            let entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            entries.get(id).map(|e| e.state.clone())
        };
        if let Some(state) = existing {
            self.set_bounds(id, bounds, visible)?;
            return self.with_entry(id, |e| e.state.clone()).or(Ok(state));
        }
        let url = match url.map(str::trim).filter(|u| !u.is_empty()) {
            Some(u) if allowed(u, &self.roots()) => u.to_string(),
            Some(u) => return Err(invalid(format!("{u} can't be opened in the browser"))),
            None => BLANK.to_string(),
        };
        let (frame, zoom) = fit(bounds, Device::Desktop, false);
        self.host.open(id, &url, frame, zoom, visible)?;
        let state = BrowserState {
            browser_id: id.to_string(),
            url: if url == BLANK {
                String::new()
            } else {
                url.clone()
            },
            title: String::new(),
            can_back: false,
            can_forward: false,
            loading: url != BLANK,
            device: Device::Desktop,
            rotated: false,
            frame,
            zoom,
            blocked: None,
            picking: false,
            seq: self.next_seq(),
        };
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                id.to_string(),
                Entry {
                    state: state.clone(),
                    bounds,
                    visible,
                    picker: None,
                    picked: Vec::new(),
                },
            );
        self.publish(&state);
        Ok(state)
    }

    /// `browser_navigate`: what the address bar's input opens.
    pub fn navigate(&self, id: &str, input: &str, engine: &str) -> Result<Navigated, IpcError> {
        let resolved = resolve_input(input, engine, &self.roots());
        let state = self.with_entry(id, |e| {
            e.state.blocked = resolved.as_ref().err().map(|_| input.trim().to_string());
            e.state.seq = self.next_seq();
            e.state.clone()
        })?;
        self.publish(&state);
        let url = resolved?;
        self.host.load(id, &url);
        Ok(Navigated { url })
    }

    pub fn command(&self, id: &str, cmd: BrowserCmd) -> Result<(), IpcError> {
        self.with_entry(id, |_| ())?;
        self.host.command(id, cmd);
        Ok(())
    }

    /// `browser_set_bounds`: the page follows its place, fitted for the
    /// device preview.
    pub fn set_bounds(&self, id: &str, bounds: Rect, visible: bool) -> Result<(), IpcError> {
        let (state, changed) = self.with_entry(id, |e| {
            let (frame, zoom) = fit(bounds, e.state.device, e.state.rotated);
            let changed = e.state.frame != frame || e.state.zoom != zoom;
            e.bounds = bounds;
            e.visible = visible;
            e.state.frame = frame;
            e.state.zoom = zoom;
            if changed {
                e.state.seq = self.next_seq();
            }
            (e.state.clone(), changed)
        })?;
        self.host.place(id, state.frame, state.zoom, visible);
        if changed {
            self.publish(&state);
        }
        Ok(())
    }

    pub fn set_device(
        &self,
        id: &str,
        device: Device,
        rotated: bool,
    ) -> Result<BrowserState, IpcError> {
        let (state, visible) = self.with_entry(id, |e| {
            e.state.device = device;
            e.state.rotated = rotated && device != Device::Desktop;
            let (frame, zoom) = fit(e.bounds, device, e.state.rotated);
            e.state.frame = frame;
            e.state.zoom = zoom;
            e.state.seq = self.next_seq();
            (e.state.clone(), e.visible)
        })?;
        self.host.place(id, state.frame, state.zoom, visible);
        self.publish(&state);
        Ok(state)
    }

    /// `browser_move`: the page goes on under `to`, history and all (a pane
    /// opened in the sidebar, a sidebar tab moved to a pane).
    pub fn rename(&self, from: &str, to: &str) -> Result<BrowserState, IpcError> {
        let state = {
            let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            if entries.contains_key(to) {
                return Err(
                    IpcError::new(ErrorCode::Conflict, "That place already has a page")
                        .with_stage("browser"),
                );
            }
            let mut entry = entries.remove(from).ok_or_else(|| {
                IpcError::new(ErrorCode::NotFound, "That browser tab is closed")
                    .with_stage("browser")
            })?;
            entry.state.browser_id = to.to_string();
            entry.state.seq = self.next_seq();
            let state = entry.state.clone();
            entries.insert(to.to_string(), entry);
            state
        };
        self.host.rename(from, to);
        self.publish(&state);
        Ok(state)
    }

    pub fn close(&self, id: &str) {
        let had = self
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id)
            .is_some();
        if had {
            self.host.close(id);
        }
    }

    /// `browser_picker`: the element picker on (a new session, an empty
    /// selection) or off.
    pub fn picker(&self, id: &str, on: bool) -> Result<BrowserState, IpcError> {
        let frames = self.host.sees_frame_navigations();
        let (state, script) = self.with_entry(id, |e| {
            let script = if on {
                let nonce = picker::nonce();
                let script = picker::script(&nonce, frames);
                // A session still on ends first, so only one ever answers.
                let stop = e.picker.replace(nonce).map(|old| picker::stop_script(&old));
                e.picked.clear();
                Some(stop.map_or(script.clone(), |stop| format!("{stop}\n{script}")))
            } else {
                e.picker.take().map(|nonce| picker::stop_script(&nonce))
            };
            e.state.picking = on;
            e.state.seq = self.next_seq();
            (e.state.clone(), script)
        })?;
        if let Some(script) = script {
            self.host.eval(id, &script);
        }
        self.publish(&state);
        if on {
            self.publish_picked(id, Vec::new());
        }
        Ok(state)
    }

    /// `browser_picker_brief`: the Markdown an agent gets (F-CD-18).
    pub fn brief(&self, id: &str, note: &str) -> Result<String, IpcError> {
        self.with_entry(id, |e| picker::brief(&e.state.url, note, &e.picked))
    }

    /// `test_browser_eval`: a script's value from the page.
    pub fn eval_result(&self, id: &str, script: &str, done: EvalDone) -> Result<(), IpcError> {
        self.with_entry(id, |_| ())?;
        self.host.eval_result(id, script, done);
        Ok(())
    }

    pub fn clear_data(&self) {
        self.host.clear_data();
    }

    /// The page's state, if it's open.
    pub fn state(&self, id: &str) -> Option<BrowserState> {
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .map(|e| e.state.clone())
    }

    fn on_event(&self, event: HostEvent) {
        let id = match &event {
            HostEvent::Loading { id }
            | HostEvent::Committed { id, .. }
            | HostEvent::SameDocument { id, .. }
            | HostEvent::Loaded { id, .. }
            | HostEvent::Title { id, .. }
            | HostEvent::Blocked { id, .. }
            | HostEvent::Picker { id, .. } => id.clone(),
        };
        // What the picker needs after this: its script again (a new page
        // while it's on), and the selection to send.
        let mut reinject = None;
        let mut picked = None;
        let frames = self.host.sees_frame_navigations();
        let Ok(state) = self.with_entry(&id, |e| {
            match event {
                // Not the URL yet: until the new page commits, the old one
                // is still what's on screen.
                HostEvent::Loading { .. } => {
                    e.state.loading = true;
                    e.state.blocked = None;
                }
                HostEvent::Committed { url, .. } => {
                    e.state.url = if url == BLANK { String::new() } else { url };
                    // The selection was the old page's.
                    if !e.picked.is_empty() {
                        e.picked.clear();
                        picked = Some(Vec::new());
                    }
                }
                // Never another site: a report that doesn't fit what's
                // shown waits for that page's own commit.
                HostEvent::SameDocument { url, .. } => {
                    if same_origin(&e.state.url, &url) {
                        e.state.url = url;
                    }
                }
                HostEvent::Loaded {
                    url,
                    can_back,
                    can_forward,
                    ..
                } => {
                    e.state.loading = false;
                    e.state.url = if url == BLANK { String::new() } else { url };
                    e.state.can_back = can_back;
                    e.state.can_forward = can_forward;
                    reinject = e
                        .picker
                        .as_deref()
                        .map(|nonce| picker::script(nonce, frames));
                }
                HostEvent::Title { title, .. } => e.state.title = title,
                HostEvent::Blocked { url, .. } => e.state.blocked = Some(url),
                // A report from another session, or the page's own, changes
                // nothing.
                HostEvent::Picker { url, .. } => {
                    let report = e
                        .picker
                        .as_deref()
                        .and_then(|nonce| picker::parse(&url, nonce));
                    if let Some(report) = report {
                        e.picked = report.elements.clone();
                        picked = Some(report.elements);
                        if report.stopped {
                            e.picker = None;
                            e.state.picking = false;
                        }
                    }
                }
            }
            e.state.seq = self.next_seq();
            e.state.clone()
        }) else {
            return;
        };
        if let Some(script) = reinject {
            self.host.eval(&id, &script);
        }
        if let Some(elements) = picked {
            self.publish_picked(&id, elements);
        }
        self.publish(&state);
    }
}

#[cfg(test)]
pub mod testing {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    pub enum Call {
        Open {
            id: String,
            url: String,
            frame: Rect,
            zoom: f64,
            visible: bool,
        },
        Load {
            id: String,
            url: String,
        },
        Command {
            id: String,
            cmd: BrowserCmd,
        },
        Place {
            id: String,
            frame: Rect,
            zoom: f64,
            visible: bool,
        },
        Close {
            id: String,
        },
        Rename {
            from: String,
            to: String,
        },
        Eval {
            id: String,
            script: String,
        },
        ClearData,
    }

    /// Records what the service asks of the webviews; tests play the
    /// webviews' events through `sink`.
    #[derive(Default)]
    pub struct FakeHost {
        pub calls: Mutex<Vec<Call>>,
        pub policy: Mutex<Option<Policy>>,
        pub sink: Mutex<Option<EventSink>>,
        pub unavailable: bool,
        /// Like WebView2: a frame's navigations go unseen.
        pub no_frame_navigations: std::sync::atomic::AtomicBool,
    }

    impl FakeHost {
        pub fn calls(&self) -> Vec<Call> {
            self.calls.lock().expect("calls").clone()
        }

        pub fn send(&self, event: HostEvent) {
            let sink = self.sink.lock().expect("sink").clone().expect("a sink");
            sink(event);
        }

        pub fn allows(&self, url: &str) -> bool {
            let policy = self
                .policy
                .lock()
                .expect("policy")
                .clone()
                .expect("a policy");
            policy(url)
        }
    }

    impl BrowserHost for FakeHost {
        fn open(
            &self,
            id: &str,
            url: &str,
            frame: Rect,
            zoom: f64,
            visible: bool,
        ) -> Result<(), IpcError> {
            if self.unavailable {
                return Err(IpcError::new(ErrorCode::Unavailable, "no webviews here"));
            }
            self.calls.lock().expect("calls").push(Call::Open {
                id: id.into(),
                url: url.into(),
                frame,
                zoom,
                visible,
            });
            Ok(())
        }
        fn load(&self, id: &str, url: &str) {
            self.calls.lock().expect("calls").push(Call::Load {
                id: id.into(),
                url: url.into(),
            });
        }
        fn command(&self, id: &str, cmd: BrowserCmd) {
            self.calls
                .lock()
                .expect("calls")
                .push(Call::Command { id: id.into(), cmd });
        }
        fn place(&self, id: &str, frame: Rect, zoom: f64, visible: bool) {
            self.calls.lock().expect("calls").push(Call::Place {
                id: id.into(),
                frame,
                zoom,
                visible,
            });
        }
        fn close(&self, id: &str) {
            self.calls
                .lock()
                .expect("calls")
                .push(Call::Close { id: id.into() });
        }
        fn eval(&self, id: &str, script: &str) {
            self.calls.lock().expect("calls").push(Call::Eval {
                id: id.into(),
                script: script.into(),
            });
        }
        fn eval_result(&self, id: &str, script: &str, done: EvalDone) {
            self.eval(id, script);
            done(Ok("null".into()));
        }
        fn sees_frame_navigations(&self) -> bool {
            !self
                .no_frame_navigations
                .load(std::sync::atomic::Ordering::SeqCst)
        }
        fn rename(&self, from: &str, to: &str) {
            self.calls.lock().expect("calls").push(Call::Rename {
                from: from.into(),
                to: to.into(),
            });
        }
        fn clear_data(&self) {
            self.calls.lock().expect("calls").push(Call::ClearData);
        }
    }

    /// A service over a fake host, and the states it published.
    pub fn service() -> (
        Arc<BrowserService>,
        Arc<FakeHost>,
        Arc<Mutex<Vec<BrowserState>>>,
    ) {
        let host = Arc::new(FakeHost::default());
        let h = host.clone();
        let service = BrowserService::new(move |policy, sink| {
            *h.policy.lock().expect("policy") = Some(policy);
            *h.sink.lock().expect("sink") = Some(sink);
            h
        });
        let published: Arc<Mutex<Vec<BrowserState>>> = Arc::default();
        let p = published.clone();
        service.set_emitter(Arc::new(move |s: &BrowserState| {
            p.lock().expect("published").push(s.clone())
        }));
        (service, host, published)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{service, Call};
    use super::*;

    const DDG: &str = "duckduckgo";
    const PLACE: Rect = Rect {
        x: 100.0,
        y: 50.0,
        width: 800.0,
        height: 600.0,
    };

    #[test]
    fn f_cd_17_1_a_bare_host_with_a_port_loads_over_http() {
        assert_eq!(
            resolve_input("localhost:5173", DDG, &[]).expect("ok"),
            "http://localhost:5173/"
        );
        assert_eq!(
            resolve_input("  localhost:3000/app?x=1 ", DDG, &[]).expect("ok"),
            "http://localhost:3000/app?x=1"
        );
        assert_eq!(
            resolve_input("example.com", DDG, &[]).expect("ok"),
            "http://example.com/"
        );
        assert_eq!(
            resolve_input("127.0.0.1:8080", DDG, &[]).expect("ok"),
            "http://127.0.0.1:8080/"
        );
        assert_eq!(
            resolve_input("[::1]:3000", DDG, &[]).expect("ok"),
            "http://[::1]:3000/"
        );
        assert_eq!(
            resolve_input("https://vibezeo.com/demo", DDG, &[]).expect("ok"),
            "https://vibezeo.com/demo"
        );
    }

    #[test]
    fn f_cd_17_3_javascript_tandem_and_other_schemes_are_blocked() {
        for input in [
            "javascript:alert(1)",
            "JavaScript:alert(document.cookie)",
            "tandem://open?x=1",
            "tauri://localhost",
            "ipc://localhost/fs_read",
            "data:text/html,<script>alert(1)</script>",
            "blob:http://x/1",
            "about:config",
            "chrome://settings",
            "view-source:http://example.com",
            "mailto:me@example.com",
        ] {
            let err = resolve_input(input, DDG, &[]).expect_err(input);
            assert_eq!(err.code, ErrorCode::Invalid, "{input}");
            assert!(!allowed(input, &[]), "{input}");
        }
        // An iframe's srcdoc is the page's own content; it can't be typed.
        assert!(allowed("about:srcdoc", &[]));
        assert!(resolve_input("about:srcdoc", DDG, &[]).is_err());
    }

    #[test]
    fn f_cd_17_anything_else_searches_with_the_chosen_engine() {
        assert_eq!(
            resolve_input("rust traits", DDG, &[]).expect("ok"),
            "https://duckduckgo.com/?q=rust+traits"
        );
        assert_eq!(
            resolve_input("vite", "google", &[]).expect("ok"),
            "https://www.google.com/search?q=vite"
        );
        assert_eq!(
            resolve_input("a&b=c", "kagi", &[]).expect("ok"),
            "https://kagi.com/search?q=a%26b%3Dc"
        );
        assert!(resolve_input("", DDG, &[]).is_err());
    }

    #[test]
    fn f_cd_17_file_pages_open_only_inside_a_workspace() {
        let dir = tempfile::tempdir().expect("tmp");
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(&ws).expect("mkdir");
        std::fs::write(ws.join("index.html"), "<p>hi</p>").expect("write");
        std::fs::write(dir.path().join("outside.html"), "<p>no</p>").expect("write");
        let roots = vec![ws.clone()];
        let inside = url::Url::from_file_path(ws.join("index.html"))
            .expect("url")
            .to_string();
        let outside = url::Url::from_file_path(dir.path().join("outside.html"))
            .expect("url")
            .to_string();
        assert_eq!(resolve_input(&inside, DDG, &roots).expect("inside"), inside);
        assert!(allowed(&inside, &roots));
        assert!(resolve_input(&outside, DDG, &roots).is_err());
        assert!(!allowed(&outside, &roots));
        assert!(!allowed("file:///etc/passwd", &roots));
    }

    #[test]
    fn f_cd_17_device_preview_fits_the_device_into_its_place_and_never_zooms_in() {
        let (frame, zoom) = fit(PLACE, Device::Desktop, false);
        assert_eq!((frame, zoom), (PLACE, 1.0));
        // A phone fits at full size into 800×600? No: 844 tall needs scaling.
        let (frame, zoom) = fit(PLACE, Device::Phone, false);
        assert!(zoom < 1.0);
        assert!(frame.height <= 600.0 - DEVICE_LABEL - DEVICE_MARGIN);
        assert!((frame.width / frame.height - 390.0 / 844.0).abs() < 0.01);
        assert_eq!(frame.y, PLACE.y + DEVICE_LABEL);
        // Rotated it's landscape.
        let (rot, _) = fit(PLACE, Device::Phone, true);
        assert!(rot.width > rot.height);
        // A big place never zooms a phone in.
        let big = Rect {
            x: 0.0,
            y: 0.0,
            width: 3000.0,
            height: 2000.0,
        };
        let (frame, zoom) = fit(big, Device::Phone, false);
        assert_eq!(zoom, 1.0);
        assert_eq!((frame.width, frame.height), PHONE);
        assert_eq!(frame.x, (3000.0 - 390.0) / 2.0);
    }

    #[test]
    fn f_cd_17_the_address_bar_shows_the_committed_url_not_what_was_typed() {
        let (svc, host, published) = service();
        svc.open("b1", None, PLACE, true).expect("open");
        assert_eq!(
            host.calls()[0],
            Call::Open {
                id: "b1".into(),
                url: BLANK.into(),
                frame: PLACE,
                zoom: 1.0,
                visible: true
            }
        );
        let to = svc.navigate("b1", "localhost:5173", DDG).expect("navigate");
        assert_eq!(to.url, "http://localhost:5173/");
        assert_eq!(
            host.calls()[1],
            Call::Load {
                id: "b1".into(),
                url: "http://localhost:5173/".into()
            }
        );
        // The page redirects; the state follows the webview, not the input.
        host.send(HostEvent::Loading { id: "b1".into() });
        // Still loading: the address bar keeps what's on screen.
        assert_eq!(
            published
                .lock()
                .expect("published")
                .last()
                .map(|s| s.url.clone()),
            Some(String::new())
        );
        host.send(HostEvent::Committed {
            id: "b1".into(),
            url: "http://localhost:5173/".into(),
        });
        assert_eq!(
            published
                .lock()
                .expect("published")
                .last()
                .map(|s| s.url.clone())
                .as_deref(),
            Some("http://localhost:5173/")
        );
        host.send(HostEvent::Loaded {
            id: "b1".into(),
            url: "http://localhost:5173/welcome".into(),
            can_back: true,
            can_forward: false,
        });
        host.send(HostEvent::Title {
            id: "b1".into(),
            title: "Fixtures".into(),
        });
        let last = published
            .lock()
            .expect("published")
            .last()
            .cloned()
            .expect("a state");
        assert_eq!(last.url, "http://localhost:5173/welcome");
        assert_eq!(last.title, "Fixtures");
        assert!(last.can_back && !last.loading);
        // Each state is newer than the one before, so a view drops stale ones.
        let seqs: Vec<u64> = published
            .lock()
            .expect("published")
            .iter()
            .map(|s| s.seq)
            .collect();
        assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");
    }

    #[test]
    fn f_cd_17_a_page_moving_within_itself_updates_the_address_but_never_to_another_site() {
        let (svc, host, published) = service();
        let last_url = || {
            published
                .lock()
                .expect("published")
                .last()
                .map(|s| s.url.clone())
                .unwrap_or_default()
        };
        svc.open("b1", Some("http://localhost:5173/"), PLACE, true)
            .expect("open");
        host.send(HostEvent::Committed {
            id: "b1".into(),
            url: "http://localhost:5173/".into(),
        });
        host.send(HostEvent::SameDocument {
            id: "b1".into(),
            url: "http://localhost:5173/settings#billing".into(),
        });
        assert_eq!(last_url(), "http://localhost:5173/settings#billing");
        // Another origin isn't a move within the page: what's shown stays.
        for other in [
            "https://bank.example/login",
            "http://localhost:5174/",
            "https://localhost:5173/",
        ] {
            host.send(HostEvent::SameDocument {
                id: "b1".into(),
                url: other.into(),
            });
            assert_eq!(
                last_url(),
                "http://localhost:5173/settings#billing",
                "{other}"
            );
        }
        // A local file moves within itself only.
        host.send(HostEvent::Committed {
            id: "b1".into(),
            url: "file:///ws/docs/index.html".into(),
        });
        host.send(HostEvent::SameDocument {
            id: "b1".into(),
            url: "file:///ws/docs/index.html#intro".into(),
        });
        assert_eq!(last_url(), "file:///ws/docs/index.html#intro");
        host.send(HostEvent::SameDocument {
            id: "b1".into(),
            url: "file:///etc/passwd".into(),
        });
        assert_eq!(last_url(), "file:///ws/docs/index.html#intro");
    }

    #[test]
    fn f_cd_17_3_a_blocked_address_never_reaches_the_page_and_says_so() {
        let (svc, host, published) = service();
        svc.open("b1", Some("http://localhost:5173/"), PLACE, true)
            .expect("open");
        let err = svc
            .navigate("b1", "javascript:alert(1)", DDG)
            .expect_err("blocked");
        assert_eq!(err.code, ErrorCode::Invalid);
        assert!(!host.calls().iter().any(|c| matches!(c, Call::Load { .. })));
        let last = published
            .lock()
            .expect("published")
            .last()
            .cloned()
            .expect("a state");
        assert_eq!(last.blocked.as_deref(), Some("javascript:alert(1)"));
        // The page itself can't go there either (a link, a redirect).
        assert!(!host.allows("tandem://x"));
        assert!(!host.allows("javascript:void(0)"));
        assert!(host.allows("https://example.com/"));
        // A later good address clears the message.
        svc.navigate("b1", "example.com", DDG).expect("ok");
        assert_eq!(
            published
                .lock()
                .expect("published")
                .last()
                .and_then(|s| s.blocked.clone()),
            None
        );
    }

    #[test]
    fn f_cd_17_opening_an_open_browser_moves_it_and_keeps_its_page() {
        let (svc, host, _) = service();
        svc.open("b1", Some("https://example.com/"), PLACE, true)
            .expect("open");
        let side = Rect {
            x: 1000.0,
            y: 40.0,
            width: 400.0,
            height: 700.0,
        };
        let state = svc
            .open("b1", Some("https://other.example/"), side, true)
            .expect("move");
        assert_eq!(state.url, "https://example.com/");
        let calls = host.calls();
        assert_eq!(
            calls
                .iter()
                .filter(|c| matches!(c, Call::Open { .. }))
                .count(),
            1
        );
        assert_eq!(
            calls.last(),
            Some(&Call::Place {
                id: "b1".into(),
                frame: side,
                zoom: 1.0,
                visible: true
            })
        );
    }

    #[test]
    fn f_cd_17_device_and_bounds_place_the_page_and_close_forgets_it() {
        let (svc, host, _) = service();
        svc.open("b1", None, PLACE, true).expect("open");
        let state = svc.set_device("b1", Device::Tablet, true).expect("device");
        assert!(state.rotated && state.zoom < 1.0);
        svc.set_bounds("b1", PLACE, false).expect("hide");
        assert!(matches!(
            host.calls().last(),
            Some(Call::Place { visible: false, .. })
        ));
        svc.close("b1");
        assert_eq!(host.calls().last(), Some(&Call::Close { id: "b1".into() }));
        assert_eq!(
            svc.set_bounds("b1", PLACE, true).expect_err("closed").code,
            ErrorCode::NotFound
        );
        assert_eq!(
            svc.command("b1", BrowserCmd::Back)
                .expect_err("closed")
                .code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn f_cd_17_a_page_moves_between_a_pane_and_the_sidebar_and_keeps_going() {
        let (svc, host, published) = service();
        svc.open("pn_1", Some("http://localhost:5173/"), PLACE, true)
            .expect("open");
        host.send(HostEvent::Loaded {
            id: "pn_1".into(),
            url: "http://localhost:5173/a".into(),
            can_back: true,
            can_forward: false,
        });
        let moved = svc.rename("pn_1", "browser-x").expect("move");
        assert_eq!(moved.browser_id, "browser-x");
        assert_eq!(moved.url, "http://localhost:5173/a");
        assert!(moved.can_back);
        assert_eq!(
            host.calls().last(),
            Some(&Call::Rename {
                from: "pn_1".into(),
                to: "browser-x".into()
            })
        );
        assert_eq!(
            published
                .lock()
                .expect("published")
                .last()
                .map(|s| s.browser_id.clone())
                .as_deref(),
            Some("browser-x")
        );
        // The page's reports follow it; the old id is gone.
        host.send(HostEvent::Title {
            id: "browser-x".into(),
            title: "A".into(),
        });
        assert_eq!(
            svc.state("browser-x").map(|s| s.title).as_deref(),
            Some("A")
        );
        assert!(svc.state("pn_1").is_none());
        assert_eq!(
            svc.rename("pn_1", "pn_2").expect_err("gone").code,
            ErrorCode::NotFound
        );
        // Opening the old id again makes a new page; a move onto it is refused.
        svc.open("pn_1", None, PLACE, true).expect("new page");
        assert_eq!(
            svc.rename("browser-x", "pn_1").expect_err("taken").code,
            ErrorCode::Conflict
        );
    }

    /// A page's report, as the picker script sends it.
    fn report(nonce: &str, selectors: &[&str], stopped: bool) -> String {
        let elements: Vec<serde_json::Value> = selectors
            .iter()
            .map(|sel| {
                serde_json::json!({
                    "tag": "li", "selector": sel, "component": {"name": "TodoItem", "framework": "React"},
                    "rect": {"x": 0, "y": 0, "width": 10, "height": 10},
                    "styles": {"display": "block", "position": "static", "font": "16px serif", "color": "rgb(0, 0, 0)",
                               "background": "rgba(0, 0, 0, 0)", "margin": "0px", "padding": "0px"},
                    "html": "<li>x</li>", "truncated": false
                })
            })
            .collect();
        let json = serde_json::json!({ "elements": elements, "stopped": stopped }).to_string();
        let encoded: String = url::form_urlencoded::byte_serialize(json.as_bytes())
            .collect::<String>()
            .replace('+', "%20");
        format!("tandem-picker://{nonce}/{encoded}")
    }

    fn nonce_in(script: &str) -> String {
        let start = script.find("const NONCE = \"").expect("a nonce") + "const NONCE = \"".len();
        script[start..start + 32].to_string()
    }

    #[test]
    fn f_cd_18_the_picker_runs_in_the_page_and_only_its_session_reports() {
        let (svc, host, published) = service();
        let picked: Arc<Mutex<Vec<picker::BrowserPicked>>> = Arc::default();
        let sink = picked.clone();
        svc.set_picker_emitter(Arc::new(move |p| {
            sink.lock().expect("picked").push(p.clone())
        }));
        let last_picked = || {
            picked
                .lock()
                .expect("picked")
                .last()
                .map(|p| p.elements.len())
        };
        svc.open("b1", Some("http://localhost:5173/react"), PLACE, true)
            .expect("open");
        assert!(svc.picker("b1", true).expect("on").picking);
        let Some(Call::Eval { script, .. }) = host.calls().last().cloned() else {
            panic!("the picker wasn't run in the page");
        };
        let nonce = nonce_in(&script);
        host.send(HostEvent::Picker {
            id: "b1".into(),
            url: report(&nonce, &["#a", "main > ul > li:nth-of-type(2)"], false),
        });
        assert_eq!(last_picked(), Some(2));
        // Another session's report, or the page's own, is no selection.
        host.send(HostEvent::Picker {
            id: "b1".into(),
            url: report("0123456789abcdef0123456789abcdef", &["#forged"], false),
        });
        assert_eq!(last_picked(), Some(2));
        let md = svc.brief("b1", "bigger").expect("brief");
        assert!(
            md.contains("## 1. `#a`")
                && md.contains("## 2. `main > ul > li:nth-of-type(2)`")
                && !md.contains("forged")
        );
        assert!(md.contains("Page: http://localhost:5173/react") && md.contains("Note: bigger"));
        // A new page: the old selection goes, and the picker runs again there.
        host.send(HostEvent::Committed {
            id: "b1".into(),
            url: "http://localhost:5173/other".into(),
        });
        assert_eq!(last_picked(), Some(0));
        host.send(HostEvent::Loaded {
            id: "b1".into(),
            url: "http://localhost:5173/other".into(),
            can_back: true,
            can_forward: false,
        });
        assert!(
            matches!(host.calls().last(), Some(Call::Eval { script, .. }) if nonce_in(script) == nonce)
        );
        // Esc in the page stops it.
        host.send(HostEvent::Picker {
            id: "b1".into(),
            url: report(&nonce, &[], true),
        });
        assert!(
            !published
                .lock()
                .expect("published")
                .last()
                .expect("state")
                .picking
        );
        // A new session gets a new nonce; turning it off stops that one.
        svc.picker("b1", true).expect("on again");
        let Some(Call::Eval { script, .. }) = host.calls().last().cloned() else {
            panic!("no picker");
        };
        let second = nonce_in(&script);
        assert_ne!(second, nonce);
        assert!(!svc.picker("b1", false).expect("off").picking);
        assert_eq!(
            host.calls().last(),
            Some(&Call::Eval {
                id: "b1".into(),
                script: picker::stop_script(&second)
            })
        );
        host.send(HostEvent::Picker {
            id: "b1".into(),
            url: report(&second, &["#late"], false),
        });
        assert_eq!(last_picked(), Some(0));
        // Where frames' navigations go unseen (WebView2), it reports by window.
        assert!(
            matches!(&host.calls()[..], [.., Call::Eval { script, .. }, _] if script.contains("const VIA = \"frame\";"))
        );
        host.no_frame_navigations
            .store(true, std::sync::atomic::Ordering::SeqCst);
        svc.picker("b1", true).expect("on by window");
        assert!(
            matches!(host.calls().last(), Some(Call::Eval { script, .. }) if script.contains("const VIA = \"window\";"))
        );
    }

    #[test]
    fn f_cd_17_an_address_the_policy_refuses_never_opens() {
        let (svc, host, _) = service();
        let err = svc
            .open("b1", Some("javascript:alert(1)"), PLACE, true)
            .expect_err("refused");
        assert_eq!(err.code, ErrorCode::Invalid);
        assert!(host.calls().is_empty());
    }
}
