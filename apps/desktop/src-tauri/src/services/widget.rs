//! The Dictate widget window (F-DI-07, UI-60): a small transparent,
//! always-on-top, never-focused window that only the voice service shows
//! and hides. It sits bottom-centre on the display under the pointer, 48 px
//! above the Dock or taskbar, unless the user dragged and locked it.
//!
//! The window is made on first use, not at launch (D-519): its WebKit web
//! process costs about 120 MB whether or not anyone dictates. [`LazyWidget`]
//! creates it on the first show (the first recording), closes it after 10
//! minutes without one, and does all of that on its own thread, so neither
//! capture nor the controller ever waits for a window.

use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use tandem_core::clock::Clock;
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

use tandem_core::settings::SettingKey;
use tandem_store::Store;

use crate::services::dictate::WidgetHost;

pub const LABEL: &str = "dictate-widget";
/// Logical size: room for the 360 px error pill and its shadow.
/// Wide enough for an error with its actions ("Use local for this take",
/// F-DI-05); the window is transparent around the pill.
pub const WIDTH: f64 = 560.0;
pub const HEIGHT: f64 = 64.0;
const ABOVE_BOTTOM: f64 = 48.0;
/// A widget window unused this long is closed (D-519).
pub const IDLE_CLOSE: Duration = Duration::from_secs(10 * 60);
/// How often the idle check runs.
const TICK: Duration = Duration::from_secs(30);

/// Where the window goes on a display: centred, `ABOVE_BOTTOM` logical
/// pixels above the bottom of its work area. All values physical.
pub fn place(area_x: i32, area_y: i32, area_w: u32, area_h: u32, scale: f64) -> (i32, i32) {
    let w = (WIDTH * scale).round() as i32;
    let h = (HEIGHT * scale).round() as i32;
    let x = area_x + (area_w as i32 - w) / 2;
    let y = area_y + area_h as i32 - h - (ABOVE_BOTTOM * scale).round() as i32;
    (x, y)
}

/// A saved `x,y` position.
pub fn parse_position(raw: &str) -> Option<(i32, i32)> {
    let (x, y) = raw.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// Creates the hidden widget window.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window(LABEL).is_some() {
        return Ok(());
    }
    WebviewWindowBuilder::new(
        app,
        LABEL,
        WebviewUrl::App("index.html?window=dictate-widget".into()),
    )
    .title("Dictate")
    .inner_size(WIDTH, HEIGHT)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .skip_taskbar(true)
    .focused(false)
    .focusable(false)
    .visible(false)
    .build()?;
    Ok(())
}

pub struct TauriWidget {
    app: AppHandle,
    store: Store,
}

impl TauriWidget {
    pub fn new(app: AppHandle, store: Store) -> Self {
        Self { app, store }
    }

    fn setting(&self, key: SettingKey) -> serde_json::Value {
        tandem_store::repo::settings::get_blocking(&self.store, key).unwrap_or_default()
    }

    fn position(&self, w: &tauri::WebviewWindow) {
        let locked = self
            .setting(SettingKey::DictateLockPosition)
            .as_bool()
            .unwrap_or(false);
        let saved = self
            .setting(SettingKey::DictateWidgetPosition)
            .as_str()
            .and_then(parse_position);
        if let (true, Some((x, y))) = (locked, saved) {
            let _ = w.set_position(PhysicalPosition::new(x, y));
            return;
        }
        let follow = self
            .setting(SettingKey::DictateFollowCursor)
            .as_bool()
            .unwrap_or(true);
        let monitor = if follow {
            self.app
                .cursor_position()
                .ok()
                .and_then(|p| self.app.monitor_from_point(p.x, p.y).ok().flatten())
        } else {
            None
        };
        let monitor = monitor.or_else(|| self.app.primary_monitor().ok().flatten());
        let Some(m) = monitor else { return };
        let area = m.work_area();
        let (x, y) = place(
            area.position.x,
            area.position.y,
            area.size.width,
            area.size.height,
            m.scale_factor(),
        );
        let _ = w.set_position(PhysicalPosition::new(x, y));
    }
}

/// What [`LazyWidget`] does to the real window; faked in tests.
pub trait WidgetWindow: Send + Sync {
    fn exists(&self) -> bool;
    fn create(&self) -> Result<(), String>;
    fn close(&self);
    fn show(&self);
    fn hide(&self);
}

