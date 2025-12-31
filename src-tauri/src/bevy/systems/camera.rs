//! Camera control system
//!
//! This module implements orbit camera controls that respond to mouse input
//! from the frontend, allowing users to rotate and zoom the camera.

use bevy::{
    math::Vec3,
    prelude::*,
};

use crate::config::camera::*;
use crate::bevy::components::CameraController;
use crate::bevy::resources::{MouseInputRes, OrbitCameraState};

/// Update camera transform based on mouse input
/// Implements orbit camera control:
/// - Left button drag: rotate camera (yaw/pitch)
/// - Scroll wheel: zoom (adjust distance)
pub fn update_camera_from_input(
    mouse_input_res: Option<Res<MouseInputRes>>,
    mut orbit_state: ResMut<OrbitCameraState>,
    mut camera_query: Query<&mut Transform, With<CameraController>>,
) {
    let Some(mouse_res) = mouse_input_res else {
        return;
    };

    // Read and clear accumulated input
    let input = {
        let mut guard = match mouse_res.0 .0.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let input = guard.clone();
        // Clear accumulated deltas after reading
        guard.delta_x = 0.0;
        guard.delta_y = 0.0;
        guard.scroll_delta = 0.0;
        input
    };

    // Apply rotation when left button is held
    if input.left_button && (input.delta_x != 0.0 || input.delta_y != 0.0) {
        orbit_state.yaw -= input.delta_x * ROTATION_SPEED;
        orbit_state.pitch -= input.delta_y * ROTATION_SPEED;

        // Clamp pitch to prevent camera flipping
        orbit_state.pitch = orbit_state.pitch.clamp(MIN_PITCH, MAX_PITCH);
    }

    // Apply zoom from scroll wheel
    if input.scroll_delta != 0.0 {
        orbit_state.distance -= input.scroll_delta * ZOOM_SPEED;
        orbit_state.distance = orbit_state.distance.clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    // Update camera transform based on orbit state
    for mut transform in camera_query.iter_mut() {
        // Calculate camera position using spherical coordinates
        // yaw: rotation around Y axis
        // pitch: rotation around X axis (elevation)
        let x = orbit_state.distance * orbit_state.pitch.cos() * orbit_state.yaw.sin();
        let y = orbit_state.distance * orbit_state.pitch.sin();
        let z = orbit_state.distance * orbit_state.pitch.cos() * orbit_state.yaw.cos();

        let camera_position = orbit_state.center + Vec3::new(x, y, z);
        *transform =
            Transform::from_translation(camera_position).looking_at(orbit_state.center, Vec3::Y);
    }
}
