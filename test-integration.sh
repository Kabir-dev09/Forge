#!/bin/bash
export NVIM_APPNAME="forge-nvim"
# Ensure the plugin is available
mkdir -p ~/.config/forge-nvim
cat << 'LUA' > ~/.config/forge-nvim/init.lua
vim.opt.rtp:append("/home/kabir/PROJECTS/forge-integrations")
require('forge-integrations').setup()
LUA

./target/release/forge-main -e nvim
