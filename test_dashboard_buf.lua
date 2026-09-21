local function dump()
    local f = io.open("/tmp/nvim_bufs.txt", "w")
    local bufs = vim.api.nvim_list_bufs()
    for _, b in ipairs(bufs) do
        f:write("Buf " .. b .. " listed: " .. tostring(vim.bo[b].buflisted) .. " type: " .. vim.bo[b].buftype .. " ft: " .. vim.bo[b].filetype .. " name: " .. vim.api.nvim_buf_get_name(b) .. "\n")
    end
    f:close()
end
vim.defer_fn(function()
    dump()
    vim.cmd("qall!")
end, 1000)
