# Forge Optimization Implementation Roadmap

This roadmap is based on `Forge_Performance_Engineering_Report.md`. It is ordered so each optimization is completed, tested, and verified before the next begins.

## Execution Rule

Work through this list strictly in order. Each item must pass its completion gate before the next starts: implementation finished, tests passing, affected docs/comments updated, no partial code paths left behind, and performance validated when relevant.

---

## Phase 0: Baseline And Guardrails

### 0.1 Add Benchmark And Profiling Baseline

**Objective:** Establish measurable baselines before optimizing.

**Why now:** Every later optimization depends on knowing whether it helped.

**Dependencies:** None.

**Affected files/modules:** `crates/forge-pty`, `crates/forge-renderer`, `crates/forge-main`, workspace `Cargo.toml` if benchmark deps are added.

**Steps:**
1. Add benchmark harnesses for VTE parsing, screen buffer scrolling/clearing/reflow, and grid tessellation.
2. Add representative workloads: plain ASCII, mixed UTF-8, escape-heavy output, full-screen redraws, selection/cursor changes.
3. Add tracing spans around startup phases, PTY parse batches, tessellation, vertex upload, and render submission.
4. Document how to run benchmarks and capture flamegraphs.
5. Capture initial baseline numbers.

**Pitfalls:** Benchmarks that do not reflect real terminal workloads; unstable measurements; benchmarking debug builds.

**Testing strategy:** `cargo test`; `cargo bench` or equivalent; verify benchmark data is reproducible enough to compare.

**Success criteria:** Baseline metrics exist and can be rerun consistently.

**Benefit:** Prevents speculative optimization and provides regression protection.

---

## Phase 1: Correctness And Startup Hygiene

### 1.1 Fix Startup Cache Versioning

**Objective:** Make startup cache read/write behavior internally consistent.

**Why now:** The report identifies this as a reliability issue in the fast startup path. It is small, foundational, and low risk.

**Dependencies:** Phase 0 baseline.

**Affected files:** `crates/forge-core/src/cache.rs`, related tests.

**Steps:**
1. Decide the active cache version.
2. Align `read_startup_cache()` and `StartupCache::new_cache()`.
3. Add tests for valid cache, wrong version, wrong checksum, wrong size.
4. Confirm fallback behavior remains non-fatal.

**Pitfalls:** Accidentally accepting stale cache data; breaking first-run behavior.

**Testing strategy:** Unit tests for cache; `cargo test`.

**Success criteria:** Valid cache round-trips; invalid cache safely falls back.

**Benefit:** Reliable startup acceleration.

### 1.2 Remove Hardcoded Font Paths

**Objective:** Replace development-only absolute font paths with config/resource-based resolution.

**Why now:** Font loading affects startup, packaging, installer behavior, and portability.

**Dependencies:** Cache version fix.

**Affected files:** `crates/forge-main/src/main.rs`, `crates/forge-core/src/config_registry.rs`, possibly `crates/forge-renderer/src/font`.

**Steps:**
1. Inspect current font config usage.
2. Define resolution order: configured font path/family, bundled asset fallback, system fallback.
3. Replace `/home/kabir/PROJECTS/Forge/...` paths.
4. Preserve background atlas loading.
5. Log chosen font source clearly.
6. Add failure fallback to keep app usable.

**Pitfalls:** Font family resolution is not currently fully implemented; packaging paths may differ; missing bold font fallback.

**Testing strategy:** Unit-test path selection if factored out; run startup with bundled fonts missing/present; `cargo test`.

**Success criteria:** No absolute developer paths remain; Forge can still start with fallback fonts.

**Benefit:** Portable startup and cleaner packaging.

### 1.3 Add Startup Phase Tracing

**Objective:** Make startup performance observable.

**Why now:** Needed before deeper startup work.

**Dependencies:** None beyond current logging.

**Affected files:** `crates/forge-main/src/main.rs`, possibly `crates/forge-core/src/cache.rs`, `crates/forge-config/src/actor.rs`.

**Steps:**
1. Add consistent tracing spans/timers for config actor spawn, cache read, Wayland connect, SHM frame, Vulkan init, PTY spawn, font thread spawn, first Vulkan frame.
2. Distinguish "first visible SHM frame" from "first Vulkan text frame."
3. Keep logs concise and controlled by tracing filters.

