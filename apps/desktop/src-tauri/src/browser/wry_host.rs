//! The browser's webviews (F-CD-17, D-206): wry webviews on the main
//! thread, sharing one profile at `$DATA/browser-profile`.
//!
//! - Linux: built into a `gtk::Fixed` layered over the main webview, which
//!   [`install_overlay`] sets up before the window first shows (a WebKitGTK
//!   view moved after it has painted stays blank).
//! - macOS and Windows: wry child webviews of the main window.
//!
//! Every call hops to the main thread. Webview callbacks never touch the
//! page list directly (a callback can fire inside a call that holds it):
//! they report through the sink, and history queries wait for idle.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use tandem_core::error::{ErrorCode, IpcError};
use tauri::AppHandle;
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::{NewWindowResponse, PageLoadEvent, WebContext, WebView, WebViewBuilder};

use super::{BrowserCmd, BrowserHost, EventSink, HostEvent, Policy, Rect, BLANK};

pub const UNAVAILABLE: &str = "Web pages can't be shown inside the app on this system. Open the address in your browser instead.";

/// Linux: the layer exists (or, elsewhere, nothing needs one).
static READY: AtomicBool = AtomicBool::new(!cfg!(target_os = "linux"));

struct Page {
    webview: WebView,
    /// Where it goes (window coordinates).
    frame: Rect,
}

#[derive(Default)]
struct Pages {
    context: Option<WebContext>,
    pages: HashMap<String, Page>,
    #[cfg(target_os = "linux")]
    layer: Option<(gtk::Overlay, gtk::Fixed)>,
}

thread_local! {
    static PAGES: RefCell<Pages> = RefCell::new(Pages::default());
}

fn bounds(frame: Rect) -> wry::Rect {
    wry::Rect {
        position: LogicalPosition::new(frame.x, frame.y).into(),
        size: LogicalSize::new(frame.width.max(1.0), frame.height.max(1.0)).into(),
    }
}

/// Moves page `id` to `frame`.
fn put(p: &mut Pages, id: &str, frame: Rect) {
    let Some(page) = p.pages.get_mut(id) else {
        return;
    };
    page.frame = frame;
    #[cfg(target_os = "linux")]
    if let Some((overlay, fixed)) = &p.layer {
        use gtk::prelude::*;
        use wry::WebViewExtUnix;
        // The Fixed's own place and size for the page, so its layout agrees
        // with `exact_places` once WebKit has taken the new size.
        let inner = page.webview.webview();
        let (x, y, w, h) = gtk_rect(frame);
        fixed.move_(&inner, x, y);
        inner.set_size_request(w, h);
        fixed.queue_resize();
        // The area a page left must be painted again.
        overlay.queue_draw();
        return;
    }
    let _ = page.webview.set_bounds(bounds(frame));
}

#[cfg(target_os = "linux")]
fn gtk_rect(frame: Rect) -> (i32, i32, i32, i32) {
    (
        frame.x.round() as i32,
        frame.y.round() as i32,
        frame.width.round().max(1.0) as i32,
        frame.height.round().max(1.0) as i32,
    )
}

/// Linux: gives each shown page exactly its frame, after the Fixed has laid
/// them out. A Fixed never makes a child smaller than its minimum, and
/// WebKit's minimum is the size it has, so a page could grow but never
/// shrink (wry's `set_bounds` only lasts until the next layout).
#[cfg(target_os = "linux")]
fn exact_places(fixed: &gtk::Fixed) {
    use gtk::prelude::*;
    use wry::WebViewExtUnix;
    let origin = fixed.allocation();
    PAGES.with(|cell| {
        // A call that's changing the pages queues another layout.
        let Ok(p) = cell.try_borrow() else { return };
        for page in p.pages.values() {
            let inner = page.webview.webview();
            if !inner.is_visible() {
                continue;
            }
            let (x, y, w, h) = gtk_rect(page.frame);
            inner.size_allocate(&gtk::Allocation::new(origin.x() + x, origin.y() + y, w, h));
        }
    });
}

