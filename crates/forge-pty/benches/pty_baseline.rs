use std::hint::black_box;
use std::time::{Duration, Instant};

use forge_core::color::Color;
use forge_pty::{ScreenBuffer, VteProcessor};

const BENCH_DURATION: Duration = Duration::from_millis(250);

fn make_screen(cols: usize, rows: usize) -> ScreenBuffer {
    ScreenBuffer::new(
        cols,
        rows,
        10_000,
        Color {
            r: 192,
            g: 202,
            b: 245,
            a: 255,
        },
        Color {
            r: 26,
            g: 27,
            b: 38,
            a: 255,
        },
    )
}

fn run_for_duration<F>(name: &str, bytes_per_iter: usize, mut f: F)
where
    F: FnMut(),
{
    let start = Instant::now();
    let mut iterations = 0_u64;

    while start.elapsed() < BENCH_DURATION {
        f();
        iterations += 1;
    }

    let elapsed = start.elapsed();
    let bytes = iterations as f64 * bytes_per_iter as f64;
    let mib_per_sec = if elapsed.as_secs_f64() > 0.0 {
        bytes / elapsed.as_secs_f64() / (1024.0 * 1024.0)
    } else {
        0.0
    };

    println!(
        "{name}: {iterations} iterations in {:.3?} ({:.2} MiB/s)",
        elapsed, mib_per_sec
    );
}

fn bench_vte_plain_ascii() {
    let payload = b"cargo check finished successfully\n".repeat(1024);
    let mut processor = VteProcessor::new();
    let mut screen = make_screen(120, 40);

    run_for_duration("vte_plain_ascii", payload.len(), || {
        let responses = processor.process(black_box(&payload), black_box(&mut screen));
        black_box(responses);
    });
}

fn bench_vte_escape_heavy() {
    let payload = b"\x1b[31merror\x1b[0m: message\n\x1b[32mok\x1b[0m\n".repeat(1024);
    let mut processor = VteProcessor::new();
    let mut screen = make_screen(120, 40);

    run_for_duration("vte_escape_heavy", payload.len(), || {
        let responses = processor.process(black_box(&payload), black_box(&mut screen));
        black_box(responses);
    });
}

fn bench_screen_scroll_and_clear() {
    let mut screen = make_screen(160, 60);
    let line = "0123456789abcdef ".repeat(12);

    run_for_duration("screen_scroll_and_clear", line.len(), || {
        screen.write_str(black_box(&line));
        screen.line_feed();
        if screen.scrollback_len() % 128 == 0 {
            screen.erase_screen();
        }
        black_box(screen.cursor);
    });
}

fn bench_screen_reflow() {
    let mut wide = make_screen(160, 60);
    let line = "reflow benchmark line with enough content to wrap across several widths ";
    for _ in 0..500 {
        wide.write_str(line);
        wide.line_feed();
    }

    run_for_duration("screen_resize_reflow", 160 * 60, || {
        let mut screen = wide.clone();
        screen.resize_reflow(100, 50);
        screen.resize_reflow(160, 60);
        black_box(screen.cursor);
    });
}

fn main() {
    println!("Forge PTY baseline benchmarks");
    bench_vte_plain_ascii();
    bench_vte_escape_heavy();
    bench_screen_scroll_and_clear();
    bench_screen_reflow();
}
