# Forge SIMD Policy

Forge should use explicit SIMD only when a narrow kernel remains hot after algorithmic and data-structure improvements.

Current benchmark results do not justify adding CPU-specific VTE or tessellation SIMD:

- Printable PTY throughput improved materially with the Rust ASCII fast path and screen-buffer storage changes.
- Escape-heavy parsing still depends on correctness-sensitive VTE state handling, so bypassing it with SIMD would increase risk without a proven isolated bottleneck.
- Tessellation throughput is dominated by per-cell rendering decisions and vertex generation; recent allocation reuse and path cleanup keep this portable and maintainable.

Policy for future SIMD work:

1. Profile first and identify one specific hot loop.
2. Implement or validate a Rust-native scalar optimization before CPU-specific code.
3. Add SIMD only behind runtime CPU feature detection with a portable fallback.
4. Keep tests identical across fallback and SIMD paths.
5. Require benchmark evidence that the win is meaningful enough to offset maintenance and portability cost.

The best future SIMD candidate remains printable-byte scanning in `forge-pty`, but only if profiling shows the scalar fast path is still a limiting factor.
