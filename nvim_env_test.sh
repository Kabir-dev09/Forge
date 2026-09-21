#!/bin/bash
nvim --headless -c 'lua print("IPC: " .. tostring(vim.env.FORGE_IPC_SOCKET) .. " PANE: " .. tostring(vim.env.FORGE_PANE_ID))' -c 'q' > /tmp/nvim_env_test.log 2>&1
