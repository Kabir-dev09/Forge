# Forge Terminal Performance Engineering Report

## Executive Summary

Forge is a Wayland-native, Vulkan-accelerated terminal emulator written primarily in Rust. Its current architecture already contains several performance-oriented design choices:

- A fast startup path using cached window/font metrics.
- A temporary Wayland SHM first frame before Vulkan initialization completes.
- A dedicated PTY parsing thread.
- A main-thread-only Vulkan renderer with explicit ownership of GPU resources.
- Dirty-row tracking in the terminal screen buffer.
- Background font atlas construction.
- Lua configuration loading and hot reload through a separate config actor.

These are good foundations. The next stage of performance work should be measurement-driven and should prioritize algorithmic improvements, memory layout, batching, and reduced synchronization overhead before introducing C, assembly, or hand-written SIMD.

The original proposal correctly identifies several likely hot areas: VTE parsing, grid tessellation, screen buffer mutation, and font atlas/rasterization. However, the best implementation path is not to immediately move these systems into C or assembly. Forge can gain most of the practical benefit while preserving safety and maintainability through Rust-native optimizations, better data structures, targeted unsafe blocks where justified, and optional SIMD only after profiling proves a need.

---

## Performance Strategy

Before implementing low-level optimizations, Forge should establish a repeatable performance baseline.

Recommended measurement targets:

- Time to first visible frame.
- Time to first Vulkan frame.
- PTY throughput for plain text, escape-heavy output, and mixed interactive workloads.
- Frame render time during full redraws and dirty-row redraws.
- CPU time spent in VTE parsing, screen buffer mutation, tessellation, vertex upload, and Vulkan submission.
- Lock contention around `ScreenBuffer`.
- Memory allocations during steady-state rendering.
- Resize/reflow latency.
- Font atlas build time and memory usage.

Recommended tooling:

- `cargo bench` or Criterion benchmarks for parser, screen buffer, and tessellation paths.
- `tracing` spans around major runtime phases.
- `perf`, flamegraphs, or similar Linux profiling tools.
- Optional GPU timing queries later for renderer-side analysis.

This keeps optimization work grounded in evidence and prevents premature complexity.

---

## 1. PTY and VTE Parsing

**Location:** `crates/forge-pty/src/vte_parser.rs`  
**Related runtime path:** PTY background thread in `crates/forge-main/src/event_loop.rs`

### Current Design

Forge uses the `vte` crate to process terminal byte streams. This is a reasonable and maintainable choice: terminal escape handling is complex, and using a proven parser avoids many correctness problems.

Currently, PTY data is processed byte by byte through the parser. This is correct but may be inefficient for common workloads where most bytes are printable text and only occasional bytes are control characters or escape sequences.

### Refined Optimization Approach

The best first optimization is a Rust-native printable-text fast path.

Instead of sending every byte directly through the VTE state machine, the parser can scan incoming data for control bytes:

- `0x00..=0x1F`
- `0x7F`
- C1 controls if relevant for the parser mode

When a run contains only printable ASCII, Forge can process that run as a batch rather than advancing the parser byte by byte.

A practical implementation path:

1. Use `memchr`/`memchr2`-style scanning or a compact Rust loop to find the next control byte.
2. Add a fast path for plain ASCII runs.
3. Keep the existing `vte` parser as the authoritative path for control sequences and complex input.
4. Preserve Unicode correctness by only bulk-processing cases that are safe, such as printable ASCII.
5. Benchmark before considering explicit SIMD.

### Why This Is Better Than Immediate C/Assembly

A SIMD scanner could eventually be useful, but it should not be the first step. Rust already allows efficient byte scanning, and libraries such as `memchr` often use platform-specific SIMD internally. This avoids FFI complexity while still reducing parser overhead.

The key performance win is batching and avoiding unnecessary state-machine transitions, not the language used to implement the scanner.

### Expected Benefit

This should improve throughput for common outputs such as:

- `cat` of large text files
- compiler logs
- command output with minimal styling
- shell history dumps

It should have limited effect on escape-heavy TUIs, where correctness still depends on the full VTE parser.

---

## 2. Screen Buffer Data Layout and Mutation

**Location:** `crates/forge-pty/src/screen_buffer.rs`

### Current Design

The active screen buffer stores rows as `Vec<Row>`, where each `Row` owns boxed cell storage. This is straightforward and easy to reason about, but some operations are expensive:

