//! Bevy Headless Offscreen Rendering Test
//!
//! This example demonstrates how to render a 3D scene without a window
//! and save the result to disk. It's derived from the official Bevy example:
//! <https://github.com/bevyengine/bevy/blob/main/examples/app/headless_renderer.rs>
//!
//! Flow:
//! 1. Render from camera to GPU image render target
//! 2. Copy from GPU image to buffer using `ImageCopyDriver` node in `RenderGraph`
//! 3. Copy from buffer to channel after `RenderSystems::Render`
//! 4. Save from channel to file in main world at `PostUpdate`
//! 5. Exit after saving the image

use bevy::{
    app::{AppExit, ScheduleRunnerPlugin},
    camera::RenderTarget,
    core_pipeline::tonemapping::Tonemapping,
    image::TextureFormatPixelInfo,
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
    window::ExitCondition,
};
use crossbeam_channel::{Receiver, Sender};
use std::{
    ops::{Deref, DerefMut},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

// =============================================================================
// Channel Communication Between Main World and Render World
// =============================================================================

/// Receives data asynchronously from the render world
#[derive(Resource, Deref)]
struct MainWorldReceiver(Receiver<Vec<u8>>);

/// Sends data asynchronously to the main world
#[derive(Resource, Deref)]
struct RenderWorldSender(Sender<Vec<u8>>);

// =============================================================================
// Configuration
// =============================================================================

struct AppConfig {
    width: u32,
    height: u32,
    single_image: bool,
}

fn main() {
    let config = AppConfig {
        width: 1920,
        height: 1080,
        single_image: true,
    };

    println!("=== Bevy Headless Offscreen Rendering Test ===");
    println!("Output size: {}x{}", config.width, config.height);
    println!("Single image mode: {}", config.single_image);

    App::new()
        .insert_resource(SceneController::new(
            config.width,
            config.height,
            config.single_image,
        ))
        .insert_resource(ClearColor(Color::srgb_u8(0, 0, 0)))
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                }),
        )
        .add_plugins(ImageCopyPlugin)
        .add_plugins(CaptureFramePlugin)
        .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .init_resource::<SceneController>()
        .add_systems(Startup, setup)
        .run();
}

// =============================================================================
// Scene State Management
// =============================================================================

#[derive(Debug, Default, Resource)]
struct SceneController {
    state: SceneState,
    name: String,
    width: u32,
    height: u32,
    single_image: bool,
}

impl SceneController {
    pub fn new(width: u32, height: u32, single_image: bool) -> SceneController {
        SceneController {
            state: SceneState::BuildScene,
            name: String::from(""),
            width,
            height,
            single_image,
        }
    }
}

#[derive(Debug, Default)]
enum SceneState {
    #[default]
    BuildScene,
    // Number of frames to wait before saving
    Render(u32),
}

