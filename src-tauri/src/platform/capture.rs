//! Screen capture abstraction. Windows captures the foreground window;
//! Linux captures the focused window on X11 or the primary monitor otherwise.
//! Port of macOS `ScreenCapture.swift` + `CaptureContext.swift`.

use base64::Engine;
use image::RgbaImage;

use crate::core::{config, feedback};

/// Where the captured pixels sit in GLOBAL screen coordinates (top-left origin
/// on both Windows and X11 — no y-flip needed, unlike macOS).
#[derive(Debug, Clone, Copy)]
pub struct CaptureContext {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub source: CaptureSource,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CaptureSource {
    // Only constructed in the cfg-gated platform modules.
    #[allow(dead_code)]
    Window,
    Display,
}

impl CaptureContext {
    /// Map a vision point (normalized 0–1000 on the screenshot) to global
    /// screen pixels.
    pub fn point_in_screen_space(&self, nx: f64, ny: f64) -> (i32, i32) {
        let x = self.x as f64 + (nx / 1000.0) * self.width as f64;
        let y = self.y as f64 + (ny / 1000.0) * self.height as f64;
        (x.round() as i32, y.round() as i32)
    }
}

pub struct CaptureResult {
    pub image: RgbaImage,
    pub context: CaptureContext,
    pub app_name: String,
}

#[derive(Debug)]
pub enum CaptureError {
    NoContent(String),
    EncodeFailed(String),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureError::NoContent(e) => write!(f, "Screen capture failed: {e}"),
            CaptureError::EncodeFailed(e) => write!(f, "Image encoding failed: {e}"),
        }
    }
}

impl std::error::Error for CaptureError {}

/// Capture the frontmost window (preferred) or the primary monitor.
/// Blocking — call via spawn_blocking.
pub fn capture_frontmost() -> Result<CaptureResult, CaptureError> {
    if let Some(result) = capture_foreground_window() {
        return Ok(result);
    }
    capture_primary_monitor()
}

/// Platform hook: capture the currently focused window, or None to fall back.
#[cfg(target_os = "windows")]
fn capture_foreground_window() -> Option<CaptureResult> {
    super::capture_windows::capture_foreground_window()
}

#[cfg(target_os = "linux")]
fn capture_foreground_window() -> Option<CaptureResult> {
    super::capture_linux::capture_focused_window()
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn capture_foreground_window() -> Option<CaptureResult> {
    // macOS dev builds only (the real macOS product is the Swift app).
    // xcap's macOS backend is unreliable on recent macOS, so shell out to
    // Apple's own `screencapture` tool instead.
    let path = std::env::temp_dir().join("wheredo-capture.png");
    let ok = std::process::Command::new("screencapture")
        .args(["-x", "-t", "png"])
        .arg(&path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return None;
    }
    let image = image::open(&path).ok()?.to_rgba8();
    let _ = std::fs::remove_file(&path);
    let context = CaptureContext {
        x: 0,
        y: 0,
        width: image.width(),
        height: image.height(),
        source: CaptureSource::Display,
    };
    Some(CaptureResult { image, context, app_name: "unknown".into() })
}

pub fn capture_primary_monitor() -> Result<CaptureResult, CaptureError> {
    let monitors = xcap::Monitor::all().map_err(|e| CaptureError::NoContent(e.to_string()))?;
    // Prefer the primary monitor; some backends never flag one, so fall back
    // to the first monitor rather than failing.
    let mut monitors = monitors.into_iter().peekable();
    let first = monitors.peek().cloned();
    let monitor = monitors
        .find(|m| m.is_primary().unwrap_or(false))
        .or(first)
        .ok_or_else(|| CaptureError::NoContent("no monitors found".into()))?;

    let image = monitor
        .capture_image()
        .map_err(|e| CaptureError::NoContent(portal_hint(&e.to_string())))?;
    let context = CaptureContext {
        x: monitor.x().unwrap_or(0),
        y: monitor.y().unwrap_or(0),
        width: image.width(),
        height: image.height(),
        source: CaptureSource::Display,
    };
    Ok(CaptureResult { image, context, app_name: "unknown".into() })
}

/// On Linux Wayland, xcap needs xdg-desktop-portal + pipewire; enrich the
/// error so the user knows what to install/allow.
fn portal_hint(err: &str) -> String {
    if cfg!(target_os = "linux") && std::env::var("WAYLAND_DISPLAY").is_ok() {
        format!(
            "{err}\nWayland detected: screen capture needs xdg-desktop-portal and pipewire.\n\
             Install them (e.g. `sudo apt install xdg-desktop-portal pipewire`) and grant\n\
             the screen-share permission when the system dialog appears."
        )
    } else {
        err.to_string()
    }
}

/// Encode as base64 JPEG for the vision API. Low quality keeps the payload
/// small while UI text stays legible to the model (same as macOS).
pub fn jpeg_base64(image: &RgbaImage) -> Result<String, CaptureError> {
    let quality = (config::vision_jpeg_quality() * 100.0).clamp(10.0, 100.0) as u8;
    let rgb = image::DynamicImage::ImageRgba8(image.clone()).to_rgb8();

    let mut jpeg = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality);
    encoder
        .encode_image(&rgb)
        .map_err(|e| CaptureError::EncodeFailed(e.to_string()))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(jpeg))
}

/// Human-readable context injected into the vision system prompt.
pub fn frontmost_context(result: &CaptureResult) -> String {
    format!(
        "Active app: {}. Captured region: {}x{} px at ({}, {}).",
        result.app_name, result.context.width, result.context.height,
        result.context.x, result.context.y
    )
}

/// Diagnostic used by --test-capture: save a JPEG, open it, report status.
pub fn test_capture() -> bool {
    match capture_frontmost() {
        Ok(result) => {
            let path = std::env::temp_dir().join("wheredo-test.jpg");
            let rgb = image::DynamicImage::ImageRgba8(result.image.clone()).to_rgb8();
            match rgb.save(&path) {
                Ok(()) => {
                    feedback::log(&format!(
                        "Capture saved: {} ({}x{}, source={:?})",
                        path.display(),
                        result.image.width(),
                        result.image.height(),
                        result.context.source
                    ));
                    let _ = open::that(&path);
                    feedback::log("VERDICT: OK");
                    true
                }
                Err(e) => {
                    feedback::error("Capture", &format!("could not save test image: {e}"));
                    false
                }
            }
        }
        Err(e) => {
            feedback::error("Capture", &e.to_string());
            feedback::log("VERDICT: BLOCKED");
            false
        }
    }
}
