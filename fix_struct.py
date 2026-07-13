with open('crates/forge-pty/src/screen_buffer.rs', 'r') as f:
    content = f.read()

bad_block = '''    pub attr_italic: bool,
    pub attr_underline: bool,
    pub attr_strikethrough: bool,
    pub scroll_offset: usize,
    pub pending_scroll: Option<ScrollEvent>,
    pub scroll_id: u64,
    pub use_alt_buffer: bool,'''

good_block = '''    pub attr_italic: bool,
    pub attr_underline: bool,
    pub attr_strikethrough: bool,
    pub palette: [Color; 16],
    pub saved_cursor: Option<CursorPos>,
    pub scroll_offset: usize,
    pub pending_scroll: Option<ScrollEvent>,
    pub scroll_id: u64,
    pub use_alt_buffer: bool,'''

content = content.replace(bad_block, good_block)

with open('crates/forge-pty/src/screen_buffer.rs', 'w') as f:
    f.write(content)

