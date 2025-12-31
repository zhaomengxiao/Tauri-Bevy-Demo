//! Tauri-Bevy Demo: Headless Offscreen Rendering
//!
//! This application demonstrates Bevy's headless 3D rendering integrated with Tauri.
//! Based on official Bevy headless renderer example with proper GPU-CPU data transfer.
//!
//! Architecture:
//! - Bevy runs in a background thread with NO window (true headless mode)
//! - Uses proper RenderGraph pipeline with ImageCopyDriver node
//! - GPU texture -> Buffer -> CPU channel -> Tauri frontend
//! - Frame data transferred via custom protocol (JPEG compression) or Base64-encoded RGBA
//!
//! # Module Structure
//!
//! - `config`: Configuration constants and settings
//! - `tauri_bridge`: Bridge layer between Tauri and Bevy
//!   - `shared_state`: Thread-safe data structures
//!   - `commands`: Tauri command handlers
//!   - `protocol`: Custom protocol handlers
//! - `bevy`: Bevy engine integration
//!   - `components`: ECS components
//!   - `resources`: Global resources
//!   - `plugins`: Custom plugins
//!   - `systems`: Game systems
//!   - `app`: Application setup

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Module declarations
mod bevy;
mod config;
mod tauri_bridge;

use std::{thread, time::Duration};
use tauri_bridge::{SharedFrameBuffer, SharedMouseInput, SharedPerfStats};

/// Main entry point for the Tauri application
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    println!("[Tauri] Starting...");

    // Create shared state
    let buffer = SharedFrameBuffer::default();
    let perf_stats = SharedPerfStats::default();
    let mouse_input = SharedMouseInput::default();

    // Start Bevy in background thread
    bevy::start_bevy(buffer.clone(), perf_stats.clone(), mouse_input.clone());

    // Wait for Bevy to initialize
    thread::sleep(Duration::from_millis(1000));

    // Clone for the custom protocol handler
    let protocol_buffer = buffer.clone();
    let protocol_perf_stats = perf_stats.clone();

    // Build and run Tauri application
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(buffer)
        .manage(perf_stats)
        .manage(mouse_input)
        // Register custom protocol "frame://" for direct binary transfer
        // This bypasses Tauri IPC JSON serialization completely!
        .register_asynchronous_uri_scheme_protocol("frame", move |_ctx, request, responder| {
            let buffer = protocol_buffer.clone();
            let perf_stats = protocol_perf_stats.clone();

            // Handle the request in a separate thread to avoid blocking
            std::thread::spawn(move || {
                let uri = request.uri();
                let path = uri.path();

                println!("[Protocol] Request URI: {}, path: {}", uri, path);

                // For Tauri v2, URL format is: http://frame.localhost/path
                let response =
                    tauri_bridge::protocol::handle_frame_protocol(path, &buffer, &perf_stats);
                responder.respond(response);
            });
        })
        .invoke_handler(tauri::generate_handler![
            tauri_bridge::commands::get_frame,
            tauri_bridge::commands::get_render_size,
            tauri_bridge::commands::get_performance_stats,
            tauri_bridge::commands::send_mouse_input
        ])
        .run(tauri::generate_context!())
        .expect("Tauri error");
}
