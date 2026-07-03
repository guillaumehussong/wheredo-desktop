//! Voice I/O: mic recording with silence detection (cpal), STT via /v1/stt,
//! TTS via /v1/tts played with rodio, plus the Clicky-style filler cache.
//! Ports macOS `AudioRecorder.swift`, `SpeechToText.swift` and `Voice.swift`.

use std::io::Cursor;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde_json::json;

use super::{config, feedback, http, oauth};

#[derive(Debug)]
pub enum VoiceError {
    MicUnavailable(String),
    NoAudioDetected,
    ApiFailed(u16, String),
    Network(String),
    Playback(String),
    Auth(oauth::OAuthError),
}

impl std::fmt::Display for VoiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VoiceError::MicUnavailable(e) => write!(f, "Microphone unavailable: {e}"),
            VoiceError::NoAudioDetected => write!(f, "No audio detected — check your microphone"),
            VoiceError::ApiFailed(code, body) => write!(f, "Voice API {code}: {body}"),
            VoiceError::Network(e) => write!(f, "Network error: {e}"),
            VoiceError::Playback(e) => write!(f, "Audio playback error: {e}"),
            VoiceError::Auth(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for VoiceError {}

impl From<oauth::OAuthError> for VoiceError {
    fn from(e: oauth::OAuthError) -> Self {
        VoiceError::Auth(e)
    }
}

impl From<reqwest::Error> for VoiceError {
    fn from(e: reqwest::Error) -> Self {
        VoiceError::Network(e.to_string())
    }
}

pub struct Recording {
    /// Mono 16-bit little-endian PCM.
    pub pcm: Vec<i16>,
    pub sample_rate: u32,
}

/// Record from the default input device until the user stops speaking.
///
/// End-of-speech logic (same as macOS): RMS of each buffer vs a threshold;
/// stop when speech was heard for >= 0.4 s and the last loud buffer is older
/// than `silence_duration`. Safety exits: `max_duration` cap and a 10 s
/// no-speech timeout.
pub fn record_until_silence(
    max_duration: Duration,
    silence_duration: Duration,
) -> Result<Recording, VoiceError> {
    const SILENCE_THRESHOLD: f32 = 0.008;
    const NO_SPEECH_TIMEOUT: Duration = Duration::from_secs(10);

    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| VoiceError::MicUnavailable("no default input device".into()))?;
    let cfg = device
        .default_input_config()
        .map_err(|e| VoiceError::MicUnavailable(e.to_string()))?;
    let sample_rate = cfg.sample_rate().0;
    let channels = cfg.channels() as usize;
    feedback::log(&format!("🎤 Mic: {sample_rate} Hz, {channels} ch"));

    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let err_tx = tx.clone();

    // The cpal stream delivers interleaved f32 buffers on its own thread;
    // we downmix to mono and run silence detection on this thread.
    let stream = device
        .build_input_stream(
            &cfg.into(),
            move |data: &[f32], _| {
                let mono: Vec<f32> = data
                    .chunks(channels)
                    .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                    .collect();
                let _ = tx.send(mono);
            },
            move |e| {
                feedback::error("Mic stream", &e.to_string());
                let _ = err_tx.send(vec![]);
            },
            None,
        )
        .map_err(|e| VoiceError::MicUnavailable(e.to_string()))?;
    stream.play().map_err(|e| VoiceError::MicUnavailable(e.to_string()))?;

    let started = Instant::now();
    let mut pcm: Vec<i16> = Vec::new();
    let mut heard_speech = false;
    let mut last_loud = Instant::now();
    let mut speech_started: Option<Instant> = None;

    loop {
        let Ok(buffer) = rx.recv_timeout(Duration::from_millis(500)) else {
            if started.elapsed() >= max_duration {
                break;
            }
            continue;
        };

        for s in &buffer {
            pcm.push((s.clamp(-1.0, 1.0) * 32767.0) as i16);
        }

        let rms = (buffer.iter().map(|s| s * s).sum::<f32>() / buffer.len().max(1) as f32).sqrt();
        let now = Instant::now();
        if rms > SILENCE_THRESHOLD {
            heard_speech = true;
            last_loud = now;
            speech_started.get_or_insert(now);
        }

        let timed_out = started.elapsed() >= max_duration;
        let spoke_long_enough = speech_started
            .map(|t| now.duration_since(t) >= Duration::from_millis(400))
            .unwrap_or(false);
        let silent_long_enough =
            heard_speech && spoke_long_enough && now.duration_since(last_loud) >= silence_duration;
        let no_speech_yet = !heard_speech && started.elapsed() >= NO_SPEECH_TIMEOUT;

        if silent_long_enough || timed_out || no_speech_yet {
            break;
        }
    }
    drop(stream);

    if !heard_speech {
        return Err(VoiceError::NoAudioDetected);
    }
    Ok(Recording { pcm, sample_rate })
}

/// Wrap PCM in a WAV container (in memory) using hound.
pub fn wav_from_recording(rec: &Recording) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rec.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec).expect("wav writer");
        for s in &rec.pcm {
            let _ = writer.write_sample(*s);
        }
        writer.finalize().expect("wav finalize");
    }
    cursor.into_inner()
}

