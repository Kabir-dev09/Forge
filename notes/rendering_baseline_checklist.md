# Rendering Baseline Checklist

Use this checklist before and after each rendering optimization. Keep the terminal size,
font, DPI scale, compositor, and workload inputs the same between runs.

## Runtime Counters

Run Forge with render counters enabled:

```text
FORGE_RENDER_STATS=1 cargo run -p forge-main
```

The renderer logs cumulative submitted frames, dirty rows, uploaded vertices, and uploaded
bytes on the first frame and then every 120 submitted frames.

## Repeatable Workloads

1. Idle shell
   - Open Forge and leave it idle for at least 60 seconds.
   - Record observed CPU usage with cursor blinking enabled and disabled.

2. Passive mouse motion
   - Move the pointer rapidly inside an idle shell with no scrollback.
   - Repeat with scrollback present and the scrollbar hidden.
   - Confirm passive motion does not submit frames unless a visible overlay changes.

3. Fast terminal output
   - Run a large text output workload, such as `cat` on a large source file or log.
   - Record frame stats and subjective smoothness.

4. Build output
   - Run `cargo build` from the repository root.
   - Record frame stats during sustained output and after output stops.

5. TUI workloads
   - Test `nvim`, `vim`, `btop`, `htop`, and `tmux`.
   - Check cursor blink, scrolling panes, box drawing, block characters, and Nerd Font symbols.

6. Resize
   - Resize the window continuously and in small increments.
   - Confirm no stale pixels, missing rows, blurry text changes, or scrollbar regressions.

## Required Commands

```text
cargo test --workspace
cargo bench -p forge-renderer --bench tessellation_baseline
```

If a benchmark cannot run in the current environment, record the failure reason with the
optimization result.
