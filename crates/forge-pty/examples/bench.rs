use forge_pty::{ScreenBuffer, VteProcessor};
use std::fs::File;
use std::io::Read;
use std::time::Instant;

fn main() {
    let mut data = Vec::new();
    // Use the actual file t
    let mut f = File::open("../../t").unwrap();
    f.read_to_end(&mut data).unwrap();
    println!("Data size: {} MB", data.len() / 1024 / 1024);

    let mut processor = VteProcessor::new();
    let mut buffer = ScreenBuffer::new(
        200,
        50,
        10000,
        forge_core::color::Color::WHITE,
        forge_core::color::Color::BLACK,
    ); // realistic terminal size

    let start = Instant::now();
    let mut offset = 0;

    let mut process_time = std::time::Duration::ZERO;
    let mut other_time = std::time::Duration::ZERO;

    while offset < data.len() {
        let chunk_end = (offset + 128 * 1024).min(data.len());

        let t0 = Instant::now();
        processor.process(&data[offset..chunk_end], &mut buffer);
        process_time += t0.elapsed();

        let t1 = Instant::now();
        buffer.view_scroll_to_bottom();
        other_time += t1.elapsed();

        offset = chunk_end;
    }
    let elapsed = start.elapsed();

    println!("Processed {} MB in {:?}", data.len() / 1024 / 1024, elapsed);
    println!("  process_time: {:?}", process_time);
    println!("  other_time: {:?}", other_time);
    println!(
        "Speed: {:.2} MB/s",
        (data.len() as f64 / 1024.0 / 1024.0) / elapsed.as_secs_f64()
    );
}
