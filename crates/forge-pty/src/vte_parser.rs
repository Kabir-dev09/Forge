use crate::screen_buffer::ScreenBuffer;
use forge_core::color::ansi_256_color;
use forge_core::color::Color;
use vte::{Params, Parser, Perform};

pub struct TerminalPerformer<'a> {
    buffer: &'a mut ScreenBuffer,
    charsets: &'a mut CharsetState,
    parser_is_ground: &'a mut bool,
    responses: Vec<u8>,
    command_events: &'a mut Vec<CommandLifecycleEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandLifecycleEvent {
    Started { command: Option<String> },
    Finished { exit_code: i32 },
}

impl<'a> Perform for TerminalPerformer<'a> {
    fn print(&mut self, c: char) {
        *self.parser_is_ground = true;
        if c >= '\x20' {
            let c = self.charsets.translate(c);
            let mut buf = [0; 4];
            self.buffer.write_grapheme(c.encode_utf8(&mut buf));
        }
    }

    fn execute(&mut self, byte: u8) {
        *self.parser_is_ground = true;
        match byte {
            0x07 => tracing::trace!("BEL received"),
            0x08 => self.buffer.move_cursor_relative(0, -1),
            0x09 => {
                let next_tab = ((self.buffer.cursor.col / 8) + 1) * 8;
                self.buffer.move_cursor_to(
                    self.buffer.cursor.row,
                    next_tab.min(self.buffer.cols().saturating_sub(1)),
                );
            }
            0x0A => self.buffer.line_feed(),
            0x0D => self.buffer.carriage_return(),
            0x0E => self.charsets.gl = GraphicSet::G1,
            0x0F => self.charsets.gl = GraphicSet::G0,
            _ => tracing::trace!("Unhandled execute: 0x{:02X}", byte),
        }
    }

    fn hook(&mut self, _params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        *self.parser_is_ground = false;
        
        // DECRQSS (Request Selection or Setting)
        if action == 'q' && intermediates == [b'$'] {
            // Reply with "invalid" to avoid timeouts for unsupported settings requests (like SGR 'm')
            self.responses.extend_from_slice(b"\x1bP0$r\x1b\\");
        }
    }

    fn put(&mut self, _byte: u8) {
        *self.parser_is_ground = false;
        tracing::trace!("put");
    }

