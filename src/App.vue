<script setup lang="ts">
/**
 * Tauri-Bevy Demo Frontend
 *
 * This Vue component displays frames rendered by Bevy on an HTML Canvas.
 * It demonstrates the integration between:
 * - Bevy (Rust game engine) running in a separate thread
 * - Tauri (Rust desktop framework) providing the bridge
 * - Vue (JavaScript UI framework) handling the frontend
 *
 * Key concepts:
 * - Canvas API: HTML5's drawing surface, similar to a Windows Device Context
 * - requestAnimationFrame: Browser's frame sync API for smooth animations
 * - async/await: JavaScript's way of handling asynchronous operations
 */
import { ref, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";

// =============================================================================
// Types
// =============================================================================

// FrameResponse no longer needed - using custom protocol with direct binary transfer

/** Performance statistics from Rust backend */
interface PerformanceStats {
  gpu_transfer_ms: number;
  data_processing_ms: number;
  frame_encoding_ms: number;
  bevy_fps: number;
  frame_count: number;
  data_size_kb: number;
  tauri_get_frame_ms: number;
  tauri_serialize_ms: number;
}

/** Frontend performance metrics */
interface FrontendPerf {
  tauri_call_ms: number;
  image_create_ms: number;
  canvas_draw_ms: number;
  total_loop_ms: number;
  frontend_fps: number;
}

// =============================================================================
// Reactive State
// =============================================================================

/** Reference to the canvas DOM element */
const canvasRef = ref<HTMLCanvasElement | null>(null);
/** Whether the render loop is currently running */
const isRendering = ref(false);
/** Current frames per second */
const fps = ref(0);
/** Total number of frames rendered */
const frameCount = ref(0);
/** Status message for user feedback */
const statusMessage = ref("Click 'Start' to begin rendering");
/** Error message if something goes wrong */
const errorMessage = ref("");
/** Last error timestamp for debouncing */
let lastErrorTime = 0;

// Performance statistics
const backendStats = ref<PerformanceStats>({
  gpu_transfer_ms: 0,
  data_processing_ms: 0,
  frame_encoding_ms: 0,
  bevy_fps: 0,
  frame_count: 0,
  data_size_kb: 0,
  tauri_get_frame_ms: 0,
  tauri_serialize_ms: 0,
});

const frontendStats = ref<FrontendPerf>({
  tauri_call_ms: 0,
  image_create_ms: 0,
  canvas_draw_ms: 0,
  total_loop_ms: 0,
  frontend_fps: 0,
});

// Animation frame ID for cleanup
let animationId: number | null = null;
// For FPS calculation
let fpsFrameCount = 0;
let fpsLastUpdate = performance.now();
// For frontend performance averaging
let frontendPerfSamples: FrontendPerf[] = [];

// =============================================================================
// Mouse Input State for Camera Control
// =============================================================================

/** Track mouse button states */
const mouseState = {
  leftButton: false,
  rightButton: false,
  lastX: 0,
  lastY: 0,
};

/**
 * Send mouse input to Bevy for camera control
 * Uses accumulated deltas to ensure smooth movement even at different frame rates
 */
async function sendMouseInput(
  deltaX: number,
  deltaY: number,
  scrollDelta: number,
  leftButton: boolean,
  rightButton: boolean
) {
  try {
    await invoke("send_mouse_input", {
      deltaX,
      deltaY,
      scrollDelta,
      leftButton,
      rightButton,
    });
  } catch (error) {
    // Silently ignore errors to avoid spamming console during rapid input
  }
}

/**
 * Handle mouse down events on canvas
 */
function handleMouseDown(event: MouseEvent) {
  if (event.button === 0) {
    mouseState.leftButton = true;
  } else if (event.button === 2) {
    mouseState.rightButton = true;
  }
  mouseState.lastX = event.clientX;
  mouseState.lastY = event.clientY;

  // Prevent default context menu on right click
  event.preventDefault();
}

/**
 * Handle mouse up events (on window to catch releases outside canvas)
 */
function handleMouseUp(event: MouseEvent) {
  if (event.button === 0) {
    mouseState.leftButton = false;
    // Send final state update when button is released
    sendMouseInput(0, 0, 0, false, mouseState.rightButton);
  } else if (event.button === 2) {
    mouseState.rightButton = false;
    sendMouseInput(0, 0, 0, mouseState.leftButton, false);
  }
}

/**
 * Handle mouse move events on canvas
 * Only sends input when a button is pressed (drag behavior)
 */
function handleMouseMove(event: MouseEvent) {
  if (!mouseState.leftButton && !mouseState.rightButton) {
    return;
  }

  const deltaX = event.clientX - mouseState.lastX;
  const deltaY = event.clientY - mouseState.lastY;
  mouseState.lastX = event.clientX;
  mouseState.lastY = event.clientY;

  // Send mouse movement delta to Bevy
  sendMouseInput(
    deltaX,
    deltaY,
    0,
    mouseState.leftButton,
    mouseState.rightButton
  );
}

/**
 * Handle mouse wheel events for zooming
 */
function handleWheel(event: WheelEvent) {
  // Prevent page scroll
  event.preventDefault();

  // Normalize scroll delta (different browsers report different values)
  // deltaY is positive when scrolling down (zoom out), negative when up (zoom in)
  const scrollDelta = -event.deltaY * 0.01;

  sendMouseInput(0, 0, scrollDelta, mouseState.leftButton, mouseState.rightButton);
}

/**
 * Prevent context menu on right click
 */
function handleContextMenu(event: MouseEvent) {
  event.preventDefault();
}

// =============================================================================
// Render Loop
// =============================================================================

/**
 * Main render loop that fetches frames from Bevy and draws them to canvas.
 *
 * OPTIMIZED: Uses custom protocol "frame://" for direct binary transfer
 * This completely bypasses Tauri IPC JSON serialization!
 *
 * How it works:
 * 1. fetch('frame://localhost/frame') returns raw RGBA ArrayBuffer
 * 2. Create ImageData directly from ArrayBuffer (no Base64 decode!)
 * 3. Draw to canvas
 * 4. Schedule next frame with requestAnimationFrame
 */
async function renderLoop() {
  if (!isRendering.value) return;

  const loopStart = performance.now();

  const canvas = canvasRef.value;
  if (!canvas) {
    console.error("Canvas not available");
    return;
  }

  const ctx = canvas.getContext("2d");
  if (!ctx) {
    console.error("Could not get 2D context");
    return;
  }

  try {
    // OPTIMIZED: Use custom protocol with JPEG compression
    // Data size reduced from ~1.8MB to ~50-100KB!
    // Tauri v2 custom protocol URL format: http://<scheme>.localhost/<path>
    const fetchStart = performance.now();
    const response = await fetch("http://frame.localhost/frame");
    
    if (!response.ok) {
      throw new Error(`Frame fetch failed: ${response.status}`);
    }
    
    // Get JPEG blob directly
    const blob = await response.blob();
    const fetchTime = performance.now() - fetchStart;

    // Use createImageBitmap for hardware-accelerated JPEG decoding
    // This is MUCH faster than manual pixel manipulation!
    const imageStart = performance.now();
    const imageBitmap = await createImageBitmap(blob);
    const imageTime = performance.now() - imageStart;

    // Draw ImageBitmap to canvas (hardware accelerated)
    const drawStart = performance.now();
    ctx.drawImage(imageBitmap, 0, 0);
    imageBitmap.close(); // Release resources
    const drawTime = performance.now() - drawStart;

    const totalTime = performance.now() - loopStart;

    // Store performance sample
    frontendPerfSamples.push({
      tauri_call_ms: fetchTime,  // Now measures fetch time instead of invoke
      image_create_ms: imageTime,
      canvas_draw_ms: drawTime,
      total_loop_ms: totalTime,
      frontend_fps: 0, // Will be calculated later
    });

    // Keep only last 30 samples
    if (frontendPerfSamples.length > 30) {
      frontendPerfSamples.shift();
    }

    // Update averaged frontend stats
    if (frontendPerfSamples.length > 0) {
      const avg = (key: keyof FrontendPerf) =>
        frontendPerfSamples.reduce((sum, s) => sum + s[key], 0) /
        frontendPerfSamples.length;

      frontendStats.value = {
        tauri_call_ms: avg("tauri_call_ms"),
        image_create_ms: avg("image_create_ms"),
        canvas_draw_ms: avg("canvas_draw_ms"),
        total_loop_ms: avg("total_loop_ms"),
        frontend_fps: fps.value,
      };
    }

    // Update statistics
    frameCount.value++;
    updateFps();
    errorMessage.value = "";
  } catch (error) {
    // Debounce error messages to avoid spamming
    const now = Date.now();
    if (now - lastErrorTime > 1000) {
      console.warn("Frame fetch error:", error);
      errorMessage.value = String(error);
      lastErrorTime = now;
    }
  }

  // Schedule the next frame
  if (isRendering.value) {
    animationId = requestAnimationFrame(renderLoop);
  }
}

/**
 * Calculate and update the FPS display
 * Updates once per second to avoid excessive updates
 */
function updateFps() {
  fpsFrameCount++;
  const now = performance.now();

  // Update FPS display once per second
  if (now - fpsLastUpdate >= 1000) {
    fps.value = Math.round(fpsFrameCount / ((now - fpsLastUpdate) / 1000));
    fpsFrameCount = 0;
    fpsLastUpdate = now;
  }
}

/**
 * Fetch backend performance statistics
 */
async function updateBackendStats() {
  try {
    const stats = await invoke<PerformanceStats>("get_performance_stats");
    backendStats.value = stats;
  } catch (error) {
    // Ignore errors silently
  }
}

// Periodically update backend stats
let statsInterval: number | null = null;

/**
 * Get performance class based on timing
 */
function getPerfClass(ms: number): string {
  if (ms < 1) return "perf-excellent";
  if (ms < 5) return "perf-good";
  if (ms < 10) return "perf-warn";
  return "perf-bad";
}

/**
 * Calculate total latency (backend + frontend)
 */
function getTotalLatency(): number {
  return backendStats.value.frame_encoding_ms + frontendStats.value.total_loop_ms;
}

// =============================================================================
// Control Functions
// =============================================================================

/**
 * Start the render loop
 */
function startRendering() {
  if (isRendering.value) return;

  isRendering.value = true;
  statusMessage.value = "Rendering...";
  errorMessage.value = "";
  fpsLastUpdate = performance.now();
  fpsFrameCount = 0;

  // Start the render loop
  animationId = requestAnimationFrame(renderLoop);

  // Start periodic backend stats updates (every 500ms)
  statsInterval = window.setInterval(updateBackendStats, 500);
  updateBackendStats(); // Initial update
}

/**
 * Stop the render loop
 */
function stopRendering() {
  isRendering.value = false;
  statusMessage.value = "Rendering stopped";

  if (animationId !== null) {
    cancelAnimationFrame(animationId);
    animationId = null;
  }

  if (statsInterval !== null) {
    clearInterval(statsInterval);
    statsInterval = null;
  }
}

// =============================================================================
// Lifecycle Hooks
// =============================================================================

onMounted(() => {
  // Auto-start rendering after a short delay to let Bevy initialize
  setTimeout(() => {
    startRendering();
  }, 500);

  // Add global mouse up listener (to catch releases outside canvas)
  window.addEventListener("mouseup", handleMouseUp);
});

onUnmounted(() => {
  // Clean up when component is destroyed
  stopRendering();

  // Remove global listeners
  window.removeEventListener("mouseup", handleMouseUp);
});
</script>

<template>
  <main class="app-container">
    <!-- Header Section -->
    <header class="header">
      <div class="header-content">
        <h1 class="title">Tauri + Bevy Demo</h1>
        <p class="subtitle">Offscreen 3D Rendering in a Web Canvas</p>
      </div>
      <!-- Status Messages in Header -->
      <div class="status-section">
        <p class="status-message">{{ statusMessage }}</p>
        <p v-if="errorMessage" class="error-message">{{ errorMessage }}</p>
      </div>
    </header>

    <div class="main-layout">
      <!-- Canvas Section (Left) -->
      <div class="canvas-section">
        <div class="canvas-wrapper">
          <canvas
            ref="canvasRef"
            width="800"
            height="600"
            class="render-canvas"
            @mousedown="handleMouseDown"
            @mousemove="handleMouseMove"
            @wheel="handleWheel"
            @contextmenu="handleContextMenu"
          ></canvas>

          <!-- Overlay Stats -->
          <div class="stats-overlay">
            <span class="stat">Frontend FPS: {{ fps }}</span>
            <span class="stat">Frames: {{ frameCount }}</span>
          </div>
        </div>
      </div>

      <!-- Sidebar Section (Right) -->
      <aside class="sidebar">
        <!-- Controls -->
        <div class="sidebar-block controls-block">
          <div class="controls">
            <button
              class="control-btn start"
              :disabled="isRendering"
              @click="startRendering"
            >
              ▶ Start
            </button>
            <button
              class="control-btn stop"
              :disabled="!isRendering"
              @click="stopRendering"
            >
              ◼ Stop
            </button>
          </div>
        </div>

        <!-- Performance Panel -->
        <div class="sidebar-block performance-block">
          <div class="performance-panel">
            <h3>🔍 Performance Diagnostics</h3>
            
            <div class="perf-section">
              <h4>🦀 Backend (Bevy/Rust)</h4>
              <div class="perf-grid">
                <div class="perf-item">
                  <span class="perf-label">Bevy FPS:</span>
                  <span class="perf-value">{{ backendStats.bevy_fps.toFixed(1) }}</span>
                </div>
                <div class="perf-item">
                  <span class="perf-label">GPU Transfer:</span>
                  <span class="perf-value" :class="getPerfClass(backendStats.gpu_transfer_ms)">
                    {{ backendStats.gpu_transfer_ms.toFixed(2) }}ms
                  </span>
                </div>
                <div class="perf-item">
                  <span class="perf-label">Data Processing:</span>
                  <span class="perf-value" :class="getPerfClass(backendStats.data_processing_ms)">
                    {{ backendStats.data_processing_ms.toFixed(2) }}ms
                  </span>
                </div>
                <div class="perf-item">
                  <span class="perf-label">Total Backend:</span>
                  <span class="perf-value" :class="getPerfClass(backendStats.frame_encoding_ms)">
                    {{ backendStats.frame_encoding_ms.toFixed(2) }}ms
                  </span>
                </div>
                <div class="perf-item">
                  <span class="perf-label">Data Size:</span>
                  <span class="perf-value">{{ backendStats.data_size_kb.toFixed(1) }} KB</span>
                </div>
                <div class="perf-item">
                  <span class="perf-label">Backend Frames:</span>
                  <span class="perf-value">{{ backendStats.frame_count }}</span>
                </div>
              </div>
            </div>

            <div class="perf-section">
              <h4>🔌 Tauri IPC</h4>
              <div class="perf-grid">
                <div class="perf-item">
                  <span class="perf-label">Get Frame:</span>
                  <span class="perf-value" :class="getPerfClass(backendStats.tauri_get_frame_ms)">
                    {{ backendStats.tauri_get_frame_ms.toFixed(2) }}ms
                  </span>
                </div>
                <div class="perf-item">
                  <span class="perf-label">Data Clone:</span>
                  <span class="perf-value" :class="getPerfClass(backendStats.tauri_serialize_ms)">
                    {{ backendStats.tauri_serialize_ms.toFixed(2) }}ms
                  </span>
                </div>
                <div class="perf-item">
                  <span class="perf-label">JS Call:</span>
                  <span class="perf-value" :class="getPerfClass(frontendStats.tauri_call_ms)">
                    {{ frontendStats.tauri_call_ms.toFixed(2) }}ms
                  </span>
                </div>
                <div class="perf-item">
                  <span class="perf-label">🔥 Overhead:</span>
                  <span class="perf-value perf-bad">
                    {{ (frontendStats.tauri_call_ms - backendStats.tauri_get_frame_ms - backendStats.tauri_serialize_ms).toFixed(2) }}ms
                  </span>
                </div>
              </div>
            </div>

            <div class="perf-section">
              <h4>⚡ Frontend (Vue)</h4>
              <div class="perf-grid">
                <div class="perf-item">
                  <span class="perf-label">Total Loop:</span>
                  <span class="perf-value" :class="getPerfClass(frontendStats.total_loop_ms)">
                    {{ frontendStats.total_loop_ms.toFixed(2) }}ms
                  </span>
                </div>
                <div class="perf-item">
                  <span class="perf-label">Draw:</span>
                  <span class="perf-value" :class="getPerfClass(frontendStats.canvas_draw_ms)">
                    {{ frontendStats.canvas_draw_ms.toFixed(2) }}ms
                  </span>
                </div>
              </div>
            </div>

            <div class="perf-analysis">
              <strong>Analysis:</strong>
              <span v-if="getTotalLatency() < 16.67" class="analysis-good">
                ✅ Excellent! ({{ getTotalLatency().toFixed(1) }}ms latency)
              </span>
              <span v-else-if="getTotalLatency() < 33.33" class="analysis-warn">
                ⚠️ Good ({{ getTotalLatency().toFixed(1) }}ms latency)
              </span>
              <span v-else class="analysis-bad">
                ❌ Lag ({{ getTotalLatency().toFixed(1) }}ms latency)
              </span>
            </div>
          </div>
        </div>

        <!-- Camera Controls Info -->
        <div class="sidebar-block info-block">
          <div class="info-panel">
            <h3>🎮 Camera Controls</h3>
            <ul class="controls-list">
              <li><strong>Left Drag:</strong> Rotate camera</li>
              <li><strong>Scroll:</strong> Zoom in/out</li>
            </ul>
          </div>
        </div>

        <!-- Info Panel -->
        <div class="sidebar-block info-block">
          <div class="info-panel">
            <h3>How It Works</h3>
            <ol>
              <li><strong>Bevy</strong> renders offscreen</li>
              <li><strong>Tauri</strong> transfers frame data</li>
              <li><strong>Vue</strong> displays on Canvas</li>
            </ol>
          </div>
        </div>
      </aside>
    </div>
  </main>
</template>

<style>
/* =============================================================================
   CSS Variables - Design Tokens
   Similar to C++ const definitions for reusable values
   ============================================================================= */
:root {
  /* Color palette - Deep space theme */
  --color-bg-primary: #0a0e17;
  --color-bg-secondary: #141b2d;
  --color-bg-tertiary: #1a2332;
  --color-accent-primary: #00d9ff;
  --color-accent-secondary: #7c3aed;
  --color-accent-gradient: linear-gradient(
    135deg,
    var(--color-accent-primary),
    var(--color-accent-secondary)
  );
  --color-text-primary: #e2e8f0;
  --color-text-secondary: #94a3b8;
  --color-text-muted: #64748b;
  --color-success: #10b981;
  --color-error: #ef4444;
  --color-border: #2d3748;

  /* Typography */
  --font-family: "JetBrains Mono", "Fira Code", "SF Mono", monospace;
  --font-size-base: 14px;
  --font-size-sm: 12px;
  --font-size-lg: 18px;
  --font-size-xl: 24px;
  --font-size-2xl: 32px;

  /* Spacing */
  --spacing-xs: 4px;
  --spacing-sm: 8px;
  --spacing-md: 16px;
  --spacing-lg: 24px;
  --spacing-xl: 32px;

  /* Effects */
  --shadow-glow: 0 0 20px rgba(0, 217, 255, 0.3);
  --shadow-card: 0 4px 6px -1px rgba(0, 0, 0, 0.5);
  --border-radius: 8px;
  --transition-fast: 150ms ease;
  --transition-normal: 250ms ease;
}

/* =============================================================================
   Reset & Base Styles
   ============================================================================= */
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

html,
body {
  height: 100%;
  background: var(--color-bg-primary);
  color: var(--color-text-primary);
  font-family: var(--font-family);
  font-size: var(--font-size-base);
  line-height: 1.6;
  overflow-x: hidden;
}

/* =============================================================================
   App Container
   ============================================================================= */
.app-container {
  height: 100vh;
  width: 100vw;
  display: flex;
  flex-direction: column;
  padding: 0;
  background: radial-gradient(ellipse at top, #1a1f35 0%, var(--color-bg-primary) 70%);
  overflow: hidden;
}

/* =============================================================================
   Header
   ============================================================================= */
.header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--spacing-md) var(--spacing-lg);
  border-bottom: 1px solid var(--color-border);
  background: rgba(10, 14, 23, 0.8);
  backdrop-filter: blur(8px);
  z-index: 10;
}

