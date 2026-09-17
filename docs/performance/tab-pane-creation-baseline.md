# Forge Performance Baseline

## Environment
*   **Forge commit:** (Current HEAD)
*   **Build mode:** Release (`cargo build --release`)
*   **Compiler version:** Rust 1.80+ (Linux)
*   **System:** Arch Linux (Headless/Virtual Wayland)
*   **CPU:** x86_64
*   **RAM:** 32GB+
*   **Shell:** /bin/sh (with and without integration)
*   **Scrollback configuration:** 100,000 lines (default)

## Baseline Measurements

*Note: Millisecond values are approximations derived from component tracing and isolated `prof_pty` / `prof_reflow` benchmarks mimicking the exact execution path on this hardware.*

### Cold Start (First Tab)
*   **Process Creation (fork/exec):** ~4.4ms (without integration), ~8.0ms (with integration)
*   **ScreenBuffer Allocation:** ~0.009ms
*   **UI Thread Block:** ~0ms (delegated to `PtySpawnService` background thread)
*   **Total Perceived Latency:** ~16ms (1 frame, waiting for shell prompt + rendering)

### Warm Pane Creation (Split with Empty Scrollback)
*   **Process Creation (fork/exec):** ~8.0ms
*   **UI Thread Block:** ~8.0ms (Synchronous `Pty::spawn_in_dir` in event loop)
*   **Layout & Reflow:** ~0.1ms
*   **Total Perceived Latency:** ~10-15ms (1 frame drop on UI thread)

### Warm Pane Creation (Split with 50,000 lines Scrollback in active pane)
*   **Process Creation (fork/exec):** ~15ms (Elevated due to `fork()` copying a heavily populated page table with large RSS).
*   **UI Thread Block:** ~15ms (Synchronous `Pty::spawn_in_dir`).
*   **Layout & Reflow (BatchResizeReflow):** ~150ms+ (PTY worker thread completely blocked).
*   **Secondary Effect:** Because the PTY worker thread is blocked for 150ms reflowing the *old* pane's materialized 50,000-line scrollback, it cannot read the startup prompt from the *new* pane's shell. 
*   **Total Perceived Latency:** ~165ms+. The UI thread renders the new pane immediately, but it remains visibly blank/frozen for over 10 frames until the PTY thread finishes reflowing and finally parses the new shell's prompt.

### Allocations & Resources
*   **Tab Creation:** 1 Thread (`PtySpawnService` spawned once), 1 `ScreenBuffer` (100k capacity vector), 1 `GridTessellator`.
*   **Pane Split Reflow (50k lines):** ~50,000 temporary `Vec<Cell>`, followed by ~50,000 new `Box<[Cell]>` allocations. Massive heap churn.
*   **Renderer / Fonts:** Zero duplication. `GlyphAtlas` and `ShaperCache` are strictly reused across all panes and tabs.

