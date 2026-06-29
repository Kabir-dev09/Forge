# Forge Performance Baseline

This document describes the baseline measurement workflow for the optimization roadmap.

Run these commands from the repository root in release mode:

```bash
cargo bench -p forge-pty --bench pty_baseline
cargo bench -p forge-renderer --bench tessellation_baseline
```

The benchmark binaries are dependency-free `harness = false` benches. They print simple throughput or iteration counts that are intended for before/after comparison while implementing one optimization at a time.

## PTY Benchmarks

`crates/forge-pty/benches/pty_baseline.rs` covers:

- Plain ASCII VTE processing.
- Escape-heavy VTE processing.
- Screen scrolling and clearing.
- Resize/reflow behavior.

These workloads are not a substitute for interactive testing, but they make parser and screen-buffer changes measurable.

## Renderer Tessellation Benchmarks

`crates/forge-renderer/benches/tessellation_baseline.rs` covers:

- Full-grid tessellation.
- Cursor-blink dirty-row tessellation.

The renderer benchmark does not create a Vulkan device. It measures CPU-side tessellation only, which keeps the baseline usable on machines without an active Wayland/Vulkan session.

## Tracing

Forge now includes profiling-oriented tracing spans around:

- Startup phases in `forge-main`.
- PTY parser batches.
- Screen-buffer scroll, clear, and reflow operations.
- Grid tessellation.
- Vertex upload and Vulkan submit/present boundaries.

Enable trace-level profiling with a focused filter, for example:

```bash
RUST_LOG=forge=trace cargo run -p forge-main --release
```

For broad profiling, prefer external tools such as `perf` or flamegraphs and use the tracing spans to interpret subsystem boundaries.

## Completion Gate For Future Optimizations

Before changing a hot path, capture the relevant baseline benchmark output. After the optimization, rerun the same command and compare:

- Throughput or iteration count.
- Correctness tests.
- Any relevant manual smoke test.

Only one optimization should be measured at a time.
