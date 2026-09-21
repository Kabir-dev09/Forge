#!/bin/bash
nvim --headless -c 'print("IPC: " .. tostring(vim.env.FORGE_IPC_SOCKET))' -c 'q' > /tmp/nvim_test.log 2>&1
