//! Tauri-Bevy Demo: Headless Offscreen Rendering
//!
//! This module demonstrates Bevy's headless 3D rendering integrated with Tauri.
//! Based on official Bevy headless renderer example with proper GPU-CPU data transfer.
//!
//! Architecture:
//! - Bevy runs in a background thread with NO window (true headless mode)
//! - Uses proper RenderGraph pipeline with ImageCopyDriver node
//! - GPU texture -> Buffer -> CPU channel -> Tauri frontend
//! - Frame data transferred as Base64-encoded RGBA (avoiding JSON array serialization)

use base64::{engine::general_purpose::STANDARD, Engine};
use bevy::{
    app::{App, ScheduleRunnerPlugin},
    asset::Assets,
    camera::RenderTarget,
    core_pipeline::tonemapping::Tonemapping,
    image::Image,
    math::{primitives::Cuboid, Quat, Vec3},
    pbr::{MeshMaterial3d, StandardMaterial},
    prelude::*,
    render::{
        render_asset::RenderAssets,
        render_graph::{self, NodeRunError, RenderGraph, RenderGraphContext, RenderLabel},
        render_resource::{
            Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor, Extent3d, MapMode,
            PollType, TexelCopyBufferInfo, TexelCopyBufferLayout, TextureFormat, TextureUsages,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue},
        Extract, Render, RenderApp, RenderSystems,
    },
    time::Time,
    window::ExitCondition,
};
use crossbeam_channel::{Receiver, Sender};
use image::{codecs::jpeg::JpegEncoder, ImageBuffer, ImageEncoder, Rgba};
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};
use tauri::{http::Response as HttpResponse, State};

// =============================================================================
// Configuration
// =============================================================================

const RENDER_WIDTH: u32 = 800;
const RENDER_HEIGHT: u32 = 600;

// =============================================================================
// Shared State for Tauri
// =============================================================================

/// Thread-safe RGBA frame buffer shared between Bevy and Tauri
/// Stores raw RGBA8 pixel data (4 bytes per pixel)
#[derive(Clone, Default)]
pub struct SharedFrameBuffer(Arc<Mutex<Option<Vec<u8>>>>);

/// Frame response containing Base64-encoded RGBA pixel data
#[derive(Serialize, Deserialize)]
pub struct FrameResponse {
    /// Base64-encoded RGBA pixel data (avoids slow JSON array serialization)
    pub data: String,
    pub width: u32,
    pub height: u32,
}

/// Performance statistics for debugging
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

// =============================================================================
// Mouse Input for Camera Control
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
pub struct SharedMouseInput(Arc<Mutex<MouseInput>>);

// =============================================================================
// Channel Communication (Main World <-> Render World)
// =============================================================================

/// Receives data from render world
#[derive(Resource, Deref)]
struct MainWorldReceiver(Receiver<Vec<u8>>);

/// Sends data to main world
#[derive(Resource, Deref)]
struct RenderWorldSender(Sender<Vec<u8>>);

// =============================================================================
// Bevy Components & Resources
// =============================================================================

#[derive(Component)]
struct OffscreenCamera;

/// Marker component for camera that can be controlled by mouse input
#[derive(Component)]
struct CameraController;

#[derive(Component)]
struct RotatingCube;