/// Linux: puts the main webview under a `gtk::Overlay` whose top layer is a
/// pass-through `gtk::Fixed` the browser's webviews go into. Must run before
/// the window first shows.
#[cfg(target_os = "linux")]
pub fn install_overlay(window: &tauri::WebviewWindow) {
    let done = window.with_webview(|platform| {
        use gtk::prelude::*;
        let main = platform.inner();
        let Some(parent) = main.parent().and_then(|p| p.downcast::<gtk::Box>().ok()) else {
            tracing::warn!(
                code = "browser.no_overlay",
                "the main webview isn't in a box"
            );
            return;
        };
        let overlay = gtk::Overlay::new();
        parent.remove(&main);
        overlay.add(&main);
        let fixed = gtk::Fixed::new();
        fixed.connect_size_allocate(|fixed, _| exact_places(fixed));
        overlay.add_overlay(&fixed);
        overlay.set_overlay_pass_through(&fixed, true);
        parent.pack_start(&overlay, true, true, 0);
        overlay.show_all();
        PAGES.with(|p| p.borrow_mut().layer = Some((overlay, fixed)));
        READY.store(true, Ordering::SeqCst);
    });
    if let Err(err) = done {
        tracing::warn!(code = "browser.no_overlay", error = %err, "couldn't reach the main webview");
    }
}

pub struct WryHost {
    app: AppHandle,
    profile: PathBuf,
    policy: Policy,
    sink: EventSink,
}

impl WryHost {
    pub fn new(app: AppHandle, profile: PathBuf, policy: Policy, sink: EventSink) -> Self {
        WryHost {
            app,
            profile,
            policy,
            sink,
        }
    }

    fn on_main(&self, f: impl FnOnce() + Send + 'static) {
        if let Err(err) = self.app.run_on_main_thread(f) {
            tracing::warn!(code = "browser.main_thread", error = %err, "browser call dropped");
        }
    }
}

/// Reports a finished load with the page's history, once nothing holds the
/// page list.
fn report_loaded(id: String, url: String, sink: EventSink) {
    let send = move || {
        let (can_back, can_forward) = PAGES.with(|p| {
            p.borrow()
                .pages
                .get(&id)
                .map(|page| history(&page.webview))
                .unwrap_or((false, false))
        });
        sink(HostEvent::Loaded {
            id,
            url,
            can_back,
            can_forward,
        });
    };
    #[cfg(target_os = "linux")]
    gtk::glib::idle_add_local_once(send);
    #[cfg(not(target_os = "linux"))]
    send();
}

#[cfg(target_os = "linux")]
fn history(webview: &WebView) -> (bool, bool) {
    use webkit2gtk::WebViewExt;
    use wry::WebViewExtUnix;
    let inner = webview.webview();
    (inner.can_go_back(), inner.can_go_forward())
}

#[cfg(not(target_os = "linux"))]
fn history(_webview: &WebView) -> (bool, bool) {
    // Not exposed here: both buttons stay enabled (manual QA, HUMAN_TODO).
    (true, true)
}

/// Linux: reports a load's commit (the address bar shows a page as soon as
/// it replaces the old one, not when it finishes) and moves within a page
/// (`pushState`, a `#fragment`), which load nothing. Elsewhere the finished
/// load reports the address.
#[cfg(target_os = "linux")]
fn watch_commits(webview: &WebView, id: &str, sink: EventSink) {
    use webkit2gtk::{LoadEvent, WebViewExt};
    use wry::WebViewExtUnix;
    let inner = webview.webview();
    let (commit_id, commit_sink) = (id.to_string(), sink.clone());
    inner.connect_load_changed(move |view, event| {
        if event == LoadEvent::Committed {
            if let Some(url) = view.uri() {
                commit_sink(HostEvent::Committed {
                    id: commit_id.clone(),
                    url: url.to_string(),
                });
            }
        }
    });
    let id = id.to_string();
    // While a load runs the URI is the requested one, not what's shown;
    // that load reports its own commit.
    inner.connect_uri_notify(move |view| {
        if view.is_loading() {
            return;
        }
        if let Some(url) = view.uri() {
            sink(HostEvent::SameDocument {
                id: id.clone(),
                url: url.to_string(),
            });
        }
    });
}

