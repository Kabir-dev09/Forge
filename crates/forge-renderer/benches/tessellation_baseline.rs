use std::collections::HashMap;
use std::hint::black_box;
use std::time::{Duration, Instant};

use forge_core::cell::Cell;
use forge_core::color::Color;
use forge_core::config_registry::CursorStyle;
use forge_renderer::font::atlas::{GlyphAtlas, GlyphAtlasDescriptor, GlyphMetrics};
use forge_renderer::grid_tessellator::GridTessellator;

const BENCH_DURATION: Duration = Duration::from_millis(250);

fn run_for_duration<F>(name: &str, mut f: F)
where
    F: FnMut() -> usize,
{
    let start = Instant::now();
    let mut iterations = 0_u64;
    let mut vertices = 0_usize;

    while start.elapsed() < BENCH_DURATION {
        vertices = black_box(f());
        iterations += 1;
    }

    let elapsed = start.elapsed();
    println!(
        "{name}: {iterations} iterations in {:.3?} (last vertex count: {vertices})",
        elapsed
    );
}


fn grid(cols: usize, rows: usize) -> Vec<Vec<Cell>> {
    let fg = Color {
        r: 192,
        g: 202,
        b: 245,
        a: 255,
    };
    let bg = Color {
        r: 26,
        g: 27,
        b: 38,
        a: 255,
    };
    let accent = Color {
        r: 65,
        g: 72,
        b: 104,
        a: 255,
    };
    let chars = b"abcdefghijklmnopqrstuvwxyz0123456789";

    (0..rows)
        .map(|row| {
            (0..cols)
                .map(|col| {
                    let mut cell = Cell {
                        c: chars[(row + col) % chars.len()] as char,
                        fg,
                        bg: if col % 17 == 0 { accent } else { bg },
                        flags: 0,
                    };
                    if col % 23 == 0 {
                        cell.set_bold(true);
                    }
                    cell
                })
                .collect()
        })
        .collect()
}

fn tessellate_once(
    tessellator: &mut GridTessellator,
    atlas: &GlyphAtlas,
    grid: &[Vec<Cell>],
    dirty_rows: &[u64],
    cursor_visible: bool,
) -> usize {
    let refs: Vec<&[Cell]> = grid.iter().map(Vec::as_slice).collect();

    tessellator.tessellate(
        black_box(&refs),
        black_box(dirty_rows),
        black_box(atlas),
        10.0,
        20.0,
        10.0,
        20.0,
        16.0,
        1200.0,
        800.0,
        [0.01, 0.01, 0.01, 1.0],
        [0.8, 0.8, 0.8, 1.0],
        Some((10, 10)),
        CursorStyle::Block,
        cursor_visible,
        None,
        [0.1, 0.1, 0.2, 0.8],
        4.0,
        4.0,
        None,
        None,
        false,
        None,
        0,
        None,
    );

    tessellator.vertices.len()
}

fn bench_full_grid() {
    let atlas = GlyphAtlas::dummy_for_bench();
    let grid = grid(120, 40);
    let dirty_rows = vec![1; 40];
    let mut tessellator = GridTessellator::new(120 * 40);

    run_for_duration("tessellate_full_grid", || {
        tessellate_once(&mut tessellator, &atlas, &grid, &dirty_rows, true)
    });
}

fn bench_cursor_blink_dirty_rows() {
    let atlas = GlyphAtlas::dummy_for_bench();
    let grid = grid(120, 40);
    let dirty_rows = vec![0; 40];
    let mut tessellator = GridTessellator::new(120 * 40);

    let mut cursor_visible = true;
    run_for_duration("tessellate_cursor_blink", || {
        cursor_visible = !cursor_visible;
        tessellate_once(&mut tessellator, &atlas, &grid, &dirty_rows, cursor_visible)
    });
}

fn main() {
    println!("Forge renderer tessellation baseline benchmarks");
    bench_full_grid();
    bench_cursor_blink_dirty_rows();
}
