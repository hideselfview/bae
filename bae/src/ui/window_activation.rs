#[cfg(target_os = "macos")]
mod macos_window;

#[cfg(target_os = "macos")]
pub use macos_window::{setup_macos_window_activation, setup_transparent_titlebar};

#[cfg(not(target_os = "macos"))]
pub fn setup_macos_window_activation() {
    // No-op on non-macOS platforms
}

#[cfg(not(target_os = "macos"))]
pub fn setup_transparent_titlebar() {
    // No-op on non-macOS platforms
}
