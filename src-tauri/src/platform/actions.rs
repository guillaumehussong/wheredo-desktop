//! Confirmed synthetic clicks (port of Actions.swift confirmAndClick/click).
//! Wheredo never clicks silently: every click goes through a native
//! confirmation dialog first.

use enigo::{Button, Coordinate, Direction, Enigo, Mouse, Settings};

use crate::core::feedback;

/// Ask the user to approve a click at (x, y). Blocking — call via spawn_blocking.
pub fn confirm_click_blocking(x: i32, y: i32, label: &str) -> bool {
    let text = format!("Action: {label} at ({x}, {y})\n\nAllow click?");
    rfd::MessageDialog::new()
        .set_title("Wheredo wants to click")
        .set_description(text)
        .set_buttons(rfd::MessageButtons::OkCancelCustom("Click".into(), "Cancel".into()))
        .set_level(rfd::MessageLevel::Warning)
        .show()
        == rfd::MessageDialogResult::Custom("Click".into())
}

/// Perform a real mouse click. Only ever called after user approval.
pub fn click(x: i32, y: i32) {
    let Ok(mut enigo) = Enigo::new(&Settings::default()) else {
        feedback::error("Click", "input backend unavailable (Wayland may block synthetic input)");
        return;
    };
    if enigo.move_mouse(x, y, Coordinate::Abs).is_err() {
        feedback::error("Click", "could not move mouse cursor");
        return;
    }
    match enigo.button(Button::Left, Direction::Click) {
        Ok(()) => feedback::log(&format!("✓ Click at ({x}, {y})")),
        Err(e) => feedback::error("Click", &e.to_string()),
    }
}