.header-content {
  text-align: left;
}

.title {
  font-size: var(--font-size-lg);
  font-weight: 700;
  background: var(--color-accent-gradient);
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  background-clip: text;
  margin-bottom: 2px;
  letter-spacing: -0.5px;
}

.subtitle {
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
  font-weight: 400;
}

/* =============================================================================
   Main Layout
   ============================================================================= */
.main-layout {
  flex: 1;
  display: grid;
  grid-template-columns: 1fr 400px;
  overflow: hidden;
}

/* =============================================================================
   Canvas Section
   ============================================================================= */
.canvas-section {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--spacing-lg);
  background: rgba(0, 0, 0, 0.2);
  position: relative;
  overflow: auto; /* Allow scrolling if canvas is larger than viewport */
}

.canvas-wrapper {
  position: relative;
  box-shadow: var(--shadow-glow), var(--shadow-card);
  border-radius: var(--border-radius);
  background: var(--color-bg-secondary);
  line-height: 0;
}

.render-canvas {
  display: block;
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius);
  /* Ensure crisp pixel rendering */
  image-rendering: crisp-edges;
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
}

.stats-overlay {
  position: absolute;
  top: var(--spacing-sm);
  left: var(--spacing-sm);
  display: flex;
  gap: var(--spacing-md);
  padding: var(--spacing-xs) var(--spacing-sm);
  background: rgba(0, 0, 0, 0.7);
  border-radius: var(--border-radius);
  font-size: var(--font-size-sm);
  font-weight: 500;
  pointer-events: none;
}

