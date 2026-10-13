//! The floating ring (F-CN-04, UI-61): an optional small window with
//! Conductor's ring that floats over everything, drags anywhere and keeps
//! its place; hovering shows Mute, Call log and End. It shows only while
//! "Floating ring" is on and the plan has Conductor.

use serde_json::json;
use tandem_core::settings::SettingKey;
use tandem_store::repo::settings;
use tandem_store::{Store, StoreError};
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "conductor-ring";
/// Logical size: the 56 px ring and its hover controls.
pub const WIDTH: f64 = 176.0;
pub const HEIGHT: f64 = 88.0;
const MARGIN: f64 = 24.0;

/// The first place: the top-right of a display's work area, inside a
/// margin. All values physical.
pub fn place(area_x: i32, area_y: i32, area_w: u32, scale: f64) -> (i32, i32) {
    let w = (WIDTH * scale).round() as i32;
    let m = (MARGIN * scale).round() as i32;
    (area_x + area_w as i32 - w - m, area_y + m)
}

/// A display's work area, physical pixels, and its scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub scale: f64,
}

/// Where the ring was dragged to (`conductor_ring_position`, "x,y").
pub async fn save_position(store: &Store, x: i32, y: i32) -> Result<(), StoreError> {
    settings::set(
        store,
        SettingKey::ConductorRingPosition,
        json!(format!("{x},{y}")),
    )
    .await
}

/// The saved place, if any.
pub async fn saved_position(store: &Store) -> Option<(i32, i32)> {
    settings::get(store, SettingKey::ConductorRingPosition)
        .await
        .ok()?
        .as_str()
        .and_then(crate::services::widget::parse_position)
}

/// Where the ring opens: where it was left, while that is still on a
/// display (its centre inside a work area); otherwise top-right of the
/// first display, as the first time. `areas[0]` is the primary display.
pub fn position(saved: Option<(i32, i32)>, areas: &[Area]) -> Option<(i32, i32)> {
    let on_screen = |(x, y): (i32, i32)| {
        areas.iter().any(|a| {
            let cx = x + (WIDTH * a.scale / 2.0) as i32;
            let cy = y + (HEIGHT * a.scale / 2.0) as i32;
            cx >= a.x && cx < a.x + a.w as i32 && cy >= a.y && cy < a.y + a.h as i32
        })
    };
    match saved {
        Some(at) if on_screen(at) => Some(at),
        _ => areas.first().map(|a| place(a.x, a.y, a.w, a.scale)),
    }
}

fn create(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    if let Some(w) = app.get_webview_window(LABEL) {
        return Ok(w);
    }
    WebviewWindowBuilder::new(
        app,
        LABEL,
        WebviewUrl::App("index.html?window=conductor-ring".into()),
    )
    .title("Conductor")
    .inner_size(WIDTH, HEIGHT)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .skip_taskbar(true)
    .focused(false)
    .visible(false)
    .build()
}

/// Shows the ring where it was left (or top-right the first time), or
/// hides it.
pub fn sync(app: &AppHandle, show: bool, saved: Option<(i32, i32)>) {
    if !show {
        if let Some(w) = app.get_webview_window(LABEL) {
            let _ = w.hide();
        }
        return;
    }
    let w = match create(app) {
        Ok(w) => w,
        Err(err) => {
            tracing::warn!(error = %err, "conductor: the floating ring window wasn't created");
            return;
        }
    };
    if w.is_visible().unwrap_or(false) {
        return;
    }
    let area = |m: &tauri::Monitor| Area {
        x: m.work_area().position.x,
        y: m.work_area().position.y,
        w: m.work_area().size.width,
        h: m.work_area().size.height,
        scale: m.scale_factor(),
    };
    let mut areas: Vec<Area> = app
        .primary_monitor()
        .ok()
        .flatten()
        .iter()
        .map(area)
        .collect();
    areas.extend(
        app.available_monitors()
            .unwrap_or_default()
            .iter()
            .map(area),
    );
    let at = position(saved, &areas);
    if let Some((x, y)) = at {
        let _ = w.set_position(PhysicalPosition::new(x, y));
    }
    let _ = w.show();
    let _ = w.set_always_on_top(true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_cn_04_the_ring_starts_top_right_inside_the_margin() {
        assert_eq!(place(0, 0, 1920, 1.0), (1920 - 176 - 24, 24));
        assert_eq!(place(1920, 0, 2560, 2.0), (1920 + 2560 - 352 - 48, 48));
    }

    const MAIN: Area = Area {
        x: 0,
        y: 0,
        w: 1920,
        h: 1080,
        scale: 1.0,
    };
    const LEFT: Area = Area {
        x: -2560,
        y: 0,
        w: 2560,
        h: 1440,
        scale: 2.0,
    };

    #[tokio::test]
    async fn f_cn_04_the_ring_reopens_where_it_was_left() {
        let dir = tempfile::tempdir().expect("dir");
        let store = Store::open_at(&dir.path().join("t.db"), &dir.path().join("b")).expect("store");
        // Never moved: the first place.
        assert_eq!(saved_position(&store).await, None);
        assert_eq!(
            position(saved_position(&store).await, &[MAIN]),
            Some((1720, 24))
        );
        // Dragged, then the window hidden and shown again (or the app
        // restarted): the same place, a display left of the main one too.
        save_position(&store, 300, 640).await.expect("save");
        assert_eq!(saved_position(&store).await, Some((300, 640)));
        assert_eq!(
            position(saved_position(&store).await, &[MAIN, LEFT]),
            Some((300, 640))
        );
        save_position(&store, -900, 200).await.expect("save");
        assert_eq!(saved_position(&store).await, Some((-900, 200)));
        assert_eq!(
            position(saved_position(&store).await, &[MAIN, LEFT]),
            Some((-900, 200))
        );
    }

    #[test]
    fn f_cn_04_a_ring_left_on_a_display_that_is_gone_comes_back() {
        // The left display was unplugged: back to the main display's corner.
        assert_eq!(position(Some((-900, 200)), &[MAIN]), Some((1720, 24)));
        // Half off the edge still counts while its centre is on screen.
        assert_eq!(position(Some((1800, 500)), &[MAIN]), Some((1800, 500)));
        assert_eq!(position(Some((1900, 500)), &[MAIN]), Some((1720, 24)));
        // No display known: leave it where the window system puts it.
        assert_eq!(position(None, &[]), None);
    }
}