**Pitfalls:** Excessive logging in hot paths; misleading timing scopes.

**Testing strategy:** Run app startup logs; `cargo test`.

**Success criteria:** Startup path timing is clear and actionable.

**Benefit:** Detects startup regressions and validates future improvements.

---

## Phase 2: Low-Risk Runtime Optimizations

### 2.1 Reduce ScreenBuffer Lock Churn

**Objective:** Reduce repeated `RwLock` acquisitions in the event loop.

**Why now:** Low architectural risk and improves maintainability before larger buffer changes.

**Dependencies:** Baseline metrics.

**Affected files:** `crates/forge-main/src/event_loop.rs`.

**Steps:**
1. Identify repeated adjacent `read()`/`write()` calls.
2. Group related operations under one lock.
3. Ensure locks are dropped before PTY writes and renderer calls.
4. Avoid changing behavior.
5. Add comments only where lock lifetime is intentional.

**Pitfalls:** Holding write locks too long; deadlocks with PTY response writes; changing cursor dirty semantics.

**Testing strategy:** `cargo test`; manual interactive smoke test; benchmark lock-heavy workloads if possible.

**Success criteria:** Behavior unchanged; fewer lock sites; no lock held during blocking IO/render submission.

**Benefit:** Cleaner event loop, lower synchronization overhead.

### 2.2 Printable ASCII Fast Path For VTE Parsing

**Objective:** Batch safe printable ASCII runs before falling back to `vte`.

**Why now:** It is one of the highest-probability CPU wins and depends on benchmarks.

**Dependencies:** Parser benchmarks; current `ScreenBuffer` behavior understood.

**Affected files:** `crates/forge-pty/src/vte_parser.rs`, `crates/forge-pty/src/screen_buffer.rs`.

**Steps:**
1. Add `ScreenBuffer` API for writing ASCII runs safely.
2. Scan input for control bytes and non-ASCII bytes.
3. For printable ASCII runs, batch insert while preserving wrapping, dirty rows, attributes.
4. For all other bytes, use existing `vte` parser.
5. Keep Unicode path unchanged initially.
6. Benchmark plain ASCII, mixed output, and escape-heavy output.

**Pitfalls:** Incorrect wrapping; mishandling tabs/newlines/ESC; breaking UTF-8; dirty rows not marked.

**Testing strategy:** Existing VTE tests; new tests for wrapping, mixed escape/plain text, Unicode fallback, SGR attributes across runs; benchmarks.

**Success criteria:** All parser tests pass; no behavior regressions; measurable improvement on plain ASCII workloads.

**Benefit:** Higher PTY throughput with limited complexity.

### 2.3 Reuse Tessellation Allocations

**Objective:** Ensure tessellation vectors retain capacity and avoid steady-state reallocations.

**Why now:** Low risk before deeper tessellator logic changes.

**Dependencies:** Tessellation benchmark.

**Affected files:** `crates/forge-renderer/src/grid_tessellator.rs`.

**Steps:**
1. Audit `vertices`, `bg_vertices`, and `fg_vertices` capacity behavior.
2. Replace allocation-prone paths with clear-and-reuse patterns.
3. Add capacity growth policy where needed.
4. Track vertex counts through tracing at debug/trace level.
5. Benchmark full redraw and dirty-row redraw.

**Pitfalls:** Excess memory retention after huge resize; hidden allocation in row resize.

**Testing strategy:** Renderer unit-level tests where possible; benchmark allocation counts if tooling supports it; app smoke test.

**Success criteria:** No new allocations in steady-state common redraw paths after warmup.

**Benefit:** Lower frame-time variance.

---

## Phase 3: Data Structure Improvements

### 3.1 Ring-Buffer Scrollback

**Objective:** Remove expensive front-removal behavior from scrollback.

**Why now:** It is the safest subset of the larger screen-buffer storage change.

**Dependencies:** Screen buffer tests and benchmarks.

**Affected files:** `crates/forge-pty/src/screen_buffer.rs`, possibly `crates/forge-pty/src/grid.rs` or `circular_grid.rs`.

**Steps:**
1. Introduce an internal scrollback abstraction with stable logical indexing.
2. Replace `Vec<Row>` scrollback front-removal with circular storage.
3. Preserve `visible_row()`, selection extraction, and resize/reflow behavior.
4. Add tests for scrollback overflow, viewport scrolling, clear scrollback, alt buffer.
5. Benchmark continuous output with large scrollback.

