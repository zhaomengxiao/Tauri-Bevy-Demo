//! Bevy systems
//!
//! This module contains all the game systems that operate on entities
//! and resources in the Bevy ECS.

pub mod scene;
pub mod camera;
pub mod animation;
pub mod frame_extraction;

pub use scene::setup_scene;
pub use camera::update_camera_from_input;
pub use animation::rotate_cubes;
pub use frame_extraction::extract_and_process_frame;
