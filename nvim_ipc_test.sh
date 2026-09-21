#!/bin/bash
nvim --headless -c 'lua print(vim.env.FORGE_IPC_SOCKET)' -c 'lua require("forge-integrations.connection").connect(vim.env.FORGE_IPC_SOCKET, tonumber(vim.env.FORGE_PANE_ID))' -c 'sleep 1' -c 'q' > /tmp/nvim_ipc_test.log 2>&1
