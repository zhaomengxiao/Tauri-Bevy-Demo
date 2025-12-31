//! Bevy component definitions
//!
//! This module contains all component markers and data structures used
//! to tag and identify entities in the Bevy ECS (Entity Component System).

use bevy::prelude::*;

/// Marker component for the offscreen rendering camera
///
/// Entities with this component are cameras that render to an offscreen
/// texture instead of a window.
#[derive(Component)]
pub struct OffscreenCamera;

/// Marker component for cameras that can be controlled by user input
///
/// Entities with this component will respond to mouse input for
/// orbit camera control (rotation, zoom).
#[derive(Component)]
pub struct CameraController;

/// Marker component for rotating cube objects
///
/// Entities with this component will be automatically rotated
/// by the animation system.
#[derive(Component)]
pub struct RotatingCube;
