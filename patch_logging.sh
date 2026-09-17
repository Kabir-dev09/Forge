#!/bin/bash
sed -i '1i use std::io::Write;' crates/forge-main/src/event_loop.rs
sed -i 's/match forge_pty::Pty::spawn_in_dir(/let start_t = std::time::Instant::now();\n                match forge_pty::Pty::spawn_in_dir(/g' crates/forge-main/src/event_loop.rs
