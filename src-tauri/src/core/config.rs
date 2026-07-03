//! Settings loaded from `.env` files and process environment.
//! Mirrors the macOS `Config.swift`: same keys, same defaults, same precedence
//! (process env overrides file values).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

fn file_values() -> &'static HashMap<String, String> {
    static VALUES: OnceLock<HashMap<String, String>> = OnceLock::new();
    VALUES.get_or_init(load_env_file)
}

/// App data dir: %APPDATA%\Wheredo (Windows) or ~/.config/Wheredo (Linux).
pub fn app_data_dir() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("Wheredo");
    let _ = std::fs::create_dir_all(&dir);
    migrate_legacy_data(&base, &dir);
    dir
}

/// One-time migration from the pre-rename "GrokBuddy" data dir: carry over
/// the OAuth tokens and .env so existing users stay logged in.
fn migrate_legacy_data(base: &std::path::Path, new_dir: &std::path::Path) {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        let old_dir = base.join("GrokBuddy");
        for name in ["oauth.json", ".env"] {
            let old = old_dir.join(name);
            let new = new_dir.join(name);
            if old.exists() && !new.exists() {
                let _ = std::fs::copy(&old, &new);
            }
        }
    });
}

/// `.env` lookup order — first existing file wins:
///   1. WHEREDO_ENV (explicit override)
///   2. ./.env (development runs)
///   3. next to the executable
///   4. <app data dir>/.env (installed launches)
fn env_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(explicit) = std::env::var("WHEREDO_ENV") {
        paths.push(PathBuf::from(explicit));
    }
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join(".env"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join(".env"));
        }
    }
    paths.push(app_data_dir().join(".env"));
    paths
}

fn load_env_file() -> HashMap<String, String> {
    for path in env_search_paths() {
        if let Ok(contents) = std::fs::read_to_string(&path) {
            return parse_env(&contents);
        }
    }
    HashMap::new()
}

fn parse_env(contents: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    for line in contents.lines() {
        let mut s = line.trim();
        if s.is_empty() || s.starts_with('#') {
            continue;
        }
        if let Some(stripped) = s.strip_prefix("export ") {
            s = stripped;
        }
        let Some(eq) = s.find('=') else { continue };
        let key = s[..eq].trim().to_string();
        let mut value = s[eq + 1..].trim();
        // Strip trailing inline comment only when the value is unquoted.
        let quoted = (value.starts_with('"') && value.ends_with('"') && value.len() >= 2)
            || (value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2);
        if quoted {
            value = &value[1..value.len() - 1];
        } else if let Some(hash) = value.find(" #") {
            value = value[..hash].trim();
        }
        result.insert(key, value.to_string());
    }
    result
}

fn string(key: &str, default: &str) -> String {
    if let Ok(v) = std::env::var(key) {
        if !v.is_empty() {
            return v;
        }
    }
    if let Some(v) = file_values().get(key) {
        if !v.is_empty() {
            return v.clone();
        }
    }
    default.to_string()
}

fn double(key: &str, default: f64) -> f64 {
    string(key, "").parse().unwrap_or(default)
}

fn int(key: &str, default: i64) -> i64 {
    string(key, "").parse().unwrap_or(default)
}

fn boolean(key: &str, default: bool) -> bool {
    let v = string(key, "");
    if v.is_empty() {
        return default;
    }
    v != "0" && v.to_lowercase() != "false"
}

// MARK: Models

pub fn vision_model() -> String { string("VISION_MODEL", "grok-4.3") }
pub fn tts_voice() -> String { string("TTS_VOICE", "eve") }
pub fn api_base() -> String { string("API_BASE", "https://api.x.ai/v1") }

// MARK: Vision speed / quality

pub fn vision_image_detail() -> String { string("VISION_IMAGE_DETAIL", "low") }
pub fn vision_jpeg_quality() -> f64 { double("VISION_JPEG_QUALITY", 0.6) }
/// Kept for .env parity with macOS; xcap always captures at native resolution.
#[allow(dead_code)]
pub fn capture_scale() -> u32 { int("CAPTURE_SCALE", 1).max(1) as u32 }
pub fn vision_max_tokens() -> i64 { int("VISION_MAX_TOKENS", 300).max(64) }
pub fn vision_temperature() -> f64 { double("VISION_TEMPERATURE", 0.2) }

// MARK: Speech

pub fn stt_language() -> String { string("STT_LANGUAGE", "en") }
pub fn tts_language() -> String { string("TTS_LANGUAGE", "en") }
pub fn stt_silence() -> f64 { double("STT_SILENCE", 0.9) }
pub fn tts_engine() -> String { string("TTS_ENGINE", "xai") }
pub fn speak_filler() -> bool { boolean("SPEAK_FILLER", true) }

// MARK: Guide cursor

pub fn show_guide_cursor() -> bool { boolean("SHOW_GUIDE_CURSOR", true) }
pub fn guide_cursor_duration() -> f64 { double("GUIDE_CURSOR_DURATION", 15.0) }

// MARK: Hotkey (desktop-specific; macOS uses ⌘$ hardcoded)

pub fn hotkey() -> String { string("HOTKEY", "Ctrl+Shift+B") }