**Pitfalls:** Off-by-one errors in visible rows; selection over history; reflow losing history.

**Testing strategy:** Unit tests for scrollback boundaries; VTE integration tests; benchmarks.

**Success criteria:** No visible behavior change; bounded memory; no O(n) front removal.

**Benefit:** Better sustained output performance and scalability.

### 3.2 Primary Grid Ring Or Row Rotation

**Objective:** Avoid row movement during normal scrolling.

**Why now:** Builds on scrollback abstraction and targets common terminal output.

**Dependencies:** Ring-buffer scrollback complete.

**Affected files:** `crates/forge-pty/src/screen_buffer.rs`, maybe `circular_grid.rs`.

**Steps:**
1. Decide whether to reuse `CircularGrid` or introduce a focused row-ring abstraction.
2. Preserve logical row APIs for renderer and VTE operations.
3. Implement scroll-up/down region behavior carefully, including non-full-screen margins.
4. Keep alt-buffer behavior intact.
5. Benchmark continuous output and region scrolling.

**Pitfalls:** Scroll regions are more complex than full-screen scroll; resize/reflow interactions; dirty-row mapping.

**Testing strategy:** Tests for full-screen scroll, partial margins, insert/delete lines, alt buffer, reflow.

**Success criteria:** Scrolling avoids moving all rows in common full-screen case; all terminal behavior tests pass.

**Benefit:** Major reduction in memory movement during output bursts.

### 3.3 Improve Dirty-Row, Cursor, And Selection Invalidation

**Objective:** Rebuild and redraw only rows that actually changed.

**Why now:** More useful after buffer indexing is stable.

**Dependencies:** Grid/storage changes complete.

**Affected files:** `crates/forge-pty/src/screen_buffer.rs`, `crates/forge-renderer/src/grid_tessellator.rs`, `crates/forge-main/src/event_loop.rs`.

**Steps:**
1. Track previous cursor row and current cursor row precisely.
2. Track previous and current selection ranges.
3. Mark only affected rows dirty.
4. Ensure scroll, resize, theme changes still mark broad dirty ranges when required.
5. Benchmark cursor blink and selection changes.

**Pitfalls:** Stale cursor artifacts; stale selection highlights; missing dirty rows after scroll.

**Testing strategy:** Unit tests for dirty-row marking; visual/manual tests for cursor blink and selection.

**Success criteria:** Cursor blink does not trigger unnecessary full redraw; no visual stale state.

**Benefit:** Lower CPU use during idle blinking and selection interaction.

---

## Phase 4: Tessellation Structure

### 4.1 Table-Driven Box Drawing Metadata

**Objective:** Replace bulky box-drawing match with compact metadata lookup.

**Why now:** After dirty-row behavior is stable, optimize classification code safely.

**Dependencies:** Tessellation tests/benchmarks.

**Affected files:** `crates/forge-renderer/src/grid_tessellator.rs`, optional generated helper module.

**Steps:**
1. Define compact metadata type for stroke weights and rounded flag.
2. Build static Rust lookup table for box drawing range.
3. Replace `decode_box_drawing()` implementation.
4. Confirm procedural shader encoding is unchanged.
5. Benchmark box-heavy workloads.

**Pitfalls:** Incorrect Unicode offsets; missing characters; changing rendering semantics.

**Testing strategy:** Unit tests for representative box characters; compare old/new metadata during transition if possible.

**Success criteria:** Same rendering metadata with cleaner code; no regression in box drawing.

**Benefit:** Better maintainability and possible branch reduction.

### 4.2 Split Common ASCII And Procedural Paths

**Objective:** Make the common glyph path simpler and keep uncommon procedural rendering isolated.

**Why now:** Builds on lookup cleanup and keeps future SIMD possibilities narrow.

**Dependencies:** Box lookup complete.

**Affected files:** `crates/forge-renderer/src/grid_tessellator.rs`.

**Steps:**
1. Factor cell tessellation into small helpers: background, cursor/selection, ASCII glyph, procedural glyph, decorations.
2. Keep hot common path straightforward.
3. Avoid over-abstraction that adds virtual dispatch or allocations.
4. Rebenchmark full redraws.

**Pitfalls:** Refactor churn; accidental behavior changes; too many tiny helpers hurting readability.

