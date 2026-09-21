#!/bin/bash
export RUST_LOG=forge_main=info
rm -f /tmp/nvim_ipc_cmd.log
# We will run Forge, use xdotool to switch tabs
./target/release/forge-main -e nvim /tmp/file1 &
FORGE_PID=$!
sleep 2

# Open another file in nvim
xdotool search --class forge windowfocus
xdotool type ":e /tmp/file2"
xdotool key Return
sleep 2

# Switch to the first tab using ctrl+1 (or whatever binding)
xdotool key ctrl+1
sleep 1

kill $FORGE_PID
cat /tmp/nvim_ipc_cmd.log
