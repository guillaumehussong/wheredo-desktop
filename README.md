# Wheredo (Windows / Linux)

**Wheredo shows you where to click.** Press a hotkey, ask a question out loud — Wheredo looks at your screen, answers with voice, and points at the exact spot with a red guide cursor.

Powered by [xAI's Grok](https://x.ai) (vision, speech-to-text, text-to-speech). Requires a SuperGrok or X Premium account.

> Looking for the macOS app? See [wheredo-mac](../wheredo-mac) — a native Swift app with the same features and the same configuration.

## How it works

1. Press **Ctrl+Shift+B** (configurable) and speak your question.
2. Wheredo captures the active window, says *"Let me take a look…"*, and sends the screenshot + your question to Grok.
3. Grok answers out loud and a **red guide cursor** appears on the control it's talking about.
4. If you asked Wheredo to click for you, it asks for confirmation first — it never clicks silently.

## Install

Download from the [Releases](../../releases) page:

- **Windows:** run `Wheredo_x.x.x_x64-setup.exe`
- **Debian/Ubuntu:** `sudo dpkg -i wheredo_x.x.x_amd64.deb`
- **Any Linux:** make `Wheredo_x.x.x_amd64.AppImage` executable and run it

Then sign in once from a terminal:

```bash
wheredo-desktop --login
```

## Usage

Wheredo lives in the system tray. Press the hotkey (or tray menu → *Speak now*) and talk.

CLI mode:

```bash
wheredo-desktop "How do I open the settings in this app?"   # text question
wheredo-desktop --no-speak "…"        # without voice playback
wheredo-desktop --test-capture        # diagnose screen capture
wheredo-desktop --setup-permissions   # probe mic + screen capture
```

## Configuration

Copy [`.env.example`](.env.example) to `.env` in the Wheredo data folder:

- Windows: `%APPDATA%\Wheredo\.env`
- Linux: `~/.config/Wheredo/.env`

Key settings: `HOTKEY`, `STT_LANGUAGE` / `TTS_LANGUAGE`, `VISION_MODEL`, `SPEAK_FILLER`. All keys are shared with the macOS app.

## Build from source

Prerequisites: Rust stable (1.88+), Node 20+.

```bash
npm install
npm run tauri build
# Windows → src-tauri/target/release/bundle/nsis/*.exe
# Linux   → src-tauri/target/release/bundle/deb/*.deb + appimage/*.AppImage
```

Linux build dependencies (Debian/Ubuntu): see [`scripts/build-linux.sh`](scripts/build-linux.sh), or run it with `INSTALL_DEPS=1`.

## Platform notes

- **Linux Wayland:** screen capture needs `xdg-desktop-portal` + `pipewire`; global hotkeys and synthetic clicks may be restricted by the compositor. X11 has full support.
- **Windows:** SmartScreen may warn until the binary is code-signed.

## Privacy

Your microphone is only recorded while you ask a question, and your screen is only captured at that moment. Both go to the xAI API and nowhere else. OAuth tokens are stored locally (`oauth.json`, chmod 600).

## License

[MIT](LICENSE)
