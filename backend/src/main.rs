//! # DiskDeck Application Entry Point
//!
//! Thin wrapper that delegates to [`diskdeck_backend_lib::run`].
//!
//! The `windows_subsystem = "windows"` attribute hides the console window on
//! Windows release builds while keeping it visible during debug sessions.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    diskdeck_backend_lib::run()
}