/// Record the user's spoken question and return its transcription.
pub async fn listen(timeout: Duration) -> Result<String, VoiceError> {
    let silence = Duration::from_secs_f64(config::stt_silence());
    let rec = tokio::task::spawn_blocking(move || record_until_silence(timeout, silence))
        .await
        .map_err(|e| VoiceError::MicUnavailable(e.to_string()))??;

    let seconds = rec.pcm.len() as f64 / rec.sample_rate as f64;
    feedback::log(&format!("🔊 {seconds:.1} s recorded — transcribing…"));

    let wav = wav_from_recording(&rec);
    transcribe(wav).await
}

async fn transcribe(wav: Vec<u8>) -> Result<String, VoiceError> {
    let token = oauth::access_token().await?;
    let resp = http::post_stt(&config::api_base(), wav, &config::stt_language(), &token).await?;
    if resp.status != 200 {
        return Err(VoiceError::ApiFailed(resp.status, resp.text()));
    }
    let json: serde_json::Value =
        serde_json::from_slice(&resp.body).map_err(|e| VoiceError::Network(e.to_string()))?;
    match json["text"].as_str() {
        Some(text) if !text.trim().is_empty() => Ok(text.trim().to_string()),
        _ => Err(VoiceError::ApiFailed(resp.status, "empty STT response".into())),
    }
}

/// Generate MP3 audio via the xAI TTS endpoint (no playback).
pub async fn synthesize_mp3(text: &str, voice: &str) -> Result<Vec<u8>, VoiceError> {
    let token = oauth::access_token().await?;
    let body = json!({
        "text": text,
        "voice_id": voice,
        "language": config::tts_language(),
        "output_format": { "codec": "mp3", "sample_rate": 24000 }
    });
    let resp = http::post_json(&format!("{}/tts", config::api_base()), &body, Some(&token)).await?;
    if resp.status != 200 {
        return Err(VoiceError::ApiFailed(resp.status, resp.text()));
    }

    // Response may be raw MP3 bytes or JSON with base64 "audio" field.
    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&resp.body) {
        if let Some(b64) = json["audio"].as_str() {
            use base64::Engine;
            if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(b64) {
                return Ok(decoded);
            }
        }
    }
    Ok(resp.body)
}

/// Play MP3 data, blocking until playback finishes (call via spawn_blocking).
pub fn play_mp3_blocking(data: Vec<u8>) -> Result<(), VoiceError> {
    let (_stream, handle) =
        rodio::OutputStream::try_default().map_err(|e| VoiceError::Playback(e.to_string()))?;
    let sink = rodio::Sink::try_new(&handle).map_err(|e| VoiceError::Playback(e.to_string()))?;
    let source = rodio::Decoder::new(Cursor::new(data))
        .map_err(|e| VoiceError::Playback(e.to_string()))?;
    sink.append(source);
    sink.sleep_until_end();
    Ok(())
}

/// Speak text aloud with the configured Grok voice.
pub async fn speak(text: &str) -> Result<(), VoiceError> {
    let audio = synthesize_mp3(text, &config::tts_voice()).await?;
    tokio::task::spawn_blocking(move || play_mp3_blocking(audio))
        .await
        .map_err(|e| VoiceError::Playback(e.to_string()))?
}

// MARK: - Filler ("Let me take a look…")

/// Clicky-style instant filler played while the model thinks. Each phrase is
/// synthesized once with the Grok voice, cached as MP3, then replayed instantly.
pub mod filler {
    use super::*;
    use std::path::PathBuf;

    pub fn phrases() -> Vec<&'static str> {
        if config::tts_language().starts_with("fr") {
            vec!["Laisse-moi regarder ça.", "Je regarde ton écran.", "Un instant, je vérifie."]
        } else {
            vec!["Let me take a look.", "Checking your screen.", "One moment, looking into it."]
        }
    }

    fn cache_dir() -> PathBuf {
        let dir = config::app_data_dir().join("fillers");
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// Cache key includes language AND voice so changing either regenerates audio.
    fn cache_url(index: usize) -> PathBuf {
        cache_dir().join(format!(
            "{}-{}-{}.mp3",
            config::tts_language(),
            config::tts_voice(),
            index
        ))
    }

    /// Speak a random cached filler. If not cached yet, skip (and warm the
    /// cache in the background so the next question gets one).
    pub async fn speak() {
        if !config::speak_filler() {
            return;
        }
        let index = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as usize
            % phrases().len();

        if let Ok(data) = std::fs::read(cache_url(index)) {
            let _ = tokio::task::spawn_blocking(move || play_mp3_blocking(data)).await;
            return;
        }
        tokio::spawn(warm_cache());
    }

    /// Pre-generate all filler audio with the Grok voice (background, startup).
    pub async fn warm_cache() {
        for (i, phrase) in phrases().iter().enumerate() {
            let url = cache_url(i);
            if url.exists() {
                continue;
            }
            match synthesize_mp3(phrase, &config::tts_voice()).await {
                Ok(data) => {
                    let _ = std::fs::write(&url, data);
                }
                Err(e) => {
                    feedback::log(&format!("⚠️  Filler cache: {e}"));
                    return;
                }
            }
        }
    }
}