    fn unhook(&mut self) {
        *self.parser_is_ground = true;
        tracing::trace!("unhook");
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        *self.parser_is_ground = true;
        if params.is_empty() {
            return;
        }

        match params[0] {
            b"0" | b"2" => {
                if params.len() > 1 {
                    if let Ok(title) = std::str::from_utf8(params[1]) {
                        self.buffer.current_title = title.to_string();
                    }
                }
            }
            b"7" => {
                if params.len() > 1 {
                    if let Ok(uri) = std::str::from_utf8(params[1]) {
                        if let Some(path) = uri.strip_prefix("file://") {
                            // simple URL decoding could be added here, but typically path is enough
                            let path = if let Some(slash_idx) = path.find('/') {
                                &path[slash_idx..]
                            } else {
                                path
                            };
                            self.buffer.current_dir = Some(path.to_string());
                        }
                    }
                }
            }
            b"133" => {
                if params.len() > 1 {
                    match params[1] {
                        b"A" => {
                            self.buffer.is_command_running = false;
                            self.buffer.current_command = None;
                        }
                        b"C" => {
                            self.buffer.is_command_running = true;
                            self.buffer.last_exit_code = None;
                            self.buffer.current_command = params.get(2).and_then(|value| {
                                std::str::from_utf8(value).ok().map(str::to_string)
                            });
                            self.command_events.push(CommandLifecycleEvent::Started {
                                command: self.buffer.current_command.clone(),
                            });
                        }
                        b"D" => {
                            self.buffer.is_command_running = false;
                            self.buffer.current_command = None;
                            let mut exit_code = 0;
                            if params.len() > 2 {
                                if let Ok(code_str) = std::str::from_utf8(params[2]) {
                                    if let Ok(code) = code_str.parse::<i32>() {
                                        self.buffer.last_exit_code = Some(code);
                                        exit_code = code;
                                    }
                                }
                            }
                            self.command_events
                                .push(CommandLifecycleEvent::Finished { exit_code });
                        }
                        _ => {}
                    }
                }
            }
            b"10" | b"11" => {
                if params.len() > 1 && params[1] == b"?" {
                    let is_bg = params[0] == b"11";
                    let color = if is_bg {
                        self.buffer.default_bg
                    } else {
                        self.buffer.default_fg
                    };
                    // format: \x1b]11;rgb:RRRR/GGGG/BBBB\x1b\\
                    let r16 = ((color.r as u16) << 8) | (color.r as u16);
                    let g16 = ((color.g as u16) << 8) | (color.g as u16);
                    let b16 = ((color.b as u16) << 8) | (color.b as u16);
                    let code = if is_bg { 11 } else { 10 };
                    let response = format!("\x1b]{};rgb:{:04x}/{:04x}/{:04x}\x1b\\", code, r16, g16, b16);
                    self.responses.extend_from_slice(response.as_bytes());
                }
            }
            _ => tracing::trace!("osc_dispatch {:?}", params),
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        *self.parser_is_ground = true;
        let p0 = get_param_or(params, 0, 1) as i32;
        let p1 = get_param_or(params, 1, 1) as i32;

        match action {
            'A' => self.buffer.move_cursor_relative(-p0, 0),
            'B' => self.buffer.move_cursor_relative(p0, 0),
            'C' => self.buffer.move_cursor_relative(0, p0),
            'D' => self.buffer.move_cursor_relative(0, -p0),
            'E' => self
                .buffer
                .move_cursor_to(self.buffer.cursor.row.saturating_add(p0 as usize), 0),
            'F' => self
                .buffer
                .move_cursor_to(self.buffer.cursor.row.saturating_sub(p0 as usize), 0),
            'G' => self
                .buffer
                .move_cursor_to(self.buffer.cursor.row, (p0 - 1).max(0) as usize),
            'H' | 'f' => self
                .buffer
                .move_cursor_to((p0 - 1).max(0) as usize, (p1 - 1).max(0) as usize),
            'd' => self
                .buffer
                .move_cursor_to((p0 - 1).max(0) as usize, self.buffer.cursor.col),
            'J' => match get_param_or(params, 0, 0) {
                0 => self.buffer.erase_to_end_of_screen(),
                1 => {
                    self.buffer.erase_to_start_of_screen();
                }
                2 => self.buffer.erase_screen(),
                3 => self.buffer.clear_scrollback(),
                _ => {}
            },
            'K' => match get_param_or(params, 0, 0) {
                0 => self.buffer.erase_to_end_of_line(),
                1 => self.buffer.erase_to_start_of_line(),
                2 => self.buffer.erase_line(),
                _ => {}
            },
            'L' => self.buffer.insert_lines(p0 as usize),
            'M' => self.buffer.delete_lines(p0 as usize),
            'P' => self.buffer.delete_chars(p0 as usize),
            'S' => self.buffer.scroll_up_in_region(p0 as usize),
            'T' => self.buffer.scroll_down_in_region(p0 as usize),
            'X' => self.buffer.erase_chars(p0 as usize),
            '@' => self.buffer.insert_chars(p0 as usize),
            'm' => {
                if intermediates.is_empty() {
                    handle_sgr(params, self.buffer);
                } else {
                    tracing::trace!(
                        "Unhandled CSI private 'm' with intermediates: {:?}",
                        intermediates
                    );
                }
            }
            'r' => {
                let top = (p0 - 1).max(0) as usize;
                let bottom = (p1 - 1).max(0) as usize;
                let bottom = if bottom == 0 {
                    self.buffer.rows().saturating_sub(1)
                } else {
                    bottom.min(self.buffer.rows().saturating_sub(1))
                };
                self.buffer.margin_top = top;
                self.buffer.margin_bottom = bottom;
                self.buffer.move_cursor_to(0, 0);
            }
            'c' => {
                if intermediates.is_empty() || intermediates == [b'0'] {
                    // Send Primary Device Attributes: \x1b[?1;2c (VT100 with Advanced Video Option)
                    self.responses.extend_from_slice(b"\x1b[?1;2c");
                }
            }
            'n' => {
                if p0 == 5 {
                    // Device Status Report (DSR) - Operating Status
                    self.responses.extend_from_slice(b"\x1b[0n");
                } else if p0 == 6 {
                    // Device Status Report (DSR) - report cursor position
                    // Format: ESC [ <row> ; <col> R (1-indexed)
                    let row = self.buffer.cursor.row + 1;
                    let col = self.buffer.cursor.col + 1;
                    let response = format!("\x1b[{};{}R", row, col);
                    self.responses.extend_from_slice(response.as_bytes());
                }
            }
            'p' => {
                if intermediates == [b'!'] {
                    // DECSTR soft reset: cursor shape returns to the terminal profile default.
                    self.buffer.clear_cursor_style_override();
                    self.buffer.set_cursor_visible(true);
                } else {
                    tracing::trace!(
                        "Unhandled CSI private 'p' with intermediates: {:?}",
                        intermediates
                    );
                }
            }
            't' => {
                let p0 = get_param_or(params, 0, 0);
                match p0 {
                    14 => {
                        // Report text area size in pixels.
                        // We use a dummy estimate (16x8 cell size) since pixel size isn't strictly tracked in the parser.
                        let height = self.buffer.rows() * 16;
                        let width = self.buffer.cols() * 8;
                        let response = format!("\x1b[4;{};{}t", height, width);
                        self.responses.extend_from_slice(response.as_bytes());
                    }
                    16 => {
                        // Report character cell size in pixels.
                        let response = "\x1b[6;16;8t".to_string();
                        self.responses.extend_from_slice(response.as_bytes());
                    }
                    18 => {
                        // Report text area size in characters.
                        let height = self.buffer.rows();
                        let width = self.buffer.cols();
                        let response = format!("\x1b[8;{};{}t", height, width);
                        self.responses.extend_from_slice(response.as_bytes());
                    }
                    _ => {
                        tracing::trace!("Unhandled CSI 't' param: {}", p0);
                    }
                }
            }
            'h' | 'l' => handle_mode(params, intermediates, action, self.buffer),
            'q' => {
                if intermediates.first() == Some(&b' ') {
                    let p0 = get_param_or(params, 0, 0);
                    match p0 {
                        0 => {
                            self.buffer.clear_cursor_style_override();
                        }
                        1 => {
                            self.buffer.set_cursor_style_override(
                                forge_core::config_registry::CursorStyle::Block,
                                Some(true),
                            );
                        }
                        2 => {
                            self.buffer.set_cursor_style_override(
                                forge_core::config_registry::CursorStyle::Block,
                                Some(false),
                            );
                        }
                        3 => {
                            self.buffer.set_cursor_style_override(
                                forge_core::config_registry::CursorStyle::Underline,
                                Some(true),
                            );
                        }
                        4 => {
                            self.buffer.set_cursor_style_override(
                                forge_core::config_registry::CursorStyle::Underline,
                                Some(false),
                            );
                        }
                        5 => {
                            self.buffer.set_cursor_style_override(
                                forge_core::config_registry::CursorStyle::Beam,
                                Some(true),
                            );
                        }
                        6 => {
                            self.buffer.set_cursor_style_override(
                                forge_core::config_registry::CursorStyle::Beam,
                                Some(false),
                            );
                        }
                        _ => {}
                    }
                }
            }
            _ => tracing::trace!("Unhandled CSI: action={}", action),
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        *self.parser_is_ground = true;
        match (intermediates, byte) {
            ([b'('], b'0') => self.charsets.g0 = Charset::DecSpecialGraphics,
            ([b'('], b'B') => self.charsets.g0 = Charset::Ascii,
            ([b')'], b'0') => self.charsets.g1 = Charset::DecSpecialGraphics,
            ([b')'], b'B') => self.charsets.g1 = Charset::Ascii,
            (_, b'7') => self.buffer.saved_cursor = Some(self.buffer.cursor),
            (_, b'8') => {
                if let Some(c) = self.buffer.saved_cursor {
                    self.buffer.cursor = c;
                }
            }
            (_, b'c') => {
                // RIS reset: clear temporary app cursor shape so rendering falls back to config.
                self.buffer.clear_cursor_style_override();
                self.buffer.set_cursor_visible(true);
            }
            (_, b'M') => {
                // reverse index (scroll down)
                self.buffer.reverse_index();
            }
            _ => tracing::trace!(
                "Unhandled ESC: intermediates={:?} byte=0x{:02X}",
                intermediates,
                byte
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Charset {
    Ascii,
    DecSpecialGraphics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GraphicSet {
    G0,
    G1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CharsetState {
    g0: Charset,
    g1: Charset,
    gl: GraphicSet,
}

impl CharsetState {
    fn new() -> Self {
        Self {
            g0: Charset::Ascii,
            g1: Charset::Ascii,
            gl: GraphicSet::G0,
        }
    }

    fn active(&self) -> Charset {
        match self.gl {
            GraphicSet::G0 => self.g0,
            GraphicSet::G1 => self.g1,
        }
    }

    fn uses_ascii_gl(&self) -> bool {
        self.active() == Charset::Ascii
    }

    fn translate(&self, c: char) -> char {
        match self.active() {
            Charset::Ascii => c,
            Charset::DecSpecialGraphics => translate_dec_special_graphics(c),
        }
    }
}

fn translate_dec_special_graphics(c: char) -> char {
    match c {
        '`' => '◆',
        'a' => '▒',
        'b' => '␉',
        'c' => '␌',
        'd' => '␍',
        'e' => '␊',
        'f' => '°',
        'g' => '±',
        'h' => '␤',
        'i' => '␋',
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'o' => '⎺',
        'p' => '⎻',
        'q' => '─',
        'r' => '⎼',
        's' => '⎽',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        'y' => '≤',
        'z' => '≥',
        '{' => 'π',
        '|' => '≠',
        '}' => '£',
        '~' => '·',
        _ => c,
    }
}

fn get_param_or(params: &Params, index: usize, default: u16) -> u16 {
    params
        .iter()
        .nth(index)
        .and_then(|p| p.iter().next().copied())
        .filter(|&v| v != 0)
        .unwrap_or(default)
}

fn handle_sgr(params: &Params, buffer: &mut ScreenBuffer) {
    let mut flat = Vec::new();
    for param in params.iter() {
        for sub in param.iter() {
            flat.push(*sub);
        }
    }

    if flat.is_empty() {
        flat.push(0);
    }

    let mut i = 0;
    while i < flat.len() {
        match flat[i] {
            0 => {
                buffer.attr_bold = false;
                buffer.attr_italic = false;
                buffer.attr_underline = false;
                buffer.attr_strikethrough = false;
                buffer.attr_inverse = false;
                buffer.current_fg = buffer.default_fg;
                buffer.current_bg = buffer.default_bg;
            }
            1 => buffer.attr_bold = true,
            3 => buffer.attr_italic = true,
            4 => buffer.attr_underline = true,
            7 => buffer.attr_inverse = true,
            9 => buffer.attr_strikethrough = true,
            22 => buffer.attr_bold = false,
            23 => buffer.attr_italic = false,
            24 => buffer.attr_underline = false,
            27 => buffer.attr_inverse = false,
            29 => buffer.attr_strikethrough = false,
            30..=37 => buffer.current_fg = ansi_256_color(flat[i] as u8 - 30, &buffer.palette),
            39 => buffer.current_fg = buffer.default_fg,
            40..=47 => buffer.current_bg = ansi_256_color(flat[i] as u8 - 40, &buffer.palette),
            49 => buffer.current_bg = buffer.default_bg,
            90..=97 => buffer.current_fg = ansi_256_color(flat[i] as u8 - 90 + 8, &buffer.palette),
            100..=107 => {
                buffer.current_bg = ansi_256_color(flat[i] as u8 - 100 + 8, &buffer.palette)
            }
            38 => {
                if i + 2 < flat.len() && flat[i + 1] == 5 {
                    buffer.current_fg = ansi_256_color(flat[i + 2] as u8, &buffer.palette);
                    i += 2;
                } else if i + 4 < flat.len() && flat[i + 1] == 2 {
                    buffer.current_fg = Color {
                        r: flat[i + 2] as u8,
                        g: flat[i + 3] as u8,
                        b: flat[i + 4] as u8,
                        a: 255,
                    };
                    i += 4;
                }
            }
            48 => {
                if i + 2 < flat.len() && flat[i + 1] == 5 {
                    buffer.current_bg = ansi_256_color(flat[i + 2] as u8, &buffer.palette);
                    i += 2;
                } else if i + 4 < flat.len() && flat[i + 1] == 2 {
                    buffer.current_bg = Color {
                        r: flat[i + 2] as u8,
                        g: flat[i + 3] as u8,
                        b: flat[i + 4] as u8,
                        a: 255,
                    };
                    i += 4;
                }
            }
            _ => tracing::trace!("Unhandled SGR: {}", flat[i]),
        }
        i += 1;
    }
}

fn handle_mode(params: &Params, intermediates: &[u8], action: char, buffer: &mut ScreenBuffer) {
    if intermediates.contains(&b'?') {
        for param in params {
            if param == [1] {
                buffer.application_cursor_keys = action == 'h';
                tracing::trace!(
                    "Application cursor keys: {}",
                    buffer.application_cursor_keys
                );
            } else if param == [25] {
                buffer.set_cursor_visible(action == 'h');
            } else if param == [1000] || param == [1002] {
                buffer.mouse_tracking_enabled = action == 'h';
                tracing::trace!("Mouse tracking: {}", buffer.mouse_tracking_enabled);
            } else if param == [1006] {
                buffer.mouse_sgr_mode = action == 'h';
                tracing::trace!("Mouse SGR mode: {}", buffer.mouse_sgr_mode);
            } else if param == [2004] {
                buffer.bracketed_paste = action == 'h';
                tracing::trace!("Bracketed paste: {}", buffer.bracketed_paste);
            } else if param == [1049] {
                if action == 'h' {
                    buffer.enable_alt_buffer();
                } else if action == 'l' {
                    buffer.disable_alt_buffer();
                }
            }
        }
    }
}

pub struct VteProcessor {
    parser: Parser,
    ascii_fast_path_enabled: bool,
    parser_is_ground: bool,
    charsets: CharsetState,
    command_events: Vec<CommandLifecycleEvent>,
}

impl VteProcessor {
    pub fn new() -> Self {
        VteProcessor {
            parser: Parser::new(),
            ascii_fast_path_enabled: true,
            parser_is_ground: true,
            charsets: CharsetState::new(),
            command_events: Vec::new(),
        }
    }

    pub fn take_command_events(&mut self) -> Vec<CommandLifecycleEvent> {
        std::mem::take(&mut self.command_events)
    }
}

impl Default for VteProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl VteProcessor {
    pub fn process(&mut self, data: &[u8], buffer: &mut ScreenBuffer) -> Vec<u8> {
        let _span = tracing::trace_span!("vte.process_batch", bytes = data.len()).entered();
        if self.ascii_fast_path_enabled && self.parser_is_ground && self.charsets.uses_ascii_gl() {
            return self.process_with_ascii_fast_path(data, buffer);
        }

        let responses = self.process_slow(data, buffer);
        if self.parser_is_ground
            && self.charsets.uses_ascii_gl()
            && data.iter().any(|&byte| matches!(byte, b'\n' | b'\r'))
        {
            self.ascii_fast_path_enabled = true;
        }
        responses
    }

    fn process_with_ascii_fast_path(&mut self, data: &[u8], buffer: &mut ScreenBuffer) -> Vec<u8> {
        let mut offset = 0;
        let mut responses = Vec::new();

        while offset < data.len() {
            // Batch printable ASCII characters.
            let printable_start = offset;
            while offset < data.len() && is_printable_ascii(data[offset]) {
                offset += 1;
            }

            if offset > printable_start {
                buffer.write_ascii_run(&data[printable_start..offset]);
            }

            if offset == data.len() {
                break;
            }

            match data[offset] {
                b'\n' => {
                    // Batch consecutive newlines into a single scroll call.
                    // This is the critical hot path for `cat large_file` and `yes | head`.
                    let nl_start = offset;
                    while offset < data.len() && data[offset] == b'\n' {
                        offset += 1;
                    }
                    let count = offset - nl_start;
                    buffer.line_feeds_n(count);
                }
                b'\r' => {
                    buffer.carriage_return();
                    offset += 1;
                    // Handle \r\n pair: batch the \n immediately after \r.
                    if offset < data.len() && data[offset] == b'\n' {
                        let nl_start = offset;
                        while offset < data.len() && data[offset] == b'\n' {
                            offset += 1;
                        }
                        buffer.line_feeds_n(offset - nl_start);
                    }
                }
                b'\t' => {
                    let next_tab = ((buffer.cursor.col / 8) + 1) * 8;
                    buffer.move_cursor_to(
                        buffer.cursor.row,
                        next_tab.min(buffer.cols().saturating_sub(1)),
                    );
                    offset += 1;
                }
                0x08 => {
                    buffer.move_cursor_relative(0, -1);
                    offset += 1;
                }
                _ => {
                    responses.extend(self.process_slow(&data[offset..], buffer));
                    self.ascii_fast_path_enabled = self.parser_is_ground
                        && self.charsets.uses_ascii_gl()
                        && matches!(data[offset], b'\n' | b'\r');
                    break;
                }
            }
        }

        responses
    }

    fn process_slow(&mut self, data: &[u8], buffer: &mut ScreenBuffer) -> Vec<u8> {
        let mut performer = TerminalPerformer {
            buffer,
            charsets: &mut self.charsets,
            parser_is_ground: &mut self.parser_is_ground,
            responses: Vec::new(),
            command_events: &mut self.command_events,
        };
        for &byte in data {
            if starts_escape_sequence(byte) {
                *performer.parser_is_ground = false;
            }
            self.parser.advance(&mut performer, byte);
        }
        performer.responses
    }
}

fn is_printable_ascii(byte: u8) -> bool {
    (0x20..=0x7e).contains(&byte)
}

fn starts_escape_sequence(byte: u8) -> bool {
    matches!(byte, 0x1B | 0x90 | 0x9B | 0x9D)
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_core::color::Color;

    fn test_screen(cols: usize, rows: usize) -> ScreenBuffer {
        ScreenBuffer::new(
            cols,
            rows,
            100,
            forge_core::color::Color {
                r: 192,
                g: 202,
                b: 245,
                a: 255,
            },
            forge_core::color::Color {
                r: 30,
                g: 30,
                b: 46,
                a: 255,
            },
        )
    }

    #[test]
    fn test_print_ascii() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(80, 24);
        processor.process(b"Hello, World!\r\n", &mut buf);
        assert_eq!(buf.cursor.row, 1);
        assert_eq!(buf.cursor.col, 0);
        assert_eq!(buf.visible_row(0)[0].c, 'H');
        assert_eq!(buf.visible_row(0)[1].c, 'e');
    }

    #[test]
    fn test_reverse_index() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(80, 24);
        
        // Print two lines
        processor.process(b"Line 1\r\nLine 2", &mut buf);
        assert_eq!(buf.cursor.row, 1);
        
        // Test Reverse Index when NOT at top margin
        processor.process(b"\x1bM", &mut buf);
        assert_eq!(buf.cursor.row, 0); // Cursor moves up
        
        // Test Reverse Index when AT top margin (should scroll down)
        // Currently at row 0.
        processor.process(b"\x1bM", &mut buf);
        assert_eq!(buf.cursor.row, 0); // Cursor stays at top
        
        // The content should have shifted down, so row 0 is empty, row 1 has "Line 1"
        assert_eq!(buf.visible_row(0)[0].c, ' '); // New blank line at top
        assert_eq!(buf.visible_row(1)[0].c, 'L'); // "Line 1" moved down to row 1
        assert_eq!(buf.visible_row(2)[0].c, 'L'); // "Line 2" moved down to row 2
    }

    #[test]
    fn test_ascii_fast_path_preserves_sgr_state() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(20, 5);

        processor.process(b"\x1b[31m", &mut buf);
        processor.process(b"red\r\n", &mut buf);

        assert_eq!(buf.visible_row(0)[0].c, 'r');
        assert_eq!(buf.visible_row(0)[0].fg.r, 194);
    }

    #[test]
    fn test_sgr_reverse_video_and_reset() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(20, 5);

        processor.process(b"\x1b[7mA\x1b[27mB\x1b[7mC\x1b[0mD", &mut buf);

        assert!(buf.visible_row(0)[0].is_inverse());
        assert!(!buf.visible_row(0)[1].is_inverse());
        assert!(buf.visible_row(0)[2].is_inverse());
        assert!(!buf.visible_row(0)[3].is_inverse());
    }

    #[test]
    fn test_split_escape_does_not_fast_path_payload() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(20, 5);

        processor.process(b"\x1b[", &mut buf);
        processor.process(b"31mred\r\n", &mut buf);

        assert_eq!(buf.visible_row(0)[0].c, 'r');
        assert_eq!(buf.visible_row(0)[0].fg.r, 194);
    }

    #[test]
    fn test_split_csi_after_newline_does_not_print_numeric_continuation() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(40, 5);

        processor.process(b"before\r\n\x1b[38;", &mut buf);
        processor.process(b"2;110;106;134mcolored", &mut buf);

        let row: String = buf.visible_row(1).iter().map(|cell| cell.c).collect();
        assert!(row.starts_with("colored"));
        assert!(!row.contains("2;110;106;134m"));
        assert_eq!(
            buf.visible_row(1)[0].fg,
            Color {
                r: 110,
                g: 106,
                b: 134,
                a: 255,
            }
        );
    }

    #[test]
    fn test_dec_special_graphics_draws_box_lines() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(20, 5);

        processor.process(b"\x1b(0lqk\r\nx x\r\nmqj\x1b(B", &mut buf);

        assert_eq!(buf.visible_row(0)[0].c, '┌');
        assert_eq!(buf.visible_row(0)[1].c, '─');
        assert_eq!(buf.visible_row(0)[2].c, '┐');
        assert_eq!(buf.visible_row(1)[0].c, '│');
        assert_eq!(buf.visible_row(1)[2].c, '│');
        assert_eq!(buf.visible_row(2)[0].c, '└');
        assert_eq!(buf.visible_row(2)[1].c, '─');
        assert_eq!(buf.visible_row(2)[2].c, '┘');
    }

    #[test]
    fn test_dec_special_graphics_state_survives_split_batches() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(20, 5);

        processor.process(b"\x1b(0", &mut buf);
        processor.process(b"qqq", &mut buf);
        processor.process(b"\x1b(Bq", &mut buf);

        assert_eq!(buf.visible_row(0)[0].c, '─');
        assert_eq!(buf.visible_row(0)[1].c, '─');
        assert_eq!(buf.visible_row(0)[2].c, '─');
        assert_eq!(buf.visible_row(0)[3].c, 'q');
    }

