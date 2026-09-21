#!/bin/bash
export RUST_LOG=forge_main=info
cargo build --release --bin forge-main
# We will use xdotool or just rely on log inspection to see if FocusBuffer is sent