.stat {
  color: var(--color-accent-primary);
}

/* =============================================================================
   Sidebar
   ============================================================================= */
.sidebar {
  border-left: 1px solid var(--color-border);
  background: var(--color-bg-secondary);
  display: flex;
  flex-direction: column;
  overflow-y: auto;
  padding: var(--spacing-md);
  gap: var(--spacing-md);
}

.sidebar-block {
  width: 100%;
}

/* =============================================================================
   Controls
   ============================================================================= */
.controls {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--spacing-sm);
}

.control-btn {
  padding: var(--spacing-sm);
  font-family: var(--font-family);
  font-size: var(--font-size-sm);
  font-weight: 600;
  border: none;
  border-radius: var(--border-radius);
  cursor: pointer;
  transition: all var(--transition-fast);
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--spacing-xs);
}

.control-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.control-btn.start {
  background: var(--color-success);
  color: white;
}

.control-btn.start:hover:not(:disabled) {
  background: #059669;
  transform: translateY(-1px);
}

.control-btn.stop {
  background: var(--color-error);
  color: white;
}

.control-btn.stop:hover:not(:disabled) {
  background: #dc2626;
  transform: translateY(-1px);
}

/* =============================================================================
   Status Section
   ============================================================================= */
.status-section {
  text-align: right;
}

