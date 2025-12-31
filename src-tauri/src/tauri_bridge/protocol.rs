//! Custom protocol handlers for efficient data transfer
//!
//! This module implements the `frame://` custom protocol for direct binary
//! transfer of render frames, bypassing Tauri's IPC JSON serialization.

use image::{codecs::jpeg::JpegEncoder, ImageBuffer, ImageEncoder, Rgba};
use tauri::http::Response as HttpResponse;

use crate::config::{RENDER_WIDTH, RENDER_HEIGHT, compression::JPEG_QUALITY};
use super::shared_state::{SharedFrameBuffer, SharedPerfStats};

type Response = HttpResponse<Vec<u8>>;

/// Handle requests to the custom `frame://` protocol
///
/// Supported endpoints:
/// - `frame` or `frame.jpg`: JPEG-compressed frame (~50-100KB)
/// - `frame.raw`: Raw RGBA frame (~1.8MB)
/// - `stats`: Performance statistics as JSON
pub fn handle_frame_protocol(
    uri_path: &str,
    buffer: &SharedFrameBuffer,
    perf_stats: &SharedPerfStats,
) -> Response {
    let resource = uri_path.trim_start_matches('/');
    
    println!("[Protocol] Resolved resource: {}", resource);

    match resource {
        // JPEG compressed frame - much smaller data size!
        "frame" | "frame.jpg" => handle_jpeg_frame(buffer),
        
        // Raw RGBA frame (for comparison/debugging)
        "frame.raw" => handle_raw_frame(buffer),
        
        // Performance stats as JSON
        "stats" => handle_stats(perf_stats),
        
        _ => HttpResponse::builder()
            .status(404)
            .header("Content-Type", "text/plain")
            .body("Not Found".as_bytes().to_vec())
            .unwrap(),
    }
}

/// Handle JPEG-compressed frame request
fn handle_jpeg_frame(buffer: &SharedFrameBuffer) -> Response {
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

            // Encode to JPEG with quality setting
            let mut jpeg_data = Vec::new();
            let encoder = JpegEncoder::new_with_quality(&mut jpeg_data, JPEG_QUALITY);
            encoder
                .write_image(
                    rgb_img.as_raw(),
                    RENDER_WIDTH,
                    RENDER_HEIGHT,
                    image::ExtendedColorType::Rgb8,
                )
                .unwrap();

            HttpResponse::builder()
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
                .unwrap()
        }
        None => HttpResponse::builder()
            .status(503)
            .header("Content-Type", "text/plain")
            .body("Frame not ready".as_bytes().to_vec())
            .unwrap(),
    }
}

/// Handle raw RGBA frame request
fn handle_raw_frame(buffer: &SharedFrameBuffer) -> Response {
    let guard = buffer.0.lock().unwrap();
    
    match &*guard {
        Some(rgba_data) => HttpResponse::builder()
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
            .unwrap(),
        None => HttpResponse::builder()
            .status(503)
            .header("Content-Type", "text/plain")
            .body("Frame not ready".as_bytes().to_vec())
            .unwrap(),
    }
}

/// Handle performance stats request
fn handle_stats(perf_stats: &SharedPerfStats) -> Response {
    let guard = perf_stats.0.lock().unwrap();
    let json = serde_json::to_vec(&*guard).unwrap_or_default();
    
    HttpResponse::builder()
        .status(200)
        .header("Content-Type", "application/json")
        .header("Access-Control-Allow-Origin", "*")
        .body(json)
        .unwrap()
}
