//! Bridge layer between Tauri and Bevy
//!
//! This module handles all communication between the Tauri frontend and
//! the Bevy rendering backend, including command handlers, custom protocols,
//! and shared state management.

pub mod shared_state;
pub mod commands;
pub mod protocol;

// Re-export commonly used types
pub use shared_state::{
    SharedFrameBuffer, SharedMouseInput, SharedPerfStats,
};
