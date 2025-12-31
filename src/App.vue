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

/** Response from the get_frame Tauri command */
interface FrameResponse {
  data: string; // Base64-encoded RGBA pixel data
  width: number;
  height: number;
}

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
// Render Loop
// =============================================================================

/**
 * Main render loop that fetches frames from Bevy and draws them to canvas.
 *
 * How it works:
 * 1. Call Tauri's invoke() to request the latest frame from Rust
 * 2. Decode the Base64 PNG data into an Image object
 * 3. Draw the image onto the canvas
 * 4. Schedule the next frame using requestAnimationFrame
 *
 * C++ analogy: Similar to a game's main loop that polls for input and renders
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
    // Measure Tauri call time
    const tauriStart = performance.now();
    const response = await invoke<FrameResponse>("get_frame");
    const tauriTime = performance.now() - tauriStart;

    // Measure Base64 decoding + ImageData creation time
    const imageStart = performance.now();
    // Decode Base64 to binary
    const binaryString = atob(response.data);
    const bytes = new Uint8ClampedArray(binaryString.length);
    for (let i = 0; i < binaryString.length; i++) {
      bytes[i] = binaryString.charCodeAt(i);
    }
    const imageData = new ImageData(bytes, response.width, response.height);
    const imageTime = performance.now() - imageStart;

    // Measure canvas drawing time
    const drawStart = performance.now();
    ctx.putImageData(imageData, 0, 0);
    const drawTime = performance.now() - drawStart;

    const totalTime = performance.now() - loopStart;

    // Store performance sample
    frontendPerfSamples.push({
      tauri_call_ms: tauriTime,
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
});

onUnmounted(() => {
  // Clean up when component is destroyed
  stopRendering();
});
</script>

<template>
  <main class="app-container">
    <!-- Header Section -->
    <header class="header">
      <h1 class="title">Tauri + Bevy Demo</h1>
      <p class="subtitle">Offscreen 3D Rendering in a Web Canvas</p>
    </header>

    <!-- Canvas Section -->
    <div class="canvas-wrapper">
      <canvas
        ref="canvasRef"
        width="800"
        height="600"
        class="render-canvas"
      ></canvas>

      <!-- Overlay Stats -->
      <div class="stats-overlay">
        <span class="stat">Frontend FPS: {{ fps }}</span>
        <span class="stat">Frames: {{ frameCount }}</span>
      </div>
    </div>

    <!-- Performance Panel -->
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
        <h4>🔌 Tauri IPC (Rust → JS)</h4>
        <div class="perf-grid">
          <div class="perf-item">
            <span class="perf-label">Get Frame (Rust):</span>
            <span class="perf-value" :class="getPerfClass(backendStats.tauri_get_frame_ms)">
              {{ backendStats.tauri_get_frame_ms.toFixed(2) }}ms
            </span>
          </div>
          <div class="perf-item">
            <span class="perf-label">Data Clone (Rust):</span>
            <span class="perf-value" :class="getPerfClass(backendStats.tauri_serialize_ms)">
              {{ backendStats.tauri_serialize_ms.toFixed(2) }}ms
            </span>
          </div>
          <div class="perf-item">
            <span class="perf-label">Tauri Call (JS):</span>
            <span class="perf-value" :class="getPerfClass(frontendStats.tauri_call_ms)">
              {{ frontendStats.tauri_call_ms.toFixed(2) }}ms
            </span>
          </div>
          <div class="perf-item">
            <span class="perf-label">🔥 IPC Overhead:</span>
            <span class="perf-value perf-bad">
              {{ (frontendStats.tauri_call_ms - backendStats.tauri_get_frame_ms - backendStats.tauri_serialize_ms).toFixed(2) }}ms
            </span>
          </div>
        </div>
      </div>

      <div class="perf-section">
        <h4>⚡ Frontend (Vue/Canvas)</h4>
        <div class="perf-grid">
          <div class="perf-item">
            <span class="perf-label">Frontend FPS:</span>
            <span class="perf-value">{{ frontendStats.frontend_fps.toFixed(1) }}</span>
          </div>
          <div class="perf-item">
            <span class="perf-label">Tauri Call:</span>
            <span class="perf-value" :class="getPerfClass(frontendStats.tauri_call_ms)">
              {{ frontendStats.tauri_call_ms.toFixed(2) }}ms
            </span>
          </div>
          <div class="perf-item">
            <span class="perf-label">ImageData Create:</span>
            <span class="perf-value" :class="getPerfClass(frontendStats.image_create_ms)">
              {{ frontendStats.image_create_ms.toFixed(2) }}ms
            </span>
          </div>
          <div class="perf-item">
            <span class="perf-label">Canvas Draw:</span>
            <span class="perf-value" :class="getPerfClass(frontendStats.canvas_draw_ms)">
              {{ frontendStats.canvas_draw_ms.toFixed(2) }}ms
            </span>
          </div>
          <div class="perf-item">
            <span class="perf-label">Total Frontend:</span>
            <span class="perf-value" :class="getPerfClass(frontendStats.total_loop_ms)">
              {{ frontendStats.total_loop_ms.toFixed(2) }}ms
            </span>
          </div>
        </div>
      </div>

      <div class="perf-analysis">
        <strong>Analysis:</strong>
        <span v-if="getTotalLatency() < 16.67" class="analysis-good">
          ✅ Excellent! Running at 60+ FPS ({{ getTotalLatency().toFixed(1) }}ms total latency)
        </span>
        <span v-else-if="getTotalLatency() < 33.33" class="analysis-warn">
          ⚠️ Good but could improve ({{ getTotalLatency().toFixed(1) }}ms total latency, targeting 30+ FPS)
        </span>
        <span v-else class="analysis-bad">
          ❌ Performance issue detected ({{ getTotalLatency().toFixed(1) }}ms total latency)
        </span>
      </div>
    </div>

    <!-- Controls -->
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

    <!-- Status Messages -->
    <div class="status-section">
      <p class="status-message">{{ statusMessage }}</p>
      <p v-if="errorMessage" class="error-message">{{ errorMessage }}</p>
    </div>

    <!-- Info Panel -->
    <div class="info-panel">
      <h3>How It Works</h3>
      <ol>
        <li><strong>Bevy</strong> renders a 3D scene to an offscreen buffer</li>
        <li><strong>Tauri</strong> transfers frame data via commands</li>
        <li><strong>Vue</strong> displays frames on this HTML Canvas</li>
      </ol>
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
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: var(--spacing-lg);
  background: radial-gradient(ellipse at top, #1a1f35 0%, var(--color-bg-primary) 70%);
}

/* =============================================================================
   Header
   ============================================================================= */
