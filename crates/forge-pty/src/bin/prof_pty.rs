use forge_pty::{ScreenBuffer, pty::Pty, Winsize};
use forge_core::config_registry::ShellConfig;
use forge_core::color::Color;
use std::time::Instant;

fn main() {
    let cols = 100;
    let rows = 40;
    
    // Measure ScreenBuffer creation
    let start = Instant::now();
    for _ in 0..100 {
        let _sb = ScreenBuffer::new(cols, rows, 100_000, Color::WHITE, Color::BLACK);
    }
    println!("ScreenBuffer::new x100: {:?}", start.elapsed());

    // Measure Pty::spawn_in_dir
    let shell = ShellConfig {
        program: "/bin/sh".to_string(),
        args: vec!["-c".to_string(), "exit 0".to_string()],
        integration_enabled: false,
        ..ShellConfig::default()
    };
    
    let winsize = Winsize {
        ws_row: rows as u16,
        ws_col: cols as u16,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    let start = Instant::now();
    for _ in 0..10 {
        let pty = Pty::spawn_in_dir(&shell, winsize, None, None).unwrap();
        pty.terminate_and_reap();
    }
    println!("Pty::spawn_in_dir (integration=false) x10: {:?}", start.elapsed());

    let shell_integ = ShellConfig {
        program: "/bin/bash".to_string(),
        args: vec![],
        integration_enabled: true,
        ..ShellConfig::default()
    };

    let start = Instant::now();
    for _ in 0..10 {
        let pty = Pty::spawn_in_dir(&shell_integ, winsize, None, None).unwrap();
        pty.terminate_and_reap();
    }
    println!("Pty::spawn_in_dir (integration=true) x10: {:?}", start.elapsed());
}
