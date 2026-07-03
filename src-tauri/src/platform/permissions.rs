//! OS-specific permission checks and guidance (port of PermissionSetup.swift).
//! Windows/Linux have no TCC: mic/screen access is either a Settings toggle
//! (Windows) or a portal consent dialog (Linux Wayland).

use crate::core::feedback;
use crate::core::speech::VoiceError;

/// Probe the microphone. A silent mic still counts as accessible — only a
/// stream/device error means access is blocked.
fn mic_accessible() -> bool {
    match crate::core::speech::record_until_silence(
        std::time::Duration::from_secs(1),
        std::time::Duration::from_millis(400),
    ) {
        Ok(_) | Err(VoiceError::NoAudioDetected) => true,
        Err(_) => false,
    }
}

/// First-run / --setup-permissions flow: probe mic and screen capture,
/// print actionable guidance for anything that fails.
pub async fn run_setup() {
    feedback::log("━━━ Wheredo permission setup ━━━");

    feedback::log("1/2 Microphone…");
    let mic_ok = tokio::task::spawn_blocking(mic_accessible)
        .await
        .unwrap_or(false);

    if mic_ok {
        feedback::log("✓ Microphone works");
    } else {
        feedback::error("Microphone", mic_help());
        open_mic_settings();
    }

    feedback::log("2/2 Screen capture…");
    let capture_ok =
        tokio::task::spawn_blocking(crate::platform::capture::test_capture)
            .await
            .unwrap_or(false);
    if capture_ok {
        feedback::log("✓ Screen capture works");
    } else {
        feedback::error("Screen capture", screen_help());
    }

    feedback::log("Setup finished.");
}

pub fn mic_help() -> &'static str {
    if cfg!(target_os = "windows") {
        "No audio detected. Check: Settings → Privacy & security → Microphone →\n\
         'Let desktop apps access your microphone' must be ON, and a working\n\
         input device must be selected in Settings → System → Sound."
    } else {
        "No audio detected. Check your input device (pavucontrol / pw-top) and,\n\
         inside Flatpak/sandboxes, allow microphone access via the portal."
    }
}

pub fn screen_help() -> &'static str {
    if cfg!(target_os = "windows") {
        "Screen capture failed. No permission is normally needed on Windows —\n\
         make sure no other app blocks capture (some DRM/kiosk software does)."
    } else {
        "Screen capture failed. On X11 this should just work.\n\
         On Wayland install xdg-desktop-portal + pipewire and accept the\n\
         screen-share dialog when it appears. If no dialog shows, your\n\
         compositor may not implement the ScreenCast portal."
    }
}

pub fn open_mic_settings() {
    #[cfg(target_os = "windows")]
    {
        let _ = open::that("ms-settings:privacy-microphone");
    }
}

/// First-run wizard (tray mode): one welcome dialog with the essentials, then
/// a background mic + screen-capture probe whose failures produce actionable
/// guidance in the log. Runs once, tracked by a marker file.
pub fn first_run_wizard() {
    let marker = crate::core::config::app_data_dir().join(".first-run-done");
    if marker.exists() {
        return;
    }
    let _ = std::fs::write(&marker, b"1");

    let hotkey = crate::core::config::hotkey();
    let login_hint = if crate::core::oauth::load().is_none() {
        "\n\nNot signed in yet: open a terminal and run\n    wheredo-desktop --login"
    } else {
        ""
    };

    // Dialog is blocking — keep it off the main/event-loop thread.
    std::thread::spawn(move || {
        rfd::MessageDialog::new()
            .set_title("Welcome to Wheredo")
            .set_description(format!(
                "Wheredo lives in your system tray.\n\n\
                 Press {hotkey} and ask a question out loud — Grok looks at your \
                 screen, answers with voice, and shows a red guide cursor where to click.\n\n\
                 Your microphone and screen are only used while you ask a question.{login_hint}"
            ))
            .set_level(rfd::MessageLevel::Info)
            .show();
    });

    // Probe both permissions in the background; failures land in the log
    // with OS-specific instructions.
    tauri::async_runtime::spawn(async {
        let mic_ok = tokio::task::spawn_blocking(mic_accessible)
            .await
            .unwrap_or(false);
        if !mic_ok {
            feedback::error("Microphone", mic_help());
            open_mic_settings();
        }

        let capture_ok = tokio::task::spawn_blocking(|| {
            crate::platform::capture::capture_frontmost().is_ok()
        })
        .await
        .unwrap_or(false);
        if !capture_ok {
            feedback::error("Screen capture", screen_help());
        }
    });
}

/// Wayland restricts synthetic input and global hotkeys; warn once at startup.
pub fn warn_wayland_limitations() {
    if cfg!(target_os = "linux") && std::env::var("WAYLAND_DISPLAY").is_ok() {
        feedback::log(
            "ℹ Wayland session detected: global hotkeys and auto-click may be\n\
             restricted by your compositor. X11 sessions have full support.",
        );
    }
}