- Scrolling can involve `Vec::remove` and `Vec::insert`.
- Scrollback can shift data when old rows are removed.
- Resize/reflow performs large row reconstruction.
- Several operations repeatedly mark rows dirty.
- Some event-loop paths repeatedly acquire read/write locks around the buffer.

The project also contains a `CircularGrid` implementation, suggesting that a more efficient ring-buffer design has already been considered.

### Refined Optimization Approach

The most important improvement is algorithmic: avoid moving rows during common scroll operations.

Recommended direction:

1. Move primary screen storage and scrollback toward a ring-buffer model.
2. Avoid `remove(0)` patterns for scrollback history.
3. Store row metadata separately where useful, such as wrap flags and dirty flags.
4. Keep row access APIs simple so the renderer does not need to know about physical storage layout.
5. Reduce lock churn by grouping buffer reads/writes in the event loop.

### Fill and Clear Operations

The original report proposed non-temporal SIMD stores for clearing the screen. That is not recommended as an early optimization.

Terminal buffers are usually read soon after being written, especially before rendering. Bypassing cache can therefore hurt performance. Standard slice fills, row-level replacement, or carefully optimized Rust copy/fill operations are more appropriate unless profiling proves cache pollution is a real problem.

### Scrolling

The best scrolling optimization is to stop copying memory when possible. A circular row model is likely to outperform custom assembly `memmove`, while also improving maintainability.

If memory copies remain necessary in specific paths, Rust slice operations should be preferred first. They already lower to efficient `memcpy`/`memmove` where applicable.

### Expected Benefit

A better buffer layout should improve:

- Large scrollback performance.
- Continuous output throughput.
- Resize/reflow behavior.
- Memory allocation stability.
- Renderer dirty-row accuracy.

---

## 3. Grid Tessellation and Vertex Generation

**Location:** `crates/forge-renderer/src/grid_tessellator.rs`

### Current Design

Forge converts terminal cells into GPU vertex data before rendering. The tessellator handles:

- Foreground glyph quads.
- Background quads.
- Cursor rendering.
- Selection rendering.
- Underline and strikethrough.
- Box drawing and block characters.
- Braille rendering.
- Powerline-style procedural glyphs.
- Scrollbar geometry.
- Dirty-row reuse.

This is a legitimate performance-sensitive area. The code already has an important optimization: it stores per-row tessellation and only rebuilds rows that are dirty or affected by cursor/selection changes.

### Refined Optimization Approach

The next improvements should focus on reducing branching, allocation, and redundant work.

Recommended steps:

1. Preserve row-level tessellation caching.
2. Ensure vertex vectors are reused and capacity is retained across frames.
3. Replace large character classification paths with lookup tables where it improves clarity and speed.
4. Consider generated Rust lookup tables for box drawing metadata instead of a large hand-written `match`.
5. Separate common ASCII glyph rendering from procedural Unicode rendering.
6. Avoid rebuilding unaffected rows when only cursor blink changes.
7. Track cursor old/new rows precisely.
8. Track selection old/new ranges precisely.
9. Ensure vertex buffer capacity grows predictably and does not reallocate during normal use.

### Box Drawing Optimization

The current `decode_box_drawing` match is correct but bulky. A table-driven design would be cleaner and may improve branch prediction.

Recommended implementation:

- Use a Rust static lookup table keyed by Unicode offset for box drawing ranges.
- Store compact encoded metadata for up/down/left/right stroke weights and rounded-corner flag.
- Keep procedural rendering in the fragment shader as it is today.

This avoids C or assembly while improving maintainability.

### SIMD Considerations

SIMD is not the best immediate fit for the full tessellation function because the work is branch-heavy and semantically diverse. SIMD may become useful later for narrow operations such as coordinate generation, but only after the simpler structural improvements are measured.

### Expected Benefit

This should reduce CPU time during:

- Full redraws.
- Large terminal windows.
- Selection changes.
- Cursor blinking.
- Output bursts that dirty many rows.

---

## 4. Vertex Upload and GPU Submission

**Location:** `crates/forge-renderer/src/renderer.rs`

### Current Design

Forge uses a host-visible vertex buffer and uploads tessellated vertices before issuing draw calls. Rendering happens on the main thread, which is appropriate for the current Vulkan ownership model.

