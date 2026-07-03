//! Wheredo desktop (Windows/Linux) — Tauri entry point.
//!
//! Two run modes, mirroring the macOS CLI:
//! - CLI mode (`--login`, `--test-capture`, `--setup-permissions`, or a text
//!   question argument): runs headless on a tokio runtime and exits.
//! - Tray mode (no args): system tray + global hotkey; each hotkey press
//!   triggers one assistant cycle.

// Prevent an extra console window on Windows in release builds.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod assistant;
mod core;
mod platform;
mod tray;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::AppHandle;
use tauri_plugin_global_shortcut::ShortcutState;

use crate::core::{config, feedback};

const USAGE: &str = r#"Wheredo desktop

Usage:
  wheredo-desktop                     start tray mode (hotkey + tray icon)
  wheredo-desktop "question"          ask one question about the screen (spoken answer)
  wheredo-desktop --no-speak "q"      ask without TTS playback
  wheredo-desktop --login             xAI sign-in (device code flow)
  wheredo-desktop --logout            forget stored tokens
  wheredo-desktop --test-capture      capture the screen and open the JPEG
  wheredo-desktop --setup-permissions probe mic + screen capture with guidance
"#;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut speak = true;
    let mut question: Option<String> = None;
    let mut cli_action: Option<&str> = None;

    for arg in &args {
        match arg.as_str() {
            "--no-speak" => speak = false,
            "--login" | "--logout" | "--test-capture" | "--setup-permissions" => {
                cli_action = Some(match arg.as_str() {
                    "--login" => "login",
                    "--logout" => "logout",
                    "--test-capture" => "test-capture",
                    _ => "setup-permissions",
                });
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                return;
            }
            other if !other.starts_with("--") => {
                question = Some(match question {
                    Some(prev) => format!("{prev} {other}"),
                    None => other.to_string(),
                });
            }
            other => {
                eprintln!("Unknown flag: {other}\n{USAGE}");
                return;
            }
        }
    }

    // Headless CLI modes run without the Tauri event loop.
    if cli_action.is_some() || question.is_some() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        runtime.block_on(async {
            match cli_action {
                Some("login") => match core::oauth::login().await {
                    Ok(_) => {}
                    Err(e) => feedback::error("Login failed", &e.to_string()),
                },
                Some("logout") => {
                    core::oauth::clear();
                    feedback::log("Tokens cleared.");
                }
                Some("test-capture") => {
                    let _ = tokio::task::spawn_blocking(platform::capture::test_capture).await;
                }
                Some("setup-permissions") => platform::permissions::run_setup().await,
                _ => {
                    if core::oauth::load().is_none() {
                        feedback::error("Not logged in", "run with --login first");
                        return;
                    }
                    assistant::run_cycle(assistant::CycleOptions {
                        use_voice: false,
                        speak,
                        question,
                        app: None,
                    })
                    .await;
                }
            }
        });
        return;
    }

    run_tray_app();
}

fn run_tray_app() {
    let busy = Arc::new(AtomicBool::new(false));
    let busy_for_hotkey = busy.clone();
    let hotkey = config::hotkey();

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_shortcuts([hotkey.as_str()])
                .unwrap_or_else(|e| {
                    feedback::error(
                        "Hotkey",
                        &format!("could not parse HOTKEY '{hotkey}': {e} — using Ctrl+Shift+B"),
                    );
                    tauri_plugin_global_shortcut::Builder::new()
                        .with_shortcuts(["Ctrl+Shift+B"])
                        .expect("default hotkey valid")
                })
                .with_handler(move |app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        trigger_cycle(app.clone(), busy_for_hotkey.clone());
                    }
                })
                .build(),
        )
        .setup(move |app| {
            let handle = app.handle().clone();

            let busy_for_tray = busy.clone();
            tray::install(&handle, move |app| {
                trigger_cycle(app.clone(), busy_for_tray.clone());
            })?;

            platform::permissions::warn_wayland_limitations();
            platform::permissions::first_run_wizard();
            feedback::log(&format!(
                "━━━ Wheredo active ━━━\nTray icon is running. Press {} to speak.",
                config::hotkey()
            ));

            if core::oauth::load().is_none() {
                feedback::error(
                    "Not logged in",
                    "Run once from a terminal: wheredo-desktop --login",
                );
            }

            // Pre-generate the Grok-voice filler audio so it plays instantly.
            tauri::async_runtime::spawn(core::speech::filler::warm_cache());

            // Debug helper: WHEREDO_TEST_OVERLAY=1 shows a sample marker so the
            // overlay rendering can be checked without a full voice cycle.
            if std::env::var("WHEREDO_TEST_OVERLAY").as_deref() == Ok("1") {
                platform::overlay::show_markers(
                    &handle,
                    vec![platform::overlay::Marker { x: 600, y: 400, label: "Test marker".into() }],
                );
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Wheredo");
}

/// One cycle per hotkey press; concurrent presses are rejected while busy.
fn trigger_cycle(app: AppHandle, busy: Arc<AtomicBool>) {
    if busy.swap(true, Ordering::SeqCst) {
        feedback::log("⏳ Wheredo is busy…");
        return;
    }
    tauri::async_runtime::spawn(async move {
        assistant::run_cycle(assistant::CycleOptions {
            use_voice: true,
            speak: true,
            question: None,
            app: Some(app.clone()),
        })
        .await;
        busy.store(false, Ordering::SeqCst);
        feedback::log(&format!("{} — ready for another question.", config::hotkey()));
    });
}
