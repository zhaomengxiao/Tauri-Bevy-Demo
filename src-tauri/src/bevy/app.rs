//! Bevy application setup and execution
//!
//! This module handles the creation and configuration of the Bevy app,
//! including plugin registration and system scheduling.

use bevy::{
    app::{App, ScheduleRunnerPlugin},
    prelude::*,
    window::ExitCondition,
};
use std::time::Duration;
use std::thread;

use crate::config::{TARGET_FPS, PRE_ROLL_FRAMES};
use crate::tauri_bridge::shared_state::{
    SharedFrameBuffer, SharedMouseInput, SharedPerfStats,
};
use crate::bevy::plugins::ImageCopyPlugin;
use crate::bevy::resources::*;
use crate::bevy::systems::*;

/// Create and configure the Bevy application
pub fn create_app(
    frame_buffer: SharedFrameBuffer,
    perf_stats: SharedPerfStats,
    mouse_input: SharedMouseInput,
) -> App {
    let mut app = App::new();

    // Use DefaultPlugins but configure for headless operation
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(ImagePlugin::default_nearest()),
    );

    // Add schedule runner for controlled frame rate
    app.add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
        1.0 / TARGET_FPS,
    )));

    // Add custom plugins
    app.add_plugins(ImageCopyPlugin);

    // Register systems
    app.add_systems(Startup, setup_scene);
    app.add_systems(Update, rotate_cubes);
    app.add_systems(Update, update_camera_from_input);
    app.add_systems(Last, extract_and_process_frame);

    // Insert resources
    app.insert_resource(FrameBufferRes(frame_buffer));
    app.insert_resource(PerfStatsRes(perf_stats));
    app.insert_resource(MouseInputRes(mouse_input));
    app.insert_resource(OrbitCameraState::default());
    app.insert_resource(FrameCount::default());
    app.insert_resource(PreRollFrames(PRE_ROLL_FRAMES));
    app.insert_resource(FrameTimings::default());
    app.insert_resource(FrameRateLimiter::default());

    println!("[Bevy] App configured (headless mode with proper GPU-CPU pipeline)");
    app
}

/// Start Bevy in a background thread
pub fn start_bevy(
    buffer: SharedFrameBuffer,
    perf_stats: SharedPerfStats,
    mouse_input: SharedMouseInput,
) {
    thread::spawn(move || {
        println!("[Bevy] Thread started");
        let mut app = create_app(buffer, perf_stats, mouse_input);
        println!("[Bevy] Running render loop...");
        app.run();
    });
}