.header {
  text-align: center;
  margin-bottom: var(--spacing-lg);
}

.title {
  font-size: var(--font-size-2xl);
  font-weight: 700;
  background: var(--color-accent-gradient);
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  background-clip: text;
  margin-bottom: var(--spacing-xs);
  letter-spacing: -0.5px;
}

.subtitle {
  color: var(--color-text-secondary);
  font-size: var(--font-size-base);
  font-weight: 400;
}

/* =============================================================================
   Canvas Section
   ============================================================================= */
.canvas-wrapper {
  position: relative;
  margin-bottom: var(--spacing-lg);
}

.render-canvas {
  display: block;
  border: 2px solid var(--color-border);
  border-radius: var(--border-radius);
  background: var(--color-bg-secondary);
  box-shadow: var(--shadow-glow), var(--shadow-card);
  /* Ensure crisp pixel rendering */
  image-rendering: crisp-edges;
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
}

.stat {
  color: var(--color-accent-primary);
}

/* =============================================================================
   Controls
   ============================================================================= */
.controls {
  display: flex;
  gap: var(--spacing-md);
  margin-bottom: var(--spacing-lg);
}

.control-btn {
  padding: var(--spacing-sm) var(--spacing-lg);
  font-family: var(--font-family);
  font-size: var(--font-size-base);
  font-weight: 600;
  border: none;
  border-radius: var(--border-radius);
  cursor: pointer;
  transition: all var(--transition-fast);
  display: flex;
  align-items: center;
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
  transform: translateY(-2px);
  box-shadow: 0 4px 12px rgba(16, 185, 129, 0.4);
}

.control-btn.stop {
  background: var(--color-error);
  color: white;
}

.control-btn.stop:hover:not(:disabled) {
  background: #dc2626;
  transform: translateY(-2px);
  box-shadow: 0 4px 12px rgba(239, 68, 68, 0.4);
}

/* =============================================================================
   Status Section
   ============================================================================= */
.status-section {
  text-align: center;
  margin-bottom: var(--spacing-lg);
}

.status-message {
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
}

.error-message {
  color: var(--color-error);
  font-size: var(--font-size-sm);
  margin-top: var(--spacing-xs);
}

/* =============================================================================
   Info Panel
   ============================================================================= */
.info-panel {
  background: var(--color-bg-secondary);
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius);
  padding: var(--spacing-lg);
  max-width: 500px;
  width: 100%;
}

.info-panel h3 {
  font-size: var(--font-size-lg);
  color: var(--color-accent-primary);
  margin-bottom: var(--spacing-md);
  font-weight: 600;
}

.info-panel ol {
  list-style-position: inside;
  color: var(--color-text-secondary);
}

.info-panel li {
  margin-bottom: var(--spacing-sm);
  line-height: 1.8;
}

.info-panel strong {
  color: var(--color-text-primary);
}

/* =============================================================================
   Performance Panel
   ============================================================================= */
.performance-panel {
  background: var(--color-bg-secondary);
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius);
  padding: var(--spacing-lg);
  max-width: 800px;
  width: 100%;
  margin-bottom: var(--spacing-lg);
}

.performance-panel h3 {
  font-size: var(--font-size-lg);
  color: var(--color-accent-primary);
  margin-bottom: var(--spacing-lg);
  font-weight: 600;
  text-align: center;
}

.perf-section {
  margin-bottom: var(--spacing-lg);
}

.perf-section h4 {
  font-size: var(--font-size-base);
  color: var(--color-text-primary);
  margin-bottom: var(--spacing-md);
  font-weight: 600;
  border-bottom: 1px solid var(--color-border);
  padding-bottom: var(--spacing-xs);
}

.perf-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
  gap: var(--spacing-md);
}

.perf-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--spacing-sm);
  background: var(--color-bg-tertiary);
  border-radius: var(--border-radius);
  border: 1px solid var(--color-border);
}

.perf-label {
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
}

.perf-value {
  color: var(--color-text-primary);
  font-weight: 600;
  font-size: var(--font-size-base);
}

/* Performance value color coding */
.perf-excellent {
  color: #10b981 !important;
}

.perf-good {
  color: #3b82f6 !important;
}

.perf-warn {
  color: #f59e0b !important;
}

.perf-bad {
  color: #ef4444 !important;
}

.perf-analysis {
  padding: var(--spacing-md);
  background: var(--color-bg-tertiary);
  border-radius: var(--border-radius);
  border-left: 4px solid var(--color-accent-primary);
  font-size: var(--font-size-sm);
}

.perf-analysis strong {
  color: var(--color-accent-primary);
  margin-right: var(--spacing-xs);
}

.analysis-good {
  color: #10b981;
}

.analysis-warn {
  color: #f59e0b;
}

.analysis-bad {
  color: #ef4444;
}
</style>