/// Orbit camera state for spherical coordinate camera control
#[derive(Resource)]
struct OrbitCameraState {
    /// Horizontal rotation angle (radians)
    yaw: f32,
    /// Vertical rotation angle (radians), clamped to avoid gimbal lock
    pitch: f32,
    /// Distance from the camera to the center point
    distance: f32,
    /// The point the camera orbits around
    center: Vec3,
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
struct MouseInputRes(SharedMouseInput);

#[derive(Resource)]
struct RenderTargetHandle(Handle<Image>);

#[derive(Resource, Clone)]
struct FrameBufferRes(SharedFrameBuffer);

#[derive(Resource, Default)]
struct FrameCount(u32);

#[derive(Resource, Default)]
struct PreRollFrames(u32);

/// Shared performance statistics
#[derive(Clone, Default)]
pub struct SharedPerfStats(Arc<Mutex<PerformanceStats>>);

#[derive(Resource)]
struct PerfStatsRes(SharedPerfStats);

/// Performance timing tracker
#[derive(Resource, Default)]
struct FrameTimings {
    last_print_time: f64,
    frame_times: Vec<f64>,
}

/// Frame rate limiter to control output FPS
#[derive(Resource)]
struct FrameRateLimiter {
    last_frame_time: std::time::Instant,
    min_frame_interval: Duration,
}

impl FrameRateLimiter {
    fn new(target_fps: f64) -> Self {
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
// Image Copy Plugin (Render World)
// =============================================================================

pub struct ImageCopyPlugin;

impl Plugin for ImageCopyPlugin {
    fn build(&self, app: &mut App) {
        let (s, r) = crossbeam_channel::unbounded();

        let render_app = app
            .insert_resource(MainWorldReceiver(r))
            .sub_app_mut(RenderApp);

        let mut graph = render_app.world_mut().resource_mut::<RenderGraph>();
        graph.add_node(ImageCopy, ImageCopyDriver);
        graph.add_node_edge(bevy::render::graph::CameraDriverLabel, ImageCopy);

        render_app
            .insert_resource(RenderWorldSender(s))
            .add_systems(ExtractSchedule, image_copy_extract)
            .add_systems(
                Render,
                receive_image_from_buffer.after(RenderSystems::Render),
            );
    }
}

#[derive(Clone, Default, Resource, Deref, DerefMut)]
struct ImageCopiers(pub Vec<ImageCopier>);

#[derive(Clone, Component)]
struct ImageCopier {
    buffer: Buffer,
    enabled: Arc<AtomicBool>,
    src_image: Handle<Image>,
}

impl ImageCopier {
    pub fn new(
        src_image: Handle<Image>,
        size: Extent3d,
        render_device: &RenderDevice,
    ) -> ImageCopier {
        let padded_bytes_per_row =
            RenderDevice::align_copy_bytes_per_row((size.width) as usize) * 4;

        let cpu_buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("image_copy_buffer"),
            size: padded_bytes_per_row as u64 * size.height as u64,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        ImageCopier {
            buffer: cpu_buffer,
            src_image,
            enabled: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
}

fn image_copy_extract(mut commands: Commands, image_copiers: Extract<Query<&ImageCopier>>) {
    commands.insert_resource(ImageCopiers(
        image_copiers.iter().cloned().collect::<Vec<ImageCopier>>(),
    ));
}

#[derive(Debug, PartialEq, Eq, Clone, Hash, RenderLabel)]
struct ImageCopy;

#[derive(Default)]
struct ImageCopyDriver;

impl render_graph::Node for ImageCopyDriver {
    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let image_copiers = world.get_resource::<ImageCopiers>().unwrap();
        let gpu_images = world
            .get_resource::<RenderAssets<bevy::render::texture::GpuImage>>()
            .unwrap();

        for image_copier in image_copiers.iter() {
            if !image_copier.enabled() {
                continue;
            }

            let src_image = gpu_images.get(&image_copier.src_image).unwrap();

            let mut encoder = render_context
                .render_device()
                .create_command_encoder(&CommandEncoderDescriptor::default());

            let block_dimensions = src_image.texture_format.block_dimensions();
            let block_size = src_image.texture_format.block_copy_size(None).unwrap();

            let padded_bytes_per_row = RenderDevice::align_copy_bytes_per_row(
                (src_image.size.width as usize / block_dimensions.0 as usize) * block_size as usize,
            );

            encoder.copy_texture_to_buffer(
                src_image.texture.as_image_copy(),
                TexelCopyBufferInfo {
                    buffer: &image_copier.buffer,
                    layout: TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(
                            std::num::NonZero::<u32>::new(padded_bytes_per_row as u32)
                                .unwrap()
                                .into(),
                        ),
                        rows_per_image: None,
                    },
                },
                src_image.size,
            );

            let render_queue = world.get_resource::<RenderQueue>().unwrap();
            render_queue.submit(std::iter::once(encoder.finish()));
        }

        Ok(())
    }
}

fn receive_image_from_buffer(
    image_copiers: Res<ImageCopiers>,
    render_device: Res<RenderDevice>,
    sender: Res<RenderWorldSender>,
) {
    for image_copier in image_copiers.0.iter() {
        if !image_copier.enabled() {
            continue;
        }

        let buffer_slice = image_copier.buffer.slice(..);

        let (s, r) = crossbeam_channel::bounded(1);

        buffer_slice.map_async(MapMode::Read, move |r| match r {
            Ok(r) => s.send(r).expect("Failed to send map update"),
            Err(err) => panic!("Failed to map buffer {err}"),
        });

        render_device
            .poll(PollType::wait())
            .expect("Failed to poll device for map async");

        r.recv().expect("Failed to receive the map_async message");

        let _ = sender.send(buffer_slice.get_mapped_range().to_vec());

        image_copier.buffer.unmap();
    }
}

// =============================================================================
// Bevy Systems
// =============================================================================

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    render_device: Res<RenderDevice>,
) {
    println!("[Bevy] Setting up scene...");

    let size = Extent3d {
        width: RENDER_WIDTH,
        height: RENDER_HEIGHT,
        depth_or_array_layers: 1,
    };

    // Render target texture
    let mut render_target_image =
        Image::new_target_texture(size.width, size.height, TextureFormat::bevy_default());
    render_target_image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let render_target_image_handle = images.add(render_target_image);

    commands.insert_resource(RenderTargetHandle(render_target_image_handle.clone()));

    commands.spawn(ImageCopier::new(
        render_target_image_handle.clone(),
        size,
        &render_device,
    ));

    // Camera with orbit controller
    // Initial position will be set by update_camera_from_input based on OrbitCameraState
    commands.spawn((
        Camera3d::default(),
        Camera {
            target: RenderTarget::Image(render_target_image_handle.into()),
            clear_color: ClearColorConfig::Custom(Color::srgb(0.05, 0.08, 0.12)),
            ..default()
        },
        Tonemapping::None,
        Transform::from_xyz(0.0, 2.5, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
        OffscreenCamera,
        CameraController,
    ));

    // Main cube (blue)
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.5, 1.5, 1.5))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.4, 0.9),
            metallic: 0.6,
            perceptual_roughness: 0.3,
            ..default()
        })),
        Transform::from_xyz(0.0, 0.0, 0.0),
        RotatingCube,
    ));

    // Small cube (red)
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.6, 0.6, 0.6))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.9, 0.2, 0.3),
            metallic: 0.8,
            perceptual_roughness: 0.2,
            ..default()
        })),
        Transform::from_xyz(2.2, 0.3, 0.0),
        RotatingCube,
    ));

    // Lights
    commands.spawn((
        PointLight {
            intensity: 2_000_000.0,
            shadows_enabled: true,
            color: Color::srgb(1.0, 0.95, 0.85),
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));

    commands.spawn((
        PointLight {
            intensity: 800_000.0,
            color: Color::srgb(0.4, 0.6, 1.0),
            ..default()
        },
        Transform::from_xyz(-3.0, 4.0, -2.0),
    ));

    commands.spawn((
        DirectionalLight {
            illuminance: 3000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(bevy::math::EulerRot::XYZ, -0.6, 0.4, 0.0)),
    ));

    println!("[Bevy] Scene setup complete!");
}

