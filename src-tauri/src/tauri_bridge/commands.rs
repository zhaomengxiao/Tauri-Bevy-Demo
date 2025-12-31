//! Tauri command handlers
//!
//! This module contains all the Tauri command functions that can be invoked
//! from the frontend JavaScript/TypeScript code.

use base64::{engine::general_purpose::STANDARD, Engine};
use tauri::State;

use crate::config::{RENDER_WIDTH, RENDER_HEIGHT};
use super::shared_state::{
    SharedFrameBuffer, SharedMouseInput, SharedPerfStats,
    FrameResponse, PerformanceStats,
};

/// Get the current rendered frame as Base64-encoded RGBA data
#[tauri::command]
pub fn get_frame(
    state: State<SharedFrameBuffer>,
    perf_state: State<SharedPerfStats>,
) -> Result<FrameResponse, String> {
    let cmd_start = std::time::Instant::now();

    let guard = state.0.lock().map_err(|e| e.to_string())?;
    let result = match &*guard {
        Some(rgba_data) => {
            let data_fetch_time = cmd_start.elapsed().as_secs_f64() * 1000.0;

            // Measure Base64 encoding time
            let encode_start = std::time::Instant::now();
            let base64_data = STANDARD.encode(rgba_data);
            let encode_time = encode_start.elapsed().as_secs_f64() * 1000.0;

            // Update perf stats
            if let Ok(mut stats) = perf_state.0.lock() {
                stats.tauri_get_frame_ms = data_fetch_time;
                stats.tauri_serialize_ms = encode_time;
            }

            Ok(FrameResponse {
                data: base64_data,
                width: RENDER_WIDTH,
                height: RENDER_HEIGHT,
            })
        }
        None => Err("No frame yet (scene still loading)".into()),
    };

    result
}

/// Get the render resolution
#[tauri::command]
pub fn get_render_size() -> (u32, u32) {
    (RENDER_WIDTH, RENDER_HEIGHT)
}

/// Get performance statistics
#[tauri::command]
pub fn get_performance_stats(state: State<SharedPerfStats>) -> Result<PerformanceStats, String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    Ok(guard.clone())
}

/// Receive mouse input from frontend for camera control
/// Input deltas are accumulated until consumed by Bevy
#[tauri::command]
pub fn send_mouse_input(
    state: State<SharedMouseInput>,
    delta_x: f32,
    delta_y: f32,
    scroll_delta: f32,
    left_button: bool,
    right_button: bool,
) -> Result<(), String> {
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    // Accumulate deltas (will be cleared when Bevy reads them)
    guard.delta_x += delta_x;
    guard.delta_y += delta_y;
    guard.scroll_delta += scroll_delta;
    // Button state is just the current state
    guard.left_button = left_button;
    guard.right_button = right_button;
    Ok(())
}