/// Loads `url` in page `id` once nothing holds the page list.
fn load_later(id: String, url: String) {
    let load = move || {
        PAGES.with(|p| {
            if let Some(page) = p.borrow().pages.get(&id) {
                let _ = page.webview.load_url(&url);
            }
        });
    };
    #[cfg(target_os = "linux")]
    gtk::glib::idle_add_local_once(load);
    #[cfg(not(target_os = "linux"))]
    load();
}

fn builder<'a>(
    context: &'a mut WebContext,
    id: &str,
    url: &str,
    frame: Rect,
    visible: bool,
    policy: Policy,
    sink: EventSink,
) -> WebViewBuilder<'a> {
    let (nav_id, nav_sink, nav_policy) = (id.to_string(), sink.clone(), policy.clone());
    let (load_id, load_sink) = (id.to_string(), sink.clone());
    let (title_id, title_sink) = (id.to_string(), sink.clone());
    let (win_id, win_sink) = (id.to_string(), sink);
    WebViewBuilder::new_with_web_context(context)
        .with_url(url)
        .with_bounds(bounds(frame))
        .with_visible(visible)
        // A page opening (a restored tab shown) never takes the keyboard.
        .with_focused(false)
        .with_devtools(false)
        .with_navigation_handler(move |to: String| {
            let ok = nav_policy(&to);
            if !ok {
                nav_sink(HostEvent::Blocked {
                    id: nav_id.clone(),
                    url: to,
                });
            }
            ok
        })
        .with_on_page_load_handler(move |event, at| match event {
            PageLoadEvent::Started => load_sink(HostEvent::Loading {
                id: load_id.clone(),
            }),
            PageLoadEvent::Finished => report_loaded(load_id.clone(), at, load_sink.clone()),
        })
        .with_document_title_changed_handler(move |title| {
            title_sink(HostEvent::Title {
                id: title_id.clone(),
                title,
            })
        })
        // A link to a new window opens in the same tab, if it may.
        .with_new_window_req_handler(move |to, _features| {
            if policy(&to) {
                load_later(win_id.clone(), to);
            } else {
                win_sink(HostEvent::Blocked {
                    id: win_id.clone(),
                    url: to,
                });
            }
            NewWindowResponse::Deny
        })
        .with_download_started_handler(|_url, _path| false)
}

impl BrowserHost for WryHost {
    fn open(
        &self,
        id: &str,
        url: &str,
        frame: Rect,
        zoom: f64,
        visible: bool,
    ) -> Result<(), IpcError> {
        if !READY.load(Ordering::SeqCst) {
            return Err(IpcError::new(ErrorCode::Unavailable, UNAVAILABLE).with_stage("browser"));
        }
        let (id, url) = (id.to_string(), url.to_string());
        let (policy, sink, profile) =
            (self.policy.clone(), self.sink.clone(), self.profile.clone());
        #[cfg(not(target_os = "linux"))]
        let app = self.app.clone();
        self.on_main(move || {
            PAGES.with(|cell| {
                let mut p = cell.borrow_mut();
                if p.pages.contains_key(&id) {
                    put(&mut p, &id, frame);
                    return;
                }
                let Pages {
                    context,
                    pages,
                    #[cfg(target_os = "linux")]
                    layer,
                } = &mut *p;
                let context = context.get_or_insert_with(|| WebContext::new(Some(profile)));
                let built = {
                    let b = builder(context, &id, &url, frame, visible, policy, sink.clone());
                    #[cfg(target_os = "linux")]
                    {
                        use wry::WebViewBuilderExtUnix;
                        match layer {
                            Some((_, fixed)) => b.build_gtk(fixed),
                            None => return,
                        }
                    }
                    #[cfg(not(target_os = "linux"))]
                    {
                        match tauri::Manager::get_webview_window(&app, "main") {
                            Some(window) => b.build_as_child(&window),
                            None => return,
                        }
                    }
                };
                match built {
                    Ok(webview) => {
                        let _ = webview.zoom(zoom);
                        #[cfg(target_os = "linux")]
                        watch_commits(&webview, &id, sink);
                        pages.insert(id, Page { webview, frame });
                        #[cfg(target_os = "linux")]
                        if let Some((_, fixed)) = layer {
                            use gtk::prelude::*;
                            fixed.queue_resize();
                        }
                    }
                    Err(err) => {
                        tracing::warn!(code = "browser.create_failed", error = %err, "browser webview not created");
                    }
                }
            });
        });
        Ok(())
    }

