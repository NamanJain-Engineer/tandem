//! The floating ring (F-CN-04, UI-61): an optional small window with
//! Conductor's ring that floats over everything, drags anywhere and keeps
//! its place; hovering shows Mute, Call log and End. It shows only while
//! "Floating ring" is on and the plan has Conductor.

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
    let at = saved.or_else(|| {
        let m = app.primary_monitor().ok().flatten()?;
        let area = m.work_area();
        Some(place(
            area.position.x,
            area.position.y,
            area.size.width,
            m.scale_factor(),
        ))
    });
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
}
