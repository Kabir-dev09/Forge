use forge_pty::{ScreenBuffer, Row, Cell};
use forge_core::color::Color;
use std::time::Instant;
use std::collections::VecDeque;

fn main() {
    let mut sb = ScreenBuffer::new(160, 40, 100_000, Color::WHITE, Color::BLACK);
    
    // Hack: populate grid and scrollback to simulate 50k lines
    for i in 0..50_000 {
        let mut row = Row {
            cells: vec![sb.default_cell(); 160].into_boxed_slice(),
            len: 10,
            wrapped: false,
            reflowable: true,
        };
        // Just put some chars
        for c in 0..10 {
            row.cells[c].c = 'A';
        }
        sb.push_row(row);
    }
    
    let start = Instant::now();
    sb.resize_reflow(80, 40);
    println!("resize_reflow (160 -> 80, 50k lines) took: {:?}", start.elapsed());
}
