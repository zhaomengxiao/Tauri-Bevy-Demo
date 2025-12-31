//! Bevy resource definitions
//!
//! This module contains all global resources used by Bevy systems.
//! Resources are singleton data that can be accessed by any system.

use bevy::prelude::*;
use std::time::Duration;

use crate::tauri_bridge::shared_state::{
    SharedFrameBuffer, SharedMouseInput, SharedPerfStats,
};

// =============================================================================
// Camera Control
// =============================================================================

/// Orbit camera state for spherical coordinate camera control
#[derive(Resource)]
pub struct OrbitCameraState {
    /// Horizontal rotation angle (radians)
    pub yaw: f32,
    /// Vertical rotation angle (radians), clamped to avoid gimbal lock
    pub pitch: f32,
    /// Distance from the camera to the center point
    pub distance: f32,
    /// The point the camera orbits around
    pub center: Vec3,
}

impl Default for OrbitCameraState {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.4, // Slight downward angle
            distance: 6.5,
            center: Vec3::ZERO,
        }
    }
}

/// Resource to hold shared mouse input in Bevy
#[derive(Resource)]
pub struct MouseInputRes(pub SharedMouseInput);

// =============================================================================
// Rendering
// =============================================================================

/// Handle to the offscreen render target texture
#[derive(Resource)]
pub struct RenderTargetHandle(pub Handle<Image>);

/// Shared frame buffer resource for Bevy
#[derive(Resource, Clone)]
pub struct FrameBufferRes(pub SharedFrameBuffer);

// =============================================================================
// Frame Management
// =============================================================================

/// Counter for total frames rendered
#[derive(Resource, Default)]
pub struct FrameCount(pub u32);

/// Number of pre-roll frames to skip before starting output
#[derive(Resource, Default)]
pub struct PreRollFrames(pub u32);

/// Frame rate limiter to control output FPS
#[derive(Resource)]
pub struct FrameRateLimiter {
    pub last_frame_time: std::time::Instant,
    pub min_frame_interval: Duration,
}

impl FrameRateLimiter {
    pub fn new(target_fps: f64) -> Self {
        Self {
            last_frame_time: std::time::Instant::now(),
            min_frame_interval: Duration::from_secs_f64(1.0 / target_fps),
        }
    }
}

impl Default for FrameRateLimiter {
    fn default() -> Self {
        Self::new(60.0) // Default to 60 FPS
    }
}

// =============================================================================
// Performance Monitoring
// =============================================================================

/// Performance timing tracker for frame processing
#[derive(Resource, Default)]
pub struct FrameTimings {
    pub last_print_time: f64,
    pub frame_times: Vec<f64>,
}

/// Shared performance statistics resource
#[derive(Resource)]
pub struct PerfStatsRes(pub SharedPerfStats);

// =============================================================================
// Channel Communication (Main World <-> Render World)
// =============================================================================

use crossbeam_channel::{Receiver, Sender};

/// Receives data from render world
#[derive(Resource, Deref)]
pub struct MainWorldReceiver(pub Receiver<Vec<u8>>);

/// Sends data to main world
#[derive(Resource, Deref)]
pub struct RenderWorldSender(pub Sender<Vec<u8>>);
