//! One full Wheredo cycle: voice question → screen capture → vision → voice
//! answer. Port of macOS `Assistant.swift`, including the parallel filler.
//!
//! Pipeline timeline (typical, with SPEAK_FILLER=true):
//!   0.0 s  user stops talking → STT transcription returned
//!   0.0 s  filler audio starts ("Let me take a look…")      ┐ run in
//!   0.1 s  screenshot captured                              │ parallel
//!   ~5 s   vision model returns answer + pointer coords     ┘
//!   ~5 s   filler finished → answer is spoken, red guide cursor appears

use std::time::{Duration, Instant};

use tauri::AppHandle;

use crate::core::{config, feedback, speech, vision};
use crate::platform::{actions, capture, overlay};
use crate::tray;

pub struct CycleOptions {
    pub use_voice: bool,
    pub speak: bool,
    pub question: Option<String>,
    /// None in headless CLI mode — overlay and tray updates are skipped.
    pub app: Option<AppHandle>,
}

pub async fn run_cycle(opts: CycleOptions) {
    if let Some(app) = &opts.app {
        tray::set_status(app, tray::Status::Busy);
    }

    let result = run_cycle_inner(&opts).await;

    if let Some(app) = &opts.app {
        let status = if result.is_ok() { tray::Status::Ready } else { tray::Status::Error };
        tray::set_status(app, status);
    }
    if let Err(message) = result {
        feedback::error("Error", &message);
        // Tray mode has no console — surface the failure as an OS notification.
        if let Some(app) = &opts.app {
            use tauri_plugin_notification::NotificationExt;
            let _ = app
                .notification()
                .builder()
                .title("Wheredo error")
                .body(&message)
                .show();
        }
    }
}

async fn run_cycle_inner(opts: &CycleOptions) -> Result<(), String> {
    // Per-step stopwatch: every stage logs its duration to wheredo.log.
    let mut t = Instant::now();
    let mut lap = move |label: &str| {
        feedback::log(&format!("   ⏱ {}: {:.1}s", label, t.elapsed().as_secs_f64()));
        t = Instant::now();
    };

    let question = if opts.use_voice {
        if let Some(app) = &opts.app {
            tray::set_status(app, tray::Status::Listening);
        }
        feedback::log("🎙 Speak your question…");
        let q = speech::listen(Duration::from_secs(30))
            .await
            .map_err(|e| e.to_string())?;
        if let Some(app) = &opts.app {
            tray::set_status(app, tray::Status::Busy);
        }
        feedback::log(&format!("🗣 You: {q}"));
        lap("voice + transcription");
        q
    } else if let Some(q) = opts.question.clone().filter(|q| !q.is_empty()) {
        q
    } else {
        return Err("no question provided".into());
    };

    dismiss_overlay(opts).await;

    // Clicky-style: speak "let me take a look…" while capture + vision run.
    let filler_task = if opts.speak && opts.use_voice {
        Some(tauri::async_runtime::spawn(speech::filler::speak()))
    } else {
        None
    };

    feedback::log("📸 Capturing screen…");
    let capture_result = tokio::task::spawn_blocking(capture::capture_frontmost)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let b64 = capture::jpeg_base64(&capture_result.image).map_err(|e| e.to_string())?;
    lap("capture");

    feedback::log("👁 Analyzing…");
    let app_ctx = capture::frontmost_context(&capture_result);
    let result = vision::analyze(&b64, &question, &app_ctx)
        .await
        .map_err(|e| e.to_string())?;
    feedback::log(&format!("💬 Grok: {}", result.spoken_answer));
    lap(&format!("vision ({})", config::vision_model()));

    // Never talk over the filler.
    if let Some(task) = filler_task {
        let _ = task.await;
    }

    // Show the red guide cursor before speaking so the user can already see
    // WHERE to click while hearing the explanation.
    show_overlay(opts, &result, &capture_result).await;

    if opts.speak {
        feedback::log("🔊 Playing response…");
        match speech::speak(&result.spoken_answer).await {
            Ok(()) => lap(&format!("speech ({})", config::tts_engine())),
            Err(e) => feedback::error("Voice unavailable", &e.to_string()),
        }
        // Re-show after speech: TTS playback can outlive the overlay timer.
        show_overlay(opts, &result, &capture_result).await;
    }

    // Optional auto-click: only when the model set click=true, and always
    // behind a user confirmation dialog (never click silently).
    for action in result.actions.iter().filter(|a| a.click) {
        let Some(point) = action.point else { continue };
        let (x, y) = capture_result.context.point_in_screen_space(point.x, point.y);
        let label = action.label.clone();
        let approved =
            tokio::task::spawn_blocking(move || actions::confirm_click_blocking(x, y, &label))
                .await
                .unwrap_or(false);
        if approved {
            let _ = tokio::task::spawn_blocking(move || actions::click(x, y)).await;
        }
    }

    Ok(())
}

async fn show_overlay(
    opts: &CycleOptions,
    result: &vision::VisionResult,
    capture_result: &capture::CaptureResult,
) {
    let Some(app) = &opts.app else { return };
    if result.actions.is_empty() {
        return;
    }

    let markers: Vec<overlay::Marker> = result
        .actions
        .iter()
        .filter_map(|action| {
            let point = action.point?;
            let (x, y) = capture_result.context.point_in_screen_space(point.x, point.y);
            Some(overlay::Marker { x, y, label: action.label.clone() })
        })
        .collect();

    if markers.is_empty() {
        feedback::log(&format!(
            "⚠️  Vision returned {} action(s) but no valid point coordinates.",
            result.actions.len()
        ));
        return;
    }

    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        overlay::show_markers(&app, markers);
    });
}

async fn dismiss_overlay(opts: &CycleOptions) {
    let Some(app) = &opts.app else { return };
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        overlay::dismiss_all(&app);
    });
}
