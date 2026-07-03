pub mod actions;
pub mod capture;
#[cfg(target_os = "linux")]
pub mod capture_linux;
#[cfg(target_os = "windows")]
pub mod capture_windows;
pub mod overlay;
pub mod permissions;