**Testing strategy:** Existing rendering smoke tests; targeted unit tests for helper decisions; benchmarks.

**Success criteria:** Code is clearer; performance is neutral or better.

**Benefit:** Maintainable tessellator and better future optimization surface.

---

## Phase 5: Renderer Upload Efficiency

### 5.1 Add Render Diagnostics

**Objective:** Track vertex count, upload bytes, and frame timing.

**Why now:** Required before changing buffer upload strategy.

**Dependencies:** Tracing infrastructure.

**Affected files:** `crates/forge-renderer/src/renderer.rs`, maybe `grid_tessellator.rs`.

**Steps:**
1. Emit trace/debug counters for vertices generated and bytes uploaded.
2. Add timing around map/copy/unmap and submit/present where appropriate.
3. Keep logs gated to avoid normal overhead.

**Pitfalls:** Logging overhead; noisy output.

**Testing strategy:** Run with tracing filters; `cargo test`.

**Success criteria:** Diagnostics are available when enabled and quiet otherwise.

**Benefit:** Enables evidence-based renderer decisions.

### 5.2 Improve Vertex Buffer Growth Strategy

**Objective:** Make vertex buffer capacity predictable and avoid emergency reallocations.

**Why now:** Lower risk than persistent mapping and useful regardless.

**Dependencies:** Render diagnostics.

**Affected files:** `crates/forge-renderer/src/renderer.rs`.

**Steps:**
1. Calculate capacity based on viewport cells plus scrollbar/cursor/decorations.
2. Add conservative growth factor.
3. Avoid repeated destroy/recreate cycles during resizing.
4. Benchmark resize and large window redraw.

**Pitfalls:** Overallocating GPU memory; underestimating procedural/decorative vertices.

**Testing strategy:** Resize smoke tests; large grid benchmark.

**Success criteria:** No repeated buffer growth under normal resize; memory use remains reasonable.

**Benefit:** Fewer stalls and better resize behavior.

### 5.3 Evaluate Persistent Mapped Vertex Buffer

**Objective:** Reduce map/unmap overhead if profiling shows it matters.

**Why now:** Only after diagnostics prove this is worth doing.

**Dependencies:** Render diagnostics show map/unmap cost is meaningful.

**Affected files:** `crates/forge-renderer/src/renderer.rs`, maybe `texture.rs`.

**Steps:**
1. Design persistent mapping lifetime and ownership.
2. Ensure memory type remains host-visible and coherent or handle flushing correctly.
3. Use per-frame regions or ring buffering to avoid overwriting in-flight data.
4. Keep fallback simple.
5. Benchmark map/copy/render workloads.

**Pitfalls:** Vulkan synchronization bugs; stale data; memory flush errors; lifetime issues.

**Testing strategy:** Vulkan validation layers in debug; resize/render smoke tests; benchmarks.

**Success criteria:** Correct rendering under validation; measurable upload improvement.

**Benefit:** Lower CPU overhead in render submission path.

---

## Phase 6: Font And Config Robustness

### 6.1 Font Atlas Cache Discipline

**Objective:** Avoid unnecessary full atlas rebuilds and make font metadata explicit.

**Why now:** Font paths are fixed earlier; now atlas behavior can be tightened.

**Dependencies:** Font path cleanup.

**Affected files:** `crates/forge-renderer/src/font`, `crates/forge-main/src/main.rs`, `crates/forge-core/src/cache.rs`.

**Steps:**
1. Track selected font identity, size, bold identity, and atlas-relevant config.
2. Rebuild atlas only when those values change.
3. Add atlas build timing.
4. Consider lazy glyph insertion only if benchmark data supports it.

**Pitfalls:** Using stale atlas after config reload; cache invalidation complexity.

**Testing strategy:** Font config change tests where possible; startup/manual tests.

**Success criteria:** Atlas rebuild policy is explicit and correct.

**Benefit:** More predictable startup and reload performance.

### 6.2 Config Reload Discipline

**Objective:** Make live reload explicit, debounced, and safer.

**Why now:** Config changes can affect fonts, renderer, theme, keybindings, and runtime behavior.

**Dependencies:** Font/cache improvements.

**Affected files:** `crates/forge-config/src/watcher.rs`, `actor.rs`, `types.rs`, `crates/forge-main/src/event_loop.rs`.

