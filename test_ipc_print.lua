local uv = vim.uv or vim.loop
print("Connecting...")
local pipe = uv.new_pipe(false)
pipe:connect(os.getenv("FORGE_IPC_SOCKET"), function(err)
    if err then print("Err: " .. err) else print("Connected callback!") end
end)
print("Connected scheduled!")
