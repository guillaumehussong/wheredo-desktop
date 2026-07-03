#!/usr/bin/env bash
# Build the Linux .deb and AppImage bundles.
# Output: src-tauri/target/release/bundle/deb/*.deb
#         src-tauri/target/release/bundle/appimage/*.AppImage
set -euo pipefail
cd "$(dirname "$0")/.."

# Build prerequisites (Debian/Ubuntu). Runtime needs: xdg-desktop-portal + pipewire (Wayland capture).
if command -v apt-get >/dev/null && [ "${INSTALL_DEPS:-0}" = "1" ]; then
  sudo apt-get update
  sudo apt-get install -y \
    libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf \
    build-essential curl wget file libssl-dev libgtk-3-dev \
    libasound2-dev libxdo-dev \
    pkg-config libclang-dev libxcb1-dev libxrandr-dev libdbus-1-dev \
    libpipewire-0.3-dev libwayland-dev libegl-dev
fi

npm install
npm run tauri build -- --bundles deb,appimage

echo ""
echo "Bundles:"
ls -1 src-tauri/target/release/bundle/deb/*.deb src-tauri/target/release/bundle/appimage/*.AppImage 2>/dev/null