.status-message {
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
  font-weight: 500;
}

.error-message {
  color: var(--color-error);
  font-size: var(--font-size-xs);
  margin-top: 2px;
}

/* =============================================================================
   Performance Panel
   ============================================================================= */
.performance-panel {
  background: var(--color-bg-tertiary);
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius);
  padding: var(--spacing-md);
}

.performance-panel h3 {
  font-size: var(--font-size-base);
  color: var(--color-accent-primary);
  margin-bottom: var(--spacing-md);
  font-weight: 600;
  text-align: center;
}

.perf-section {
  margin-bottom: var(--spacing-md);
}

.perf-section:last-of-type {
  margin-bottom: var(--spacing-sm);
}

.perf-section h4 {
  font-size: var(--font-size-sm);
  color: var(--color-text-primary);
  margin-bottom: var(--spacing-sm);
  font-weight: 600;
  border-bottom: 1px solid var(--color-border);
  padding-bottom: 2px;
}

.perf-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--spacing-sm);
}

.perf-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 6px var(--spacing-sm);
  background: var(--color-bg-primary);
  border-radius: var(--border-radius);
  border: 1px solid var(--color-border);
}

.perf-label {
  color: var(--color-text-secondary);
  font-size: 11px;
}

.perf-value {
  color: var(--color-text-primary);
  font-weight: 600;
  font-size: var(--font-size-sm);
}

