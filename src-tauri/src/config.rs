//! Configuration constants and settings for the Tauri-Bevy integration
//!
//! This module contains all configurable parameters such as render resolution,
//! frame rates, and performance tuning settings.

/// Width of the offscreen render target in pixels
pub const RENDER_WIDTH: u32 = 800;

/// Height of the offscreen render target in pixels
pub const RENDER_HEIGHT: u32 = 600;

/// Target frames per second for the Bevy render loop
pub const TARGET_FPS: f64 = 60.0;

/// Number of pre-roll frames to skip before starting output
/// This allows the scene to fully load and stabilize
pub const PRE_ROLL_FRAMES: u32 = 30;

/// Camera control settings
pub mod camera {
    /// Rotation speed multiplier for mouse drag
    pub const ROTATION_SPEED: f32 = 0.005;

    /// Zoom speed multiplier for scroll wheel
    pub const ZOOM_SPEED: f32 = 0.5;

    /// Minimum camera distance from center point
    pub const MIN_DISTANCE: f32 = 2.0;

    /// Maximum camera distance from center point
    pub const MAX_DISTANCE: f32 = 20.0;

    /// Maximum pitch angle (radians) to prevent camera flipping
    pub const MAX_PITCH: f32 = 1.5;

    /// Minimum pitch angle (radians) to prevent camera flipping
    pub const MIN_PITCH: f32 = -1.5;
}

/// Performance monitoring settings
pub mod performance {
    /// Interval for printing performance stats (seconds)
    pub const STATS_PRINT_INTERVAL: f64 = 2.0;

    /// Number of frame timing samples to keep for averaging
    pub const FRAME_TIMING_SAMPLES: usize = 60;

    /// Number of frontend performance samples to keep
    pub const FRONTEND_PERF_SAMPLES: usize = 30;
}

/// Image compression settings
pub mod compression {
    /// JPEG quality level (0-100, higher = better quality but larger size)
    pub const JPEG_QUALITY: u8 = 85;
}