    #[test]
    fn test_dec_special_graphics_g1_shift() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(20, 5);

        processor.process(b"\x1b)0\x0Eq\x0Fq", &mut buf);

        assert_eq!(buf.visible_row(0)[0].c, '─');
        assert_eq!(buf.visible_row(0)[1].c, 'q');
    }

    #[test]
    fn test_sgr_colors() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(10, 10);
        processor.process(b"\x1b[31m", &mut buf);
        assert_eq!(buf.current_fg.r, 194);
        assert_eq!(buf.current_fg.g, 54);
        assert_eq!(buf.current_fg.b, 33);

        processor.process(b"\x1b[38;2;100;200;50m", &mut buf);
        assert_eq!(
            buf.current_fg,
            Color {
                r: 100,
                g: 200,
                b: 50,
                a: 255
            }
        );

        processor.process(b"\x1b[0m", &mut buf);
        assert_eq!(
            buf.current_fg,
            Color {
                r: 192,
                g: 202,
                b: 245,
                a: 255
            }
        );
    }

    #[test]
    fn test_cursor_movement() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(10, 10);
        buf.move_cursor_to(5, 5);
        processor.process(b"\x1b[3A", &mut buf); // Up 3
        assert_eq!(buf.cursor.row, 2);
    }

    #[test]
    fn test_erase_line() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(10, 10);
        processor.process(b"12345", &mut buf);
        buf.move_cursor_to(0, 2);
        processor.process(b"\x1b[2K", &mut buf);
        for c in 0..10 {
            assert!(buf.visible_row(0)[c].is_empty());
        }
    }

    #[test]
    fn test_cursor_movement_edge_cases() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(20, 20);

        // Default param test (H)
        buf.move_cursor_to(10, 10);
        processor.process(b"\x1b[H", &mut buf);
        assert_eq!(buf.cursor.row, 0);
        assert_eq!(buf.cursor.col, 0);

        // Explicit 1;1H
        buf.move_cursor_to(10, 10);
        processor.process(b"\x1b[1;1H", &mut buf);
        assert_eq!(buf.cursor.row, 0);
        assert_eq!(buf.cursor.col, 0);

        // Explicit 5;10H -> row 4, col 9
        processor.process(b"\x1b[5;10H", &mut buf);
        assert_eq!(buf.cursor.row, 4);
        assert_eq!(buf.cursor.col, 9);

        // Default param test (A)
        processor.process(b"\x1b[A", &mut buf);
        assert_eq!(buf.cursor.row, 3);
        assert_eq!(buf.cursor.col, 9);
    }

    #[test]
    fn cursor_next_and_previous_line_reset_column_and_clamp() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(20, 10);
        buf.move_cursor_to(5, 7);

        processor.process(b"\x1b[2F", &mut buf);
        assert_eq!(buf.cursor.row, 3);
        assert_eq!(buf.cursor.col, 0);

        processor.process(b"\x1b[E", &mut buf);
        assert_eq!(buf.cursor.row, 4);
        assert_eq!(buf.cursor.col, 0);

        processor.process(b"\x1b[99F", &mut buf);
        assert_eq!(buf.cursor.row, 0);
        assert_eq!(buf.cursor.col, 0);

        processor.process(b"\x1b[99E", &mut buf);
        assert_eq!(buf.cursor.row, 9);
        assert_eq!(buf.cursor.col, 0);
    }

    #[test]
    fn vertical_position_absolute_preserves_column() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(80, 24);
        buf.move_cursor_to(23, 17);

        processor.process(b"\x1b[2d", &mut buf);

        assert_eq!(buf.cursor.row, 1);
        assert_eq!(buf.cursor.col, 17);
    }

    #[test]
    fn nano_final_cursor_sequence_moves_from_footer_to_edit_area() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(80, 24);
        buf.move_cursor_to(23, 79);

        processor.process(b"\r\x1b[2d\x1b[?12l\x1b[?25h", &mut buf);

        assert_eq!(buf.cursor.row, 1);
        assert_eq!(buf.cursor.col, 0);
        assert!(buf.cursor_visible);
    }

    #[test]
    fn pacman_style_multibar_updates_existing_rows() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(32, 6);
        processor.process(b"package-a\r\npackage-b\r\nTotal", &mut buf);

        processor.process(b"\x1b[2F\x1b[Kupdated-a", &mut buf);
        processor.process(b"\x1b[2E\x1b[Kupdated-total", &mut buf);

        assert_eq!(buf.cursor.row, 2);
        assert_eq!(buf.visible_row(0)[0].c, 'u');
        assert_eq!(buf.visible_row(1)[0].c, 'p');
        assert_eq!(buf.visible_row(2)[0].c, 'u');
        assert!(buf.visible_row(3).iter().all(|cell| cell.is_empty()));
    }

    #[test]
    fn test_sgr_256_colors() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(10, 10);

        // 256-color index 0
        processor.process(b"\x1b[38;5;0m", &mut buf);
        assert_eq!(
            buf.current_fg,
            Color {
                r: 0,
                g: 0,
                b: 0,
                a: 255
            }
        );

        // reset
        processor.process(b"\x1b[0m", &mut buf);
        assert_eq!(
            buf.current_fg,
            Color {
                r: 192,
                g: 202,
                b: 245,
                a: 255
            }
        );
    }

    #[test]
    fn test_sgr_colon_separated() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(10, 10);

        // This simulates 38:2:10:20:30 which vte parses as sub-parameters.
        // Wait, vte parser handles it internally, so we just pass the bytes.
        processor.process(b"\x1b[38:2:10:20:30m", &mut buf);
        assert_eq!(
            buf.current_fg,
            Color {
                r: 10,
                g: 20,
                b: 30,
                a: 255
            }
        );
    }

    #[test]
    fn decscusr_default_clears_cursor_style_override() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(10, 10);

        processor.process(b"\x1b[2 q", &mut buf);
        assert_eq!(
            buf.cursor_style_override,
            Some(forge_core::config_registry::CursorStyle::Block)
        );
        assert_eq!(buf.cursor_blink_override, Some(false));
        assert_eq!(buf.generate_snapshot().cursor_blink_override, Some(false));

        processor.process(b"\x1b[0 q", &mut buf);
        assert_eq!(buf.cursor_style_override, None);
        assert_eq!(buf.cursor_blink_override, None);
    }

    #[test]
    fn dectcem_hides_and_restores_cursor_in_snapshots() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(10, 10);

        assert!(buf.generate_snapshot().cursor.is_some());
        processor.process(b"\x1b[?25l", &mut buf);
        assert!(!buf.cursor_visible);
        assert!(buf.generate_snapshot().cursor.is_none());

        processor.process(b"\x1b[?25h", &mut buf);
        assert!(buf.cursor_visible);
        assert!(buf.generate_snapshot().cursor.is_some());
    }

    #[test]
    fn dectcem_dirties_only_when_visibility_changes() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(10, 10);
        buf.mark_all_clean();
        let clean_generation = buf.dirty_generations[0];

        processor.process(b"\x1b[?25l", &mut buf);
        let hidden_generation = buf.dirty_generations[0];
        assert!(hidden_generation > clean_generation);

        processor.process(b"\x1b[?25l", &mut buf);
        assert_eq!(buf.dirty_generations[0], hidden_generation);
    }

    #[test]
    fn alternate_screen_exit_clears_temporary_cursor_style_override() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(10, 10);

        processor.process(b"\x1b[?1049h", &mut buf);
        processor.process(b"\x1b[2 q", &mut buf);
        assert_eq!(
            buf.cursor_style_override,
            Some(forge_core::config_registry::CursorStyle::Block)
        );

        processor.process(b"\x1b[?1049l", &mut buf);
        assert_eq!(buf.cursor_style_override, None);
        assert_eq!(buf.cursor_blink_override, None);
    }

    #[test]
    fn alternate_screen_generation_changes_only_on_real_transitions() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(10, 10);

        assert_eq!(buf.alt_buffer_generation, 0);
        processor.process(b"\x1b[?1049h", &mut buf);
        assert_eq!(buf.alt_buffer_generation, 1);
        assert_eq!(buf.generate_snapshot().alt_buffer_generation, 1);

        processor.process(b"\x1b[?1049h", &mut buf);
        assert_eq!(buf.alt_buffer_generation, 1);

        processor.process(b"\x1b[?1049l", &mut buf);
        assert_eq!(buf.alt_buffer_generation, 2);
        assert_eq!(buf.generate_snapshot().alt_buffer_generation, 2);

        processor.process(b"\x1b[?1049l", &mut buf);
        assert_eq!(buf.alt_buffer_generation, 2);
    }

    #[test]
    fn terminal_resets_clear_cursor_style_override() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(10, 10);

        processor.process(b"\x1b[6 q", &mut buf);
        processor.process(b"\x1b[?25l", &mut buf);
        assert_eq!(
            buf.cursor_style_override,
            Some(forge_core::config_registry::CursorStyle::Beam)
        );

        processor.process(b"\x1b[!p", &mut buf);
        assert_eq!(buf.cursor_style_override, None);
        assert!(buf.cursor_visible);

        processor.process(b"\x1b[4 q", &mut buf);
        processor.process(b"\x1b[?25l", &mut buf);
        assert_eq!(
            buf.cursor_style_override,
            Some(forge_core::config_registry::CursorStyle::Underline)
        );

        processor.process(b"\x1bc", &mut buf);
        assert_eq!(buf.cursor_style_override, None);
        assert_eq!(buf.cursor_blink_override, None);
        assert!(buf.cursor_visible);
    }

    #[test]
    fn osc_133_command_completion_clears_current_command() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(10, 10);

        processor.process(b"\x1b]133;C;ls\x07", &mut buf);
        assert!(buf.is_command_running);
        assert_eq!(buf.current_command.as_deref(), Some("ls"));
        assert_eq!(
            processor.take_command_events(),
            vec![CommandLifecycleEvent::Started {
                command: Some("ls".to_string())
            }]
        );

        processor.process(b"\x1b]133;D;0\x07", &mut buf);
        assert!(!buf.is_command_running);
        assert_eq!(buf.current_command, None);
        assert_eq!(buf.last_exit_code, Some(0));
        assert_eq!(
            processor.take_command_events(),
            vec![CommandLifecycleEvent::Finished { exit_code: 0 }]
        );
    }

    #[test]
    fn osc_133_prompt_clears_stale_current_command() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(10, 10);

        processor.process(b"\x1b]133;C;ls\x07", &mut buf);
        processor.process(b"\x1b]133;A\x07", &mut buf);

        assert!(!buf.is_command_running);
        assert_eq!(buf.current_command, None);
    }

    #[test]
    fn osc_133_start_and_finish_in_same_chunk_emit_both_events() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(10, 10);

        processor.process(b"\x1b]133;C;true\x07\x1b]133;D;0\x07", &mut buf);

        assert_eq!(
            processor.take_command_events(),
            vec![
                CommandLifecycleEvent::Started {
                    command: Some("true".to_string())
                },
                CommandLifecycleEvent::Finished { exit_code: 0 }
            ]
        );
        assert!(!buf.is_command_running);
        assert_eq!(buf.current_command, None);
    }
}
