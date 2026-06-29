-- ==============================================================================
-- Forge Terminal Configuration File
-- ==============================================================================
-- This is an example configuration file with maximum details and professional 
-- comments for every available option. Forge uses Lua for its configuration, 
-- which gives you the flexibility to define variables and script dynamic values.
--
-- Note: Forge will automatically hot-reload changes made to this file.

local config = {}

-- ==============================================================================
-- FONT CONFIGURATION
-- ==============================================================================
config.font = {
    -- The primary font family to use for rendering text.
    -- Forge uses fontconfig to resolve fonts on Linux.
    family = "monospace",

    -- The size of the font in points (pt).
    size = 14.0,

    -- (Optional) Specific font family to use for bold text. 
    -- If nil, Forge will try to automatically resolve the bold variant of the primary family.
    bold_family = nil,

    -- (Optional) Specific font family to use for italic text.
    -- If nil, Forge will try to automatically resolve the italic variant of the primary family.
    italic_family = nil,

    -- Enable or disable font ligatures (e.g., rendering '->' as a single arrow glyph).
    ligatures = true,

    -- Enable or disable Nerd Font support (automatically handles fallback glyphs if the 
    -- main font doesn't provide them).
    nerd_fonts = true,
}

-- ==============================================================================
-- WINDOW CONFIGURATION
-- ==============================================================================
config.window = {
    -- The initial width of the terminal window in logical pixels.
    width = 800,

    -- The initial height of the terminal window in logical pixels.
    height = 600,

    -- Background opacity. 1.0 is completely opaque, 0.0 is completely transparent.
    -- (Requires a Wayland compositor that supports transparent surfaces)
    opacity = 1.0,

    -- The initial title of the window. Note that the shell may dynamically override this.
    title = "Forge",

    -- Enable or disable Wayland client-side window decorations (title bar/borders).
    decorations = true,

    -- Inner padding between the window border and the terminal grid content.
    padding = {
        top = 4,
        bottom = 4,
        left = 4,
        right = 4,
    },

    -- Defines how excess space (due to the terminal grid size not perfectly aligning 
    -- with the window pixel size) is distributed.
    -- Options: 
    --  - "center" : Centers the grid, distributing excess padding equally to all sides.
    --  - "fill"   : Forces the grid to start from the exact specified padding (top-left).
    padding_balance = "center",
}

-- ==============================================================================
-- COMPOSITOR BLUR CONFIGURATION
-- ==============================================================================
config.blur = {
    -- Enable compositor-managed background blur. Forge does not render blur itself.
    enabled = false,

    -- Blur backend selection.
    -- Options:
    --  - "auto"     : Use a native compositor blur protocol if one is available.
    --  - "kde"      : Require KDE/KWin's org_kde_kwin_blur protocol.
    --  - "external" : Do not use app-side blur protocols; rely on compositor rules.
    --  - "off"      : Disable blur.
    method = "auto",

    -- Advisory only. Most Wayland blur protocols let the compositor choose blur strength.
    radius = 0,
}

-- ==============================================================================
-- CURSOR CONFIGURATION
-- ==============================================================================
config.cursor = {
    -- The shape of the terminal cursor.
    -- Options: "block", "underline", "beam"
    style = "block",

    -- Whether the cursor should blink when idle.
    blink = true,

    -- The blink interval rate in milliseconds (if blink is enabled).
    blink_rate_ms = 530,
}

-- ==============================================================================
-- SCROLLBACK CONFIGURATION
-- ==============================================================================
config.scrollback = {
    -- The maximum number of lines to keep in the scrollback history buffer.
    lines = 10000,

    -- Enable or disable smooth scrolling (if implemented by the renderer).
    smooth_scroll = true,

    -- The multiplier applied to mouse wheel scrolling events.
    -- Higher values will scroll more lines per wheel tick.
    scroll_multiplier = 3.0,
}

-- ==============================================================================
-- SHELL CONFIGURATION
-- ==============================================================================
config.shell = {
    -- The program to launch as the primary shell.
    -- If nil, Forge will default to the $SHELL environment variable or /bin/sh.
    program = "/bin/bash",

    -- Arguments to pass to the shell program upon startup.
    args = {},

    -- Additional environment variables to set for the shell session.
    -- Provide them as an array of key-value pair arrays: { {"KEY", "VALUE"}, ... }
    env = {
        -- {"TERM", "xterm-256color"},
        -- {"COLORTERM", "truecolor"},
    },
}

-- ==============================================================================
-- BEHAVIOR CONFIGURATION
-- ==============================================================================
config.behavior = {
    -- If true, selecting text with the mouse will automatically copy it to the 
    -- system clipboard.
    copy_on_select = false,

    -- If true, the mouse cursor will be hidden when you start typing, and 
    -- revealed again when the mouse is moved.
    hide_mouse_when_typing = false,

    -- If true, Forge's default internal keybindings will be disabled, 
    -- giving you total control over the keybinding table.
    disable_default_keybindings = false,
}

-- ==============================================================================
-- RENDER CONFIGURATION
-- ==============================================================================
config.render = {
    -- How Braille characters (U+2800 - U+28FF) are rendered.
    -- Options: 
    --  - "dots"  : Rendered precisely as standard Braille dots (default).
    --  - "solid" : Rendered as solid blocks to fill out pixel-perfect boundaries.
    braille_style = "dots",
}

-- ==============================================================================
-- KEYBINDINGS
-- ==============================================================================
-- Keybindings map a physical keystroke combination to a specific Action.
-- Valid Modifiers: "ctrl", "shift", "alt", "logo"
-- Valid Actions: "copy", "paste", "togglefullscreen"
-- 
-- Example: "ctrl+shift+c" maps to the "Copy" action.
config.keybindings = {
    ["ctrl+shift+c"] = "copy",
    ["ctrl+shift+v"] = "paste",
    ["f11"]          = "togglefullscreen",
    ["ctrl+enter"]   = "togglefullscreen",
}

-- ==============================================================================
-- THEME CONFIGURATION
-- ==============================================================================
-- Colors are defined in hexadecimal format (#RRGGBB or #RRGGBBAA).
-- If Alpha (AA) is omitted, it defaults to FF (fully opaque).
config.theme = {
    -- Primary window background color.
    background = "#1a1b26",
    
    -- Primary text foreground color.
    foreground = "#c0caf5",
    
    -- Color of the cursor.
    cursor_color = "#c0caf5",
    
    -- Background color of selected text.
    selection_bg = "#414868C8",
    
    -- The standard 16 ANSI colors used by terminal applications.
    ansi = {
        -- Normal Colors
        "#414868", -- 0: Black
        "#f7768e", -- 1: Red
        "#9ece6a", -- 2: Green
        "#e0af68", -- 3: Yellow
        "#7aa2f7", -- 4: Blue
        "#bb9af7", -- 5: Magenta
        "#7dcfff", -- 6: Cyan
        "#c0caf5", -- 7: White
        
        -- Bright Colors
        "#414868", -- 8: Bright Black
        "#f7768e", -- 9: Bright Red
        "#9ece6a", -- 10: Bright Green
        "#e0af68", -- 11: Bright Yellow
        "#7aa2f7", -- 12: Bright Blue
        "#bb9af7", -- 13: Bright Magenta
        "#7dcfff", -- 14: Bright Cyan
        "#c0caf5", -- 15: Bright White
    }
}

-- Finally, return the constructed configuration object to Forge.
return config
