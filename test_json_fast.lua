local uv = vim.uv or vim.loop
local timer = uv.new_timer()
timer:start(10, 0, function()
    local ok, msg = pcall(vim.json.decode, '{"type": "focus_buffer", "buffer_id": 1}')
    print("FAST EVENT:", ok, msg)
end)
