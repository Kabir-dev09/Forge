#!/bin/bash
nvim --headless -c 'lua print("Is Forge-Integrations loaded? " .. tostring(package.loaded["forge-integrations"] ~= nil))' -c 'q' > /tmp/nvim_plugin_test.log 2>&1