/* Performance value color coding */
.perf-excellent { color: #10b981 !important; }
.perf-good { color: #3b82f6 !important; }
.perf-warn { color: #f59e0b !important; }
.perf-bad { color: #ef4444 !important; }

.perf-analysis {
  padding: var(--spacing-sm);
  background: var(--color-bg-primary);
  border-radius: var(--border-radius);
  border-left: 3px solid var(--color-accent-primary);
  font-size: var(--font-size-xs);
  margin-top: var(--spacing-sm);
}

.perf-analysis strong {
  color: var(--color-accent-primary);
  margin-right: 4px;
}

.analysis-good { color: #10b981; }
.analysis-warn { color: #f59e0b; }
.analysis-bad { color: #ef4444; }

/* =============================================================================
   Info Panel
   ============================================================================= */
.info-panel {
  background: var(--color-bg-tertiary);
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius);
  padding: var(--spacing-md);
}

.info-panel h3 {
  font-size: var(--font-size-sm);
  color: var(--color-accent-primary);
  margin-bottom: var(--spacing-sm);
  font-weight: 600;
}

.info-panel ol {
  list-style-position: inside;
  color: var(--color-text-secondary);
  font-size: var(--font-size-xs);
}

.info-panel li {
  margin-bottom: 4px;
  line-height: 1.4;
}

.info-panel strong {
  color: var(--color-text-primary);
}

.controls-list {
  list-style: none;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
}

.controls-list li {
  margin-bottom: 6px;
  padding-left: 0;
}

.controls-list strong {
  color: var(--color-accent-primary);
  margin-right: 4px;
}

/* Make canvas interactive */
.render-canvas {
  cursor: grab;
}

.render-canvas:active {
  cursor: grabbing;
}
</style>
