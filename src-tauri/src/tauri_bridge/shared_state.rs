//! Shared state structures for communication between Tauri and Bevy
//!
//! This module defines thread-safe data structures that allow bidirectional
//! communication between the Tauri frontend and the Bevy render backend.

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

// =============================================================================
// Frame Buffer
// =============================================================================

/// Thread-safe RGBA frame buffer shared between Bevy and Tauri
/// Stores raw RGBA8 pixel data (4 bytes per pixel)
#[derive(Clone, Default)]
pub struct SharedFrameBuffer(pub Arc<Mutex<Option<Vec<u8>>>>);

/// Frame response containing Base64-encoded RGBA pixel data
#[derive(Serialize, Deserialize)]
pub struct FrameResponse {
    /// Base64-encoded RGBA pixel data (avoids slow JSON array serialization)
    pub data: String,
    pub width: u32,
    pub height: u32,
}

// =============================================================================
// Mouse Input
// =============================================================================

/// Mouse input state received from frontend
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct MouseInput {
    /// Accumulated X movement delta
    pub delta_x: f32,
    /// Accumulated Y movement delta
    pub delta_y: f32,
    /// Accumulated scroll wheel delta
    pub scroll_delta: f32,
    /// Left mouse button is pressed
    pub left_button: bool,
    /// Right mouse button is pressed
    pub right_button: bool,
}

/// Thread-safe mouse input shared between Tauri and Bevy
#[derive(Clone, Default)]
pub struct SharedMouseInput(pub Arc<Mutex<MouseInput>>);

// =============================================================================
// Performance Statistics
// =============================================================================

/// Performance statistics for debugging and monitoring
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct PerformanceStats {
    // Backend (Bevy/Rust) timings
    pub gpu_transfer_ms: f64,
    pub data_processing_ms: f64,
    pub frame_encoding_ms: f64,
    pub bevy_fps: f64,
    pub frame_count: u32,
    pub data_size_kb: f64,
    // Tauri command timings
    pub tauri_get_frame_ms: f64,
    pub tauri_serialize_ms: f64,
}

/// Thread-safe performance statistics
#[derive(Clone, Default)]
pub struct SharedPerfStats(pub Arc<Mutex<PerformanceStats>>);