    fn load(&self, id: &str, url: &str) {
        let (id, url) = (id.to_string(), url.to_string());
        self.on_main(move || load_later(id, url));
    }

    fn command(&self, id: &str, cmd: BrowserCmd) {
        let id = id.to_string();
        self.on_main(move || {
            PAGES.with(|p| {
                let p = p.borrow();
                let Some(page) = p.pages.get(&id) else { return };
                #[cfg(target_os = "linux")]
                {
                    use webkit2gtk::WebViewExt;
                    use wry::WebViewExtUnix;
                    let inner = page.webview.webview();
                    match cmd {
                        BrowserCmd::Back => inner.go_back(),
                        BrowserCmd::Forward => inner.go_forward(),
                        BrowserCmd::Reload => inner.reload(),
                        BrowserCmd::Stop => inner.stop_loading(),
                    }
                }
                #[cfg(not(target_os = "linux"))]
                {
                    let _ = match cmd {
                        BrowserCmd::Back => page.webview.evaluate_script("history.back()"),
                        BrowserCmd::Forward => page.webview.evaluate_script("history.forward()"),
                        BrowserCmd::Reload => page.webview.reload(),
                        BrowserCmd::Stop => page.webview.evaluate_script("window.stop()"),
                    };
                }
            });
        });
    }

    fn place(&self, id: &str, frame: Rect, zoom: f64, visible: bool) {
        let id = id.to_string();
        self.on_main(move || {
            PAGES.with(|p| {
                let mut p = p.borrow_mut();
                put(&mut p, &id, frame);
                let Some(page) = p.pages.get(&id) else { return };
                let _ = page.webview.zoom(zoom);
                let _ = page.webview.set_visible(visible);
            });
        });
    }

    fn close(&self, id: &str) {
        let id = id.to_string();
        self.on_main(move || {
            let page = PAGES.with(|p| p.borrow_mut().pages.remove(&id));
            drop(page);
            #[cfg(target_os = "linux")]
            PAGES.with(|p| {
                if let Some((overlay, _)) = &p.borrow().layer {
                    use gtk::prelude::*;
                    overlay.queue_draw();
                }
            });
        });
    }

    fn clear_data(&self) {
        let profile = self.profile.clone();
        #[cfg(not(target_os = "linux"))]
        let app = self.app.clone();
        self.on_main(move || {
            PAGES.with(|cell| {
                let mut p = cell.borrow_mut();
                if let Some(page) = p.pages.values().next() {
                    let _ = page.webview.clear_all_browsing_data();
                    return;
                }
                let Pages {
                    context,
                    #[cfg(target_os = "linux")]
                    layer,
                    ..
                } = &mut *p;
                let Some(context) = context.as_mut() else {
                    // Never opened this session: nothing holds the profile.
                    if let Err(err) = std::fs::remove_dir_all(&profile) {
                        if err.kind() != std::io::ErrorKind::NotFound {
                            tracing::warn!(code = "browser.clear_failed", error = %err, "browser data not cleared");
                        }
                    }
                    return;
                };
                // A hidden page on the open profile clears it.
                let b = WebViewBuilder::new_with_web_context(context)
                    .with_url(BLANK)
                    .with_visible(false);
                #[cfg(target_os = "linux")]
                let built = {
                    use wry::WebViewBuilderExtUnix;
                    match layer {
                        Some((_, fixed)) => b.build_gtk(fixed),
                        None => return,
                    }
                };
                #[cfg(not(target_os = "linux"))]
                let built = match tauri::Manager::get_webview_window(&app, "main") {
                    Some(window) => b.build_as_child(&window),
                    None => return,
                };
                if let Ok(webview) = built {
                    let _ = webview.clear_all_browsing_data();
                }
            });
        });
    }
}
