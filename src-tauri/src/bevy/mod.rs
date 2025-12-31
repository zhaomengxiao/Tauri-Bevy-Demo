//! Bevy engine integration
//!
//! This module contains all Bevy-related code including components,
//! resources, systems, plugins, and application setup.

pub mod components;
pub mod resources;
pub mod plugins;
pub mod systems;
pub mod app;

// Re-export commonly used items
pub use app::start_bevy;