// =============================================================================
// Scene Setup
// =============================================================================

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut scene_controller: ResMut<SceneController>,
    render_device: Res<RenderDevice>,
) {
    println!("[Setup] Creating scene...");

    let render_target = setup_render_target(
        &mut commands,
        &mut images,
        &render_device,
        &mut scene_controller,
        // Pre-roll frames: allow scene to fully render before capturing
        // Higher values = more stable output, but longer wait time
        40,
        "test_scene".into(),
    );

    // Ground plane (circular)
    commands.spawn((
        Mesh3d(meshes.add(Circle::new(4.0))),
        MeshMaterial3d(materials.add(Color::WHITE)),
        Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
    ));

    // Main cube (blue)
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(Color::srgb_u8(124, 144, 255))),
        Transform::from_xyz(0.0, 0.5, 0.0),
    ));

    // Point light
    commands.spawn((
        PointLight {
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));

    // Camera
    commands.spawn((
        Camera3d::default(),
        Camera {
            target: render_target,
            ..default()
        },
        Tonemapping::None,
        Transform::from_xyz(-2.5, 4.5, 9.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    println!("[Setup] Scene created successfully");
}

fn setup_render_target(
    commands: &mut Commands,
    images: &mut ResMut<Assets<Image>>,
    render_device: &Res<RenderDevice>,
    scene_controller: &mut ResMut<SceneController>,
    pre_roll_frames: u32,
    scene_name: String,
) -> RenderTarget {
    let size = Extent3d {
        width: scene_controller.width,
        height: scene_controller.height,
        ..Default::default()
    };

    // Render target texture
    let mut render_target_image =
        Image::new_target_texture(size.width, size.height, TextureFormat::bevy_default());
    render_target_image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let render_target_image_handle = images.add(render_target_image);

    // CPU-accessible image
    let cpu_image =
        Image::new_target_texture(size.width, size.height, TextureFormat::bevy_default());
    let cpu_image_handle = images.add(cpu_image);

    commands.spawn(ImageCopier::new(
        render_target_image_handle.clone(),
        size,
        render_device,
    ));

    commands.spawn(ImageToSave(cpu_image_handle));

    scene_controller.state = SceneState::Render(pre_roll_frames);
    scene_controller.name = scene_name;

    RenderTarget::Image(render_target_image_handle.into())
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
            label: None,
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
// Capture Frame Plugin (Main World)
// =============================================================================

pub struct CaptureFramePlugin;

impl Plugin for CaptureFramePlugin {
    fn build(&self, app: &mut App) {
        println!("[Plugin] CaptureFramePlugin added");
        app.add_systems(PostUpdate, update);
    }
}

#[derive(Component, Deref, DerefMut)]
struct ImageToSave(Handle<Image>);

fn update(
    images_to_save: Query<&ImageToSave>,
    receiver: Res<MainWorldReceiver>,
    mut images: ResMut<Assets<Image>>,
    mut scene_controller: ResMut<SceneController>,
    mut app_exit_writer: MessageWriter<AppExit>,
    mut file_number: Local<u32>,
) {
    if let SceneState::Render(n) = scene_controller.state {
        if n < 1 {
            let mut image_data = Vec::new();
            while let Ok(data) = receiver.try_recv() {
                image_data = data;
            }
            if !image_data.is_empty() {
                println!("[Capture] Received image data: {} bytes", image_data.len());

                for image in images_to_save.iter() {
                    let img_bytes = images.get_mut(image.id()).unwrap();

                    let row_bytes = img_bytes.width() as usize
                        * img_bytes.texture_descriptor.format.pixel_size().unwrap();
                    let aligned_row_bytes = RenderDevice::align_copy_bytes_per_row(row_bytes);

                    if row_bytes == aligned_row_bytes {
                        img_bytes.data.as_mut().unwrap().clone_from(&image_data);
                    } else {
                        img_bytes.data = Some(
                            image_data
                                .chunks(aligned_row_bytes)
                                .take(img_bytes.height() as usize)
                                .flat_map(|row| &row[..row_bytes.min(row.len())])
                                .cloned()
                                .collect(),
                        );
                    }

                    let img = match img_bytes.clone().try_into_dynamic() {
                        Ok(img) => img.to_rgba8(),
                        Err(e) => panic!("Failed to create image buffer {e:?}"),
                    };

                    let images_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_images");
                    println!("[Capture] Saving to: {:?}", images_dir);
                    std::fs::create_dir_all(&images_dir).unwrap();

                    let image_path = images_dir.join(format!("{:03}.png", file_number.deref()));
                    *file_number.deref_mut() += 1;

                    if let Err(e) = img.save(&image_path) {
                        panic!("Failed to save image: {e}");
                    }

                    println!("[Capture] ✅ Image saved successfully: {:?}", image_path);
                }

                if scene_controller.single_image {
                    println!("[App] Single image mode - exiting");
                    app_exit_writer.write(AppExit::Success);
                }
            }
        } else {
            while receiver.try_recv().is_ok() {}
            scene_controller.state = SceneState::Render(n - 1);
            if n % 10 == 0 {
                println!("[Render] Pre-roll frames remaining: {}", n);
            }
        }
    }
}
