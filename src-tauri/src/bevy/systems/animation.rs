//! Animation systems
//!
//! This module contains systems that animate entities in the scene.

use bevy::{
    prelude::*,
    time::Time,
};

use crate::bevy::components::RotatingCube;

/// Rotate all cubes marked with RotatingCube component
pub fn rotate_cubes(time: Res<Time>, mut query: Query<&mut Transform, With<RotatingCube>>) {
    let dt = time.delta_secs();
    for mut transform in query.iter_mut() {
        transform.rotate_y(dt * 0.7);
        transform.rotate_x(dt * 0.25);
    }
}
