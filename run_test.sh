#!/bin/bash
export RUST_LOG=forge_main=debug
./target/release/forge-main -e nvim --headless -c 'sleep 1' -c 'e /tmp/testfile2' -c 'sleep 1' -c 'q' > /tmp/forge_test.log 2>&1