fn rotate_cubes(time: Res<Time>, mut query: Query<&mut Transform, With<RotatingCube>>) {
    let dt = time.delta_secs();
    for mut transform in query.iter_mut() {
        transform.rotate_y(dt * 0.7);
        transform.rotate_x(dt * 0.25);
    }
}

/// Update camera transform based on mouse input
/// Implements orbit camera control:
/// - Left button drag: rotate camera (yaw/pitch)
/// - Scroll wheel: zoom (adjust distance)
fn update_camera_from_input(
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

    // Rotation sensitivity
    const ROTATION_SPEED: f32 = 0.005;
    // Zoom sensitivity
    const ZOOM_SPEED: f32 = 0.5;
    // Min/max distance limits
    const MIN_DISTANCE: f32 = 2.0;
    const MAX_DISTANCE: f32 = 20.0;
    // Pitch limits to prevent flipping (slightly less than 90 degrees)
    const MAX_PITCH: f32 = 1.5;
    const MIN_PITCH: f32 = -1.5;

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

fn extract_and_process_frame(
    receiver: Res<MainWorldReceiver>,
    buffer: Option<Res<FrameBufferRes>>,
    perf_stats: Option<Res<PerfStatsRes>>,
    mut count: ResMut<FrameCount>,
    mut pre_roll: ResMut<PreRollFrames>,
    mut timings: ResMut<FrameTimings>,
    mut frame_limiter: ResMut<FrameRateLimiter>,
    time: Res<Time>,
) {
    let Some(b) = buffer else { return };

    // Wait for scene to be fully rendered
    if pre_roll.0 > 0 {
        while receiver.try_recv().is_ok() {}
        pre_roll.0 -= 1;
        if pre_roll.0 % 10 == 0 && pre_roll.0 > 0 {
            println!("[Bevy] Pre-roll frames remaining: {}", pre_roll.0);
        }
        return;
    }

    // Frame rate limiting - skip if not enough time has passed
    let now = std::time::Instant::now();
    let elapsed = now.duration_since(frame_limiter.last_frame_time);
    if elapsed < frame_limiter.min_frame_interval {
        // Drain the receiver but don't process - too early for next frame
        while receiver.try_recv().is_ok() {}
        return;
    }
    frame_limiter.last_frame_time = now;

    let frame_start = std::time::Instant::now();

    // Try to receive latest frame data from render world
    // This data comes from GPU->CPU transfer (already completed in render world)
    let receive_start = std::time::Instant::now();
    let mut image_data = Vec::new();
    while let Ok(data) = receiver.try_recv() {
        image_data = data;
    }
    let receive_time = receive_start.elapsed().as_secs_f64() * 1000.0;

    if !image_data.is_empty() {
        // Remove row padding and store raw RGBA data
        let process_start = std::time::Instant::now();
        if let Some(rgba) = remove_row_padding(&image_data, RENDER_WIDTH, RENDER_HEIGHT) {
            let process_time = process_start.elapsed().as_secs_f64() * 1000.0;
            let data_size = rgba.len();

            if let Ok(mut guard) = b.0 .0.lock() {
                *guard = Some(rgba);
                count.0 += 1;

                let total_time = frame_start.elapsed().as_secs_f64() * 1000.0;
                timings.frame_times.push(total_time);

                // Keep only last 60 samples for averaging
                if timings.frame_times.len() > 60 {
                    timings.frame_times.remove(0);
                }

                // Update performance stats
                if let Some(perf_res) = &perf_stats {
                    if let Ok(mut stats) = perf_res.0 .0.lock() {
                        stats.gpu_transfer_ms = receive_time;
                        stats.data_processing_ms = process_time;
                        stats.frame_encoding_ms = total_time;
                        stats.frame_count = count.0;
                        stats.data_size_kb = data_size as f64 / 1024.0;

                        // Calculate FPS from frame times
                        if !timings.frame_times.is_empty() {
                            let avg_time = timings.frame_times.iter().sum::<f64>()
                                / timings.frame_times.len() as f64;
                            stats.bevy_fps = if avg_time > 0.0 {
                                1000.0 / avg_time
                            } else {
                                0.0
                            };
                        }
                    }
                }

                // Print detailed stats every 2 seconds
                let current_time = time.elapsed_secs_f64();
                if current_time - timings.last_print_time >= 2.0 {
                    let avg_time =
                        timings.frame_times.iter().sum::<f64>() / timings.frame_times.len() as f64;
                    let max_time = timings.frame_times.iter().cloned().fold(0.0f64, f64::max);
                    let min_time = timings.frame_times.iter().cloned().fold(f64::MAX, f64::min);

                    println!(
                        "[Bevy] Frame {} | Receive: {:.2}ms | Process: {:.2}ms | Total: {:.2}ms | Avg: {:.2}ms (Min: {:.2}ms, Max: {:.2}ms) | Size: {:.1}KB",
                        count.0,
                        receive_time,
                        process_time,
                        total_time,
                        avg_time,
                        min_time,
                        max_time,
                        data_size as f64 / 1024.0
                    );
                    timings.last_print_time = current_time;
                }
            }
        }
    }
}

/// Remove GPU buffer row padding alignment, returning pure RGBA data
fn remove_row_padding(data: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    if data.is_empty() {
        return None;
    }

    // Handle row padding alignment
    let row_bytes = width as usize * 4;
    let aligned_row_bytes = RenderDevice::align_copy_bytes_per_row(row_bytes);

    let rgba_data = if row_bytes == aligned_row_bytes {
        // No padding, return as-is
        data.to_vec()
    } else {
        // Remove padding from each row
        data.chunks(aligned_row_bytes)
            .take(height as usize)
            .flat_map(|row| &row[..row_bytes.min(row.len())])
            .cloned()
            .collect()
    };

    Some(rgba_data)
}

// =============================================================================
// Bevy App Setup
// =============================================================================

fn create_app(
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

    // Add schedule runner for controlled frame rate (60 FPS to match frontend)
    app.add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
        1.0 / 60.0,
    )));

    // Image copy plugin (GPU -> CPU transfer)
    app.add_plugins(ImageCopyPlugin);

    // Our systems
    app.add_systems(Startup, setup_scene);
    app.add_systems(Update, rotate_cubes);
    app.add_systems(Update, update_camera_from_input);
    app.add_systems(Last, extract_and_process_frame);

    // Resources
    app.insert_resource(FrameBufferRes(frame_buffer));
    app.insert_resource(PerfStatsRes(perf_stats));
    app.insert_resource(MouseInputRes(mouse_input));
    app.insert_resource(OrbitCameraState::default());
    app.insert_resource(FrameCount::default());
    app.insert_resource(PreRollFrames(30)); // Wait 30 frames (~0.5s at 60 FPS) for scene to stabilize
    app.insert_resource(FrameTimings::default());
    app.insert_resource(FrameRateLimiter::default()); // Limit output to 60 FPS

    println!("[Bevy] App configured (headless mode with proper GPU-CPU pipeline)");
    app
}

