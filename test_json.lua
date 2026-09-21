local line = '{"type": "focus_buffer", "buffer_id": 1}'
local ok, msg = pcall(vim.fn.json_decode, line)
print(ok, msg)
