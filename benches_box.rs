use forge_pty::Cell;
use forge_core::color::Color;
use std::time::Instant;

fn main() {
    let default_cell = Cell {
        c: ' ',
        fg: Color::WHITE,
        bg: Color::BLACK,
        flags: 0,
    };
    let start = Instant::now();
    let mut vec = Vec::with_capacity(100_000);
    for _ in 0..100_000 {
        vec.push(vec![default_cell; 80].into_boxed_slice());
    }
    println!("100k vec![...].into_boxed_slice() took: {:?}", start.elapsed());
}