The renderer recreates the swapchain on resize or out-of-date presentation results. It also grows the vertex buffer when the estimated required vertex count exceeds capacity.

### Refined Optimization Approach

Recommended improvements:

1. Keep the renderer main-thread-owned unless a larger render architecture is introduced.
2. Use persistent mapped buffers if profiling shows map/unmap overhead is meaningful.
3. Consider a ring of per-frame vertex buffers to avoid synchronization hazards.
4. Ensure vertex buffer growth is conservative and amortized.
5. Track uploaded byte counts and vertex counts through tracing.
6. Avoid full vertex upload when future renderer changes allow partial row upload.

### GPU/CPU Balance

The current renderer is primarily CPU-driven: terminal cells are converted to vertices on the CPU, then submitted to Vulkan. This is reasonable for a terminal emulator and easier to control than a heavily GPU-driven design.

A future GPU-side cell buffer could reduce vertex upload cost, but it would significantly complicate shaders, memory layout, and update logic. It is not the right next step unless profiling shows vertex upload dominates frame time.

---

## 5. Font Rasterization, Atlas, and Text Shaping

**Location:** `crates/forge-renderer/src/font/`

### Current Design

Forge uses:

- `fontdue` for rasterization.
- `rustybuzz` for shaping support.
- A glyph atlas uploaded as a Vulkan texture.
- A dummy atlas during initial renderer boot.
- Background construction of the full atlas.

This is a sensible startup optimization. It allows the application to present quickly without blocking the entire startup sequence on full glyph rasterization.

### Refined Optimization Approach

Recommended improvements:

1. Keep background atlas construction.
2. Avoid rebuilding the full atlas unless font configuration changes.
3. Cache atlas metadata where possible.
4. Treat font loading paths as configuration-driven rather than hardcoded.
5. Add benchmarks for atlas build time and memory usage.
6. Consider lazy glyph insertion only if full atlas construction proves too expensive.

### FreeType/HarfBuzz Consideration

Moving to FreeType or HarfBuzz through C FFI may improve compatibility or rendering quality, but it should not be framed as an automatic performance win.

Forge already uses Rust libraries designed for this domain. Replacing them would add dependency and packaging complexity. A C library transition should be justified by specific missing capabilities or measured quality/performance problems, not by assumption.

---

## 6. Startup Performance

**Locations:**  
`crates/forge-main/src/main.rs`  
`crates/forge-core/src/cache.rs`  
`crates/forge-config/src/actor.rs`

### Current Design

Forge has a thoughtful startup sequence:

1. Initialize logging and panic handling.
2. Spawn the Lua config actor early.
3. Read startup cache.
4. Connect to Wayland.
5. Create the window.
6. Present a first SHM frame using cached/default colors.
7. Load resolved configuration.
8. Initialize Vulkan.
9. Spawn the PTY.
10. Render using Vulkan.
11. Load full font atlas in the background.
12. Enter the main event loop.

This is one of the stronger parts of the project. It prioritizes perceived startup time instead of waiting for every subsystem to complete before showing a window.

### Recommended Improvements

1. Fix startup cache version consistency.
2. Remove hardcoded development font paths.
3. Store resolved font metrics and selected font identity in the cache.
4. Invalidate cache when relevant config or font settings change.
5. Add tracing spans for each startup phase.
6. Distinguish time to first SHM frame from time to first Vulkan text frame.

### Expected Benefit

These changes improve startup reliability and make performance regressions easier to detect.

---

## 7. Concurrency and Synchronization

### Current Design

Forge uses multiple threads:

- Main thread for Wayland event loop and Vulkan rendering.
- PTY reader/parser thread.
- Config actor thread.
- Config watcher thread.
- Font atlas loading thread.
- Clipboard helper threads.

This model is reasonable, but the shared `ScreenBuffer` is protected by an `Arc<RwLock<_>>`, and the event loop sometimes performs repeated lock acquisitions in close succession.

### Refined Optimization Approach

Recommended improvements:

1. Reduce repeated `read()`/`write()` lock calls in the event loop.
2. Group related buffer mutations under one write lock.
3. Avoid holding buffer locks while performing PTY writes or renderer work.
4. Consider message-based buffer update ownership if lock contention appears in profiling.
5. Keep Vulkan rendering on the main thread for now.

### Why This Matters

