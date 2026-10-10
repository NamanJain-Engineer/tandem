//! The Dictate widget window (F-DI-07, UI-60): a small transparent,
//! always-on-top, never-focused window that only the voice service shows
//! and hides. It sits bottom-centre on the display under the pointer, 48 px
//! above the Dock or taskbar, unless the user dragged and locked it.

use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

use tandem_core::settings::SettingKey;
use tandem_store::Store;

use crate::services::dictate::WidgetHost;

pub const LABEL: &str = "dictate-widget";
/// Logical size: room for the 360 px error pill and its shadow.
pub const WIDTH: f64 = 380.0;
pub const HEIGHT: f64 = 64.0;
const ABOVE_BOTTOM: f64 = 48.0;

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

impl WidgetHost for TauriWidget {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_di_07_bottom_centre_48px_above_the_work_area() {
        // 1920×1040 work area (taskbar below), scale 1.
        assert_eq!(place(0, 0, 1920, 1040, 1.0), (770, 928));
        // Second display to the right at 2×.
        let (x, y) = place(1920, 0, 2560, 1600, 2.0);
        assert_eq!(x, 1920 + (2560 - 760) / 2);
        assert_eq!(y, 1600 - 128 - 96);
    }

    #[test]
    fn positions_parse() {
        assert_eq!(parse_position("12, -40"), Some((12, -40)));
        assert_eq!(parse_position("x"), None);
    }
}
