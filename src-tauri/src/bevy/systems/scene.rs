//! Scene setup system
//!
//! This module handles the initial setup of the 3D scene including
//! cameras, meshes, materials, and lights.

use bevy::{
    asset::Assets,
    camera::RenderTarget,
    core_pipeline::tonemapping::Tonemapping,
    image::Image,
    math::{primitives::Cuboid, Quat, Vec3},
    pbr::{MeshMaterial3d, StandardMaterial},
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureFormat, TextureUsages},
        renderer::RenderDevice,
    },
};

use crate::config::{RENDER_WIDTH, RENDER_HEIGHT};
use crate::bevy::components::{OffscreenCamera, CameraController, RotatingCube};
use crate::bevy::plugins::image_copy::ImageCopier;
use crate::bevy::resources::RenderTargetHandle;

/// Setup the 3D scene with camera, objects, and lights
pub fn setup_scene(
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

    // Create render target texture
    let mut render_target_image =
        Image::new_target_texture(size.width, size.height, TextureFormat::bevy_default());
    render_target_image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let render_target_image_handle = images.add(render_target_image);

    commands.insert_resource(RenderTargetHandle(render_target_image_handle.clone()));

    // Spawn image copier for GPU-to-CPU transfer
    commands.spawn(ImageCopier::new(
        render_target_image_handle.clone(),
        size,
        &render_device,
    ));

    // Spawn camera with orbit controller
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

    // Primary point light
    commands.spawn((
        PointLight {
            intensity: 2_000_000.0,
            shadows_enabled: true,
            color: Color::srgb(1.0, 0.95, 0.85),
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));

    // Secondary point light (blue tint)
    commands.spawn((
        PointLight {
            intensity: 800_000.0,
            color: Color::srgb(0.4, 0.6, 1.0),
            ..default()
        },
        Transform::from_xyz(-3.0, 4.0, -2.0),
    ));

    // Directional light
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