fn start_bevy(
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

// =============================================================================
// Tauri Commands
// =============================================================================

#[tauri::command]
fn get_frame(
    state: State<SharedFrameBuffer>,
    perf_state: State<SharedPerfStats>,
) -> Result<FrameResponse, String> {
    let cmd_start = std::time::Instant::now();

    let guard = state.0.lock().map_err(|e| e.to_string())?;
    let result = match &*guard {
        Some(rgba_data) => {
            let data_fetch_time = cmd_start.elapsed().as_secs_f64() * 1000.0;

            // Measure Base64 encoding time
            let encode_start = std::time::Instant::now();
            let base64_data = STANDARD.encode(rgba_data);
            let encode_time = encode_start.elapsed().as_secs_f64() * 1000.0;

            let total_time = cmd_start.elapsed().as_secs_f64() * 1000.0;

            // Update perf stats
            if let Ok(mut stats) = perf_state.0.lock() {
                stats.tauri_get_frame_ms = data_fetch_time;
                stats.tauri_serialize_ms = encode_time;
            }

            Ok(FrameResponse {
                data: base64_data,
                width: RENDER_WIDTH,
                height: RENDER_HEIGHT,
            })
        }
        None => Err("No frame yet (scene still loading)".into()),
    };

    result
}

#[tauri::command]
fn get_render_size() -> (u32, u32) {
    (RENDER_WIDTH, RENDER_HEIGHT)
}

#[tauri::command]
fn get_performance_stats(state: State<SharedPerfStats>) -> Result<PerformanceStats, String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    Ok(guard.clone())
}