Lock overhead is usually smaller than rendering or parsing cost, but repeated locking can become visible under high PTY throughput or frequent redraws. Cleaning this up improves both performance and maintainability.

---

## 8. Configuration System

**Location:** `crates/forge-config/`

### Current Design

Forge uses Lua configuration loaded through a background actor. Missing config files are generated from an embedded default config. File changes trigger reloads through a watcher thread.

This is a flexible and user-friendly design.

### Recommended Improvements

1. Apply configuration changes selectively instead of replacing the entire runtime config.
2. Define which config changes are live-reloadable and which require restart.
3. Avoid reload storms by debouncing filesystem events.
4. Improve error reporting for invalid Lua config.
5. Ensure documented config keys match extractor behavior.
6. Keep previous valid config on reload failure, which the current design already supports.

### Expected Benefit

This improves runtime stability and makes the configuration system easier to extend.

---

## 9. Error Handling and Recovery

### Current Design

Forge has a shared `ForgeError` type and a panic hook that writes crash logs. Renderer errors such as surface loss and swapchain recreation are partially handled.

### Recommended Improvements

1. Reduce reliance on `unwrap()` in runtime paths where recovery is possible.
2. Convert expected runtime failures into typed errors.
3. Keep crash logging for truly unexpected failures.
4. Add clearer handling for missing Wayland globals such as data device manager.
5. Improve Vulkan error context around device/surface/swapchain failures.
6. Add tests for config reload failures and PTY parser edge cases.

These changes are not primarily performance optimizations, but they improve professional quality and reliability.

---

## 10. C, Assembly, and SIMD Policy

C, assembly, and explicit SIMD should be considered tools of last resort, not default optimization strategies.

Recommended policy:

1. Prefer algorithmic improvements first.
2. Prefer Rust-native optimized libraries second.
3. Use targeted unsafe Rust where profiling proves bounds checks or layout overhead matter.
4. Use explicit SIMD through Rust intrinsics only for isolated, benchmarked kernels.
5. Use C/assembly only when:
   - The hot path is proven by profiling.
   - Rust cannot express the optimization cleanly.
   - CPU feature detection and fallback paths are implemented.
   - The maintenance cost is justified.

Potential future SIMD candidates:

- Printable-byte scanning in PTY input.
- ASCII-only text insertion.
- Simple vertex coordinate generation.
- Bulk color conversion if profiling shows it matters.

Non-recommended early candidates:

- Non-temporal stores for screen clearing.
- Assembly `memmove` replacements.
- C rewrites of branch-heavy tessellation logic.
- FFI font stack replacement without a quality or performance benchmark.

---

## Prioritized Roadmap

### Phase 1: Measurement

- Add parser, screen buffer, and tessellation benchmarks.
- Add tracing spans for startup and frame phases.
- Capture baseline flamegraphs.

### Phase 2: Low-Risk Runtime Optimizations

- Add printable ASCII fast path before VTE parsing.
- Reduce repeated `RwLock` acquisitions.
- Reuse allocations more aggressively in tessellation.
- Fix startup cache versioning.
- Remove hardcoded font paths.

### Phase 3: Data Structure Improvements

- Move scrollback and scrolling behavior toward ring-buffer storage.
- Improve dirty-row and selection/cursor invalidation tracking.
- Replace box drawing match with table-driven metadata if benchmarks support it.

### Phase 4: Renderer Efficiency

- Evaluate persistent mapped vertex buffers.
- Improve vertex buffer growth strategy.
- Add render-time diagnostics for vertex count and upload size.

### Phase 5: Targeted SIMD

- Only after benchmarks identify a specific hot kernel.
- Implement Rust-native SIMD or optimized library use first.
- Keep portable fallback paths.

---

## Conclusion

Forge already has a solid performance-oriented foundation: Vulkan rendering, Wayland-native operation, dirty-row tracking, background configuration and font work, and a startup path designed around early visual feedback.

The most valuable next improvements are not broad C or assembly rewrites. The project will benefit more from profiling, batching, better screen-buffer data structures, reduced locking overhead, cleaner cache/config handling, and targeted renderer improvements.

Low-level SIMD may eventually be useful, especially for PTY scanning, but it should be introduced surgically and only after measurement. The project's architecture is strongest when it keeps performance-critical systems explicit, modular, and mostly Rust-native while reserving unsafe or platform-specific code for narrow, proven bottlenecks.