impl WidgetWindow for TauriWidget {
    fn exists(&self) -> bool {
        self.app.get_webview_window(LABEL).is_some()
    }

    fn create(&self) -> Result<(), String> {
        create(&self.app).map_err(|e| e.to_string())
    }

    fn close(&self) {
        if let Some(w) = self.app.get_webview_window(LABEL) {
            let _ = w.destroy();
        }
    }

    fn show(&self) {
        let Some(w) = self.app.get_webview_window(LABEL) else {
            return;
        };
        if w.is_visible().unwrap_or(false) {
            return;
        }
        self.position(&w);
        let _ = w.show();
        let _ = w.set_always_on_top(true);
    }

    fn hide(&self) {
        if let Some(w) = self.app.get_webview_window(LABEL) {
            let _ = w.hide();
        }
    }
}

enum Op {
    Show,
    Hide,
    Tick,
    Flush(mpsc::Sender<()>),
}

/// The widget window made on first use and closed when idle (D-519). Every
/// call only queues an operation: one thread creates, shows, hides and
/// closes in order, so a show that races an idle close simply recreates.
pub struct LazyWidget {
    tx: mpsc::Sender<Op>,
}

impl LazyWidget {
    pub fn new(window: Arc<dyn WidgetWindow>, clock: Arc<dyn Clock>, idle: Duration) -> Self {
        let (tx, rx) = mpsc::channel::<Op>();
        let _ = std::thread::Builder::new()
            .name("dictate-widget".into())
            .spawn(move || {
                let idle_ms = idle.as_millis() as i64;
                // When the window last showed, or was asked to.
                let mut used: i64 = clock.now_ms();
                let mut shown = false;
                while let Ok(op) = rx.recv() {
                    match op {
                        Op::Show => {
                            used = clock.now_ms();
                            if !window.exists() {
                                if let Err(err) = window.create() {
                                    tracing::warn!(error = %err, "dictate: widget window not created");
                                    continue;
                                }
                            }
                            window.show();
                            shown = true;
                        }
                        Op::Hide => {
                            if shown {
                                used = clock.now_ms();
                            }
                            shown = false;
                            if window.exists() {
                                window.hide();
                            }
                        }
                        Op::Tick => {
                            if !shown && window.exists() && clock.now_ms() - used >= idle_ms {
                                window.close();
                                tracing::info!(code = "dictate.widget_closed", "closed the idle widget window");
                            }
                        }
                        Op::Flush(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            });
        Self { tx }
    }

    /// Runs the idle check every 30 s for as long as the app does.
    pub fn tick_every_30s(self: &Arc<Self>) {
        let me = Arc::downgrade(self);
        let _ = std::thread::Builder::new()
            .name("dictate-widget-idle".into())
            .spawn(move || loop {
                std::thread::sleep(TICK);
                match me.upgrade() {
                    Some(w) => w.tick(),
                    None => return,
                }
            });
    }

    pub fn tick(&self) {
        let _ = self.tx.send(Op::Tick);
    }

    /// Waits until everything queued so far has run (tests).
    pub fn flush(&self) {
        let (done, rx) = mpsc::channel();
        if self.tx.send(Op::Flush(done)).is_ok() {
            let _ = rx.recv_timeout(Duration::from_secs(10));
        }
    }
}

impl WidgetHost for LazyWidget {
    fn show(&self) {
        let _ = self.tx.send(Op::Show);
    }

    fn hide(&self) {
        let _ = self.tx.send(Op::Hide);
    }
}

#[cfg(test)]
mod tests {
    use parking_lot::Mutex;
    use std::time::Instant;
    use tandem_core::clock::MockClock;

    #[derive(Default)]
    struct FakeWindow {
        open: Mutex<bool>,
        ops: Mutex<Vec<&'static str>>,
        create_delay: Mutex<Duration>,
    }

    impl WidgetWindow for FakeWindow {
        fn exists(&self) -> bool {
            *self.open.lock()
        }
        fn create(&self) -> Result<(), String> {
            std::thread::sleep(*self.create_delay.lock());
            *self.open.lock() = true;
            self.ops.lock().push("create");
            Ok(())
        }
        fn close(&self) {
            *self.open.lock() = false;
            self.ops.lock().push("close");
        }
        fn show(&self) {
            assert!(*self.open.lock(), "shown only once it exists");
            self.ops.lock().push("show");
        }
        fn hide(&self) {
            self.ops.lock().push("hide");
        }
    }

    fn rig() -> (Arc<FakeWindow>, MockClock, LazyWidget) {
        let win = Arc::new(FakeWindow::default());
        let clock = MockClock::new(chrono::Utc::now());
        let w = LazyWidget::new(win.clone(), Arc::new(clock.clone()), IDLE_CLOSE);
        (win, clock, w)
    }

    #[test]
    fn f_di_07_no_widget_window_exists_after_launch() {
        let (win, clock, w) = rig();
        // Launch, and an idle hide from the visibility setting.
        w.hide();
        w.flush();
        clock.advance(Duration::from_secs(30 * 60));
        w.tick();
        w.flush();
        assert!(!win.exists());
        assert!(!win.ops.lock().contains(&"create"));
    }

    #[test]
    fn f_di_07_the_first_recording_creates_the_window_and_shows_the_pill() {
        let (win, _clock, w) = rig();
        w.show();
        w.flush();
        assert_eq!(*win.ops.lock(), vec!["create", "show"]);
        w.hide();
        w.show();
        w.flush();
        assert_eq!(
            *win.ops.lock(),
            vec!["create", "show", "hide", "show"],
            "made once"
        );
    }

    #[test]
    fn f_di_07_a_recording_never_waits_for_the_window() {
        let (win, _clock, w) = rig();
        *win.create_delay.lock() = Duration::from_millis(800);
        let t = Instant::now();
        w.show();
        assert!(t.elapsed() < Duration::from_millis(50), "{:?}", t.elapsed());
        w.flush();
        assert_eq!(
            *win.ops.lock(),
            vec!["create", "show"],
            "the pill appears once it's ready"
        );
    }

    #[test]
    fn f_di_07_ten_idle_minutes_close_the_window_and_the_next_recording_recreates_it() {
        let (win, clock, w) = rig();
        w.show();
        w.hide();
        w.flush();
        clock.advance(Duration::from_secs(9 * 60));
        w.tick();
        w.flush();
        assert!(win.exists(), "still open before 10 minutes");
        clock.advance(Duration::from_secs(60));
        w.tick();
        w.flush();
        assert!(!win.exists(), "closed after 10 idle minutes");
        w.show();
        w.flush();
        assert!(win.exists());
        assert_eq!(
            *win.ops.lock(),
            vec!["create", "show", "hide", "close", "create", "show"]
        );
    }

    #[test]
    fn f_di_07_a_shown_window_is_never_closed_and_a_show_racing_a_close_recreates() {
        let (win, clock, w) = rig();
        // A long hands-free take: shown, so the idle check leaves it alone.
        w.show();
        w.flush();
        clock.advance(Duration::from_secs(15 * 60));
        w.tick();
        w.flush();
        assert!(win.exists());
        // The take ends; ten minutes later the check and a new take arrive together.
        w.hide();
        w.flush();
        clock.advance(Duration::from_secs(10 * 60));
        w.tick();
        w.show();
        w.flush();
        assert!(win.exists(), "the take that raced the close has its window");
        assert_eq!(win.ops.lock().last(), Some(&"show"));
        // Many threads at once: ops stay ordered, the window ends shown.
        let w = Arc::new(w);
        let hs: Vec<_> = (0..8)
            .map(|i| {
                let w = w.clone();
                std::thread::spawn(move || {
                    if i % 2 == 0 {
                        w.tick();
                    } else {
                        w.show();
                    }
                })
            })
            .collect();
        for h in hs {
            let _ = h.join();
        }
        w.flush();
        assert!(win.exists());
    }

    use super::*;

    #[test]
    fn f_di_07_bottom_centre_48px_above_the_work_area() {
        // 1920×1040 work area (taskbar below), scale 1.
        assert_eq!(place(0, 0, 1920, 1040, 1.0), (680, 928));
        // Second display to the right at 2×.
        let (x, y) = place(1920, 0, 2560, 1600, 2.0);
        assert_eq!(x, 1920 + (2560 - 1120) / 2);
        assert_eq!(y, 1600 - 128 - 96);
    }

    #[test]
    fn positions_parse() {
        assert_eq!(parse_position("12, -40"), Some((12, -40)));
        assert_eq!(parse_position("x"), None);
    }
}
