//! Frame extraction system
//!
//! This module handles extracting rendered frames from the GPU and
//! preparing them for transfer to the Tauri frontend.

use bevy::{prelude::*, render::renderer::RenderDevice, time::Time};

use crate::bevy::resources::{
    FrameBufferRes, FrameCount, FrameRateLimiter, FrameTimings, MainWorldReceiver, PerfStatsRes,
    PreRollFrames,
};
use crate::config::{performance::*, RENDER_HEIGHT, RENDER_WIDTH};

/// Extract and process frame data from the render pipeline
pub fn extract_and_process_frame(
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

                // Keep only last N samples for averaging
                if timings.frame_times.len() > FRAME_TIMING_SAMPLES {
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

                // Print detailed stats periodically
                let current_time = time.elapsed_secs_f64();
                if current_time - timings.last_print_time >= STATS_PRINT_INTERVAL {
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
