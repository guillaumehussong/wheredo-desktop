# Build the Windows NSIS installer.
# Prerequisites: Rust stable, Node 20+, WebView2 runtime (bundled by NSIS).
# Output: src-tauri\target\release\bundle\nsis\Wheredo_<version>_x64-setup.exe

$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

npm install
npm run tauri build -- --bundles nsis

Write-Host ""
Write-Host "Installer:"
Get-ChildItem "src-tauri\target\release\bundle\nsis\*.exe" | ForEach-Object { Write-Host "  $($_.FullName)" }