/// Receive mouse input from frontend for camera control
/// Input deltas are accumulated until consumed by Bevy
#[tauri::command]
fn send_mouse_input(
    state: State<SharedMouseInput>,
    delta_x: f32,
    delta_y: f32,
    scroll_delta: f32,
    left_button: bool,
    right_button: bool,
) -> Result<(), String> {
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    // Accumulate deltas (will be cleared when Bevy reads them)
    guard.delta_x += delta_x;
    guard.delta_y += delta_y;
    guard.scroll_delta += scroll_delta;
    // Button state is just the current state
    guard.left_button = left_button;
    guard.right_button = right_button;
    Ok(())
}

// =============================================================================
// Entry Point
// =============================================================================

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    println!("[Tauri] Starting...");

    let buffer = SharedFrameBuffer::default();
    let perf_stats = SharedPerfStats::default();
    let mouse_input = SharedMouseInput::default();
    start_bevy(buffer.clone(), perf_stats.clone(), mouse_input.clone());

    // Wait for Bevy to initialize
    thread::sleep(Duration::from_millis(1000));

    // Clone for the custom protocol handler
    let protocol_buffer = buffer.clone();
    let protocol_perf_stats = perf_stats.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(buffer)
        .manage(perf_stats)
        .manage(mouse_input)
        // Register custom protocol "frame://" for direct binary transfer
        // This bypasses Tauri IPC JSON serialization completely!
        .register_asynchronous_uri_scheme_protocol("frame", move |_ctx, request, responder| {
            let buffer = protocol_buffer.clone();
            let perf_stats = protocol_perf_stats.clone();

            // Handle the request in a separate thread to avoid blocking
            std::thread::spawn(move || {
                let uri = request.uri();
                let uri_str = uri.to_string();
                let path = uri.path();
                let host = uri.host().unwrap_or("");

                // Debug logging
                println!(
                    "[Protocol] Request URI: {}, host: {}, path: {}",
                    uri_str, host, path
                );

                // For Tauri v2, URL format is: http://frame.localhost/path
                // So we extract the resource from the path
                let resource = path.trim_start_matches('/');

                println!("[Protocol] Resolved resource: {}", resource);

                match resource {
                    // JPEG compressed frame - much smaller data size!
                    "frame" | "frame.jpg" => {
                        let guard = buffer.0.lock().unwrap();
                        match &*guard {
                            Some(rgba_data) => {
                                // Compress RGBA to JPEG - reduces ~1.8MB to ~50-100KB!
                                let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(
                                    RENDER_WIDTH,
                                    RENDER_HEIGHT,
                                    rgba_data.clone(),
                                )
                                .unwrap();

                                // Convert RGBA to RGB for JPEG (no alpha channel)
                                let rgb_img = image::DynamicImage::ImageRgba8(img).to_rgb8();

                                // Encode to JPEG with quality 85 (good balance of size vs quality)
                                let mut jpeg_data = Vec::new();
                                let encoder = JpegEncoder::new_with_quality(&mut jpeg_data, 85);
                                encoder
                                    .write_image(
                                        rgb_img.as_raw(),
                                        RENDER_WIDTH,
                                        RENDER_HEIGHT,
                                        image::ExtendedColorType::Rgb8,
                                    )
                                    .unwrap();

                                let response = HttpResponse::builder()
                                    .status(200)
                                    .header("Content-Type", "image/jpeg")
                                    .header("X-Frame-Width", RENDER_WIDTH.to_string())
                                    .header("X-Frame-Height", RENDER_HEIGHT.to_string())
                                    .header("Access-Control-Allow-Origin", "*")
                                    .header(
                                        "Access-Control-Expose-Headers",
                                        "X-Frame-Width, X-Frame-Height",
                                    )
                                    .body(jpeg_data)
                                    .unwrap();
                                responder.respond(response);
                            }
                            None => {
                                let response = HttpResponse::builder()
                                    .status(503)
                                    .header("Content-Type", "text/plain")
                                    .body("Frame not ready".as_bytes().to_vec())
                                    .unwrap();
                                responder.respond(response);
                            }
                        }
                    }
                    // Raw RGBA frame (for comparison/debugging)
                    "frame.raw" => {
                        let guard = buffer.0.lock().unwrap();
                        match &*guard {
                            Some(rgba_data) => {
                                let response = HttpResponse::builder()
                                    .status(200)
                                    .header("Content-Type", "application/octet-stream")
                                    .header("X-Frame-Width", RENDER_WIDTH.to_string())
                                    .header("X-Frame-Height", RENDER_HEIGHT.to_string())
                                    .header("Access-Control-Allow-Origin", "*")
                                    .header(
                                        "Access-Control-Expose-Headers",
                                        "X-Frame-Width, X-Frame-Height",
                                    )
                                    .body(rgba_data.clone())
                                    .unwrap();
                                responder.respond(response);
                            }
                            None => {
                                let response = HttpResponse::builder()
                                    .status(503)
                                    .header("Content-Type", "text/plain")
                                    .body("Frame not ready".as_bytes().to_vec())
                                    .unwrap();
                                responder.respond(response);
                            }
                        }
                    }
                    "stats" => {
                        // Return performance stats as JSON (small data, OK to use JSON)
                        let guard = perf_stats.0.lock().unwrap();
                        let json = serde_json::to_vec(&*guard).unwrap_or_default();
                        let response = HttpResponse::builder()
                            .status(200)
                            .header("Content-Type", "application/json")
                            .header("Access-Control-Allow-Origin", "*")
                            .body(json)
                            .unwrap();
                        responder.respond(response);
                    }
                    _ => {
                        let response = HttpResponse::builder()
                            .status(404)
                            .header("Content-Type", "text/plain")
                            .body("Not Found".as_bytes().to_vec())
                            .unwrap();
                        responder.respond(response);
                    }
                }
            });
        })
        .invoke_handler(tauri::generate_handler![
            get_frame,
            get_render_size,
            get_performance_stats,
            send_mouse_input
        ])
        .run(tauri::generate_context!())
        .expect("Tauri error");
}