**Steps:**
1. Define live-reloadable fields versus restart-required fields.
2. Add debounce to watcher reload events.
3. Represent config updates with changed categories or a diff-like structure.
4. Keep previous valid config on errors.
5. Improve error logging for Lua parse/extraction failures.

**Pitfalls:** Overcomplicated diff model; missed reloads from atomic file saves; reload storms.

**Testing strategy:** Config extraction tests; watcher behavior tests if feasible; manual edit-save smoke test.

**Success criteria:** Reload behavior is predictable; invalid config does not break current session.

**Benefit:** Safer runtime customization and cleaner extension path.

---

## Phase 7: Error Handling And Recovery

### 7.1 Replace Recoverable Runtime `unwrap()`s

**Objective:** Convert expected failures into typed errors or guarded branches.

**Why now:** Should happen after major flow changes so cleanup is targeted.

**Dependencies:** Earlier runtime architecture stable.

**Affected files:** `crates/forge-main/src/main.rs`, `event_loop.rs`, Wayland modules, renderer setup.

**Steps:**
1. Audit runtime `unwrap()`/`expect()` excluding tests and proven constants.
2. Categorize each as invariant, recoverable, or impossible.
3. Replace recoverable cases with `ForgeError` or graceful fallback.
4. Leave documented invariant assertions only where justified.
5. Add tests for reachable failure cases.

**Pitfalls:** Hiding real programmer bugs; noisy error plumbing.

**Testing strategy:** `cargo test`; targeted failure simulations where possible.

**Success criteria:** Runtime expected failures do not panic unnecessarily.

**Benefit:** Production-grade reliability.

### 7.2 Improve Wayland/Vulkan Error Context

**Objective:** Make platform/render failures diagnosable.

**Why now:** Complements recovery cleanup.

**Dependencies:** Runtime error handling audit.

**Affected files:** `crates/forge-main/src/wayland`, `crates/forge-renderer/src/*`, `crates/forge-core/src/error.rs`.

**Steps:**
1. Add context to missing optional/required Wayland globals.
2. Improve Vulkan device/surface/swapchain error messages.
3. Ensure surface loss and out-of-date swapchain paths remain handled.
4. Keep panic hook for unexpected bugs only.

**Pitfalls:** Overly verbose errors; changing error enums too broadly.

**Testing strategy:** Unit tests for error formatting where practical; manual no-Wayland/no-Vulkan failure checks if environment allows.

**Success criteria:** Failures clearly state subsystem and operation.

**Benefit:** Faster debugging and better user/developer experience.

---

## Phase 8: Targeted SIMD Policy Implementation

### 8.1 Decide Whether SIMD Is Justified

**Objective:** Use explicit SIMD only for proven hot kernels.

**Why now:** Only after algorithmic and structural improvements have landed.

**Dependencies:** Benchmarks show remaining bottleneck.

**Affected files:** Depends on selected kernel, likely `vte_parser.rs` first.

**Steps:**
1. Review latest profiles.
2. Pick one narrow kernel, preferably printable-byte scanning.
3. Implement Rust-native optimized version first.
4. Add CPU-feature-gated SIMD only if benchmark proves meaningful gain.
5. Keep portable fallback.
6. Document safety and dispatch behavior.

**Pitfalls:** CPU-specific bugs; marginal gains; increased maintenance.

**Testing strategy:** Run tests on fallback and SIMD paths where possible; benchmark on supported hardware.

**Success criteria:** Clear measured win with no portability regression.

**Benefit:** Captures remaining low-level gains without compromising architecture.

---

## Recommended Overall Order

1. Measurement baseline.
2. Startup cache and font path correctness.
3. Startup tracing.
4. Lock churn cleanup.
5. VTE ASCII fast path.
6. Tessellation allocation reuse.
7. Ring-buffer scrollback.
8. Primary grid scrolling optimization.
9. Dirty-row/cursor/selection invalidation.
10. Box drawing lookup.
11. Tessellator path cleanup.
12. Render diagnostics.
13. Vertex buffer growth.
14. Persistent mapping only if proven.
15. Font atlas rebuild discipline.
16. Config reload discipline.
17. Runtime error cleanup.
18. Wayland/Vulkan error context.
19. Targeted SIMD only if still justified.

This plan keeps the project stable after every optimization and avoids the main failure mode of performance work: stacking risky changes before proving the previous one was correct.
