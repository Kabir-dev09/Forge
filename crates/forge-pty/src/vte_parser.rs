use crate::screen_buffer::ScreenBuffer;
use forge_core::color::ansi_256_color;
use forge_core::color::Color;
use vte::{Params, Parser, Perform};

pub struct TerminalPerformer<'a> {
    buffer: &'a mut ScreenBuffer,
    charsets: &'a mut CharsetState,
    parser_is_ground: &'a mut bool,
    responses: Vec<u8>,
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

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {
        *self.parser_is_ground = false;
        tracing::trace!("hook");
    }

    fn put(&mut self, _byte: u8) {
        *self.parser_is_ground = false;
        tracing::trace!("put");
    }

    fn unhook(&mut self) {
        *self.parser_is_ground = true;
        tracing::trace!("unhook");
    }

    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {
        *self.parser_is_ground = true;
        tracing::trace!("osc_dispatch");
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
            'G' => self
                .buffer
                .move_cursor_to(self.buffer.cursor.row, (p0 - 1).max(0) as usize),
            'H' | 'f' => self
                .buffer
                .move_cursor_to((p0 - 1).max(0) as usize, (p1 - 1).max(0) as usize),
            'J' => match get_param_or(params, 0, 0) {
                0 => self.buffer.erase_to_end_of_screen(),
                1 => {
                    let r = self.buffer.cursor.row;
                    for row in 0..r {
                        let cols = self.buffer.cols();
                        for col in 0..cols {
                            self.buffer.move_cursor_to(row, col);
                            self.buffer.write_grapheme(" ");
                        }
                    }
                    self.buffer.erase_to_start_of_line();
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
                if p0 == 6 {
                    // Device Status Report (DSR) - report cursor position
                    // Format: ESC [ <row> ; <col> R (1-indexed)
                    let row = self.buffer.cursor.row + 1;
                    let col = self.buffer.cursor.col + 1;
                    let response = format!("\x1b[{};{}R", row, col);
                    self.responses.extend_from_slice(response.as_bytes());
                }
            }
            'h' | 'l' => handle_mode(params, intermediates, action, self.buffer),
            'q' => {
                if intermediates.first() == Some(&b' ') {
                    let p0 = get_param_or(params, 0, 0);
                    match p0 {
                        0 | 1 => {
                            self.buffer.cursor_style_override =
                                Some(forge_core::config_registry::CursorStyle::Block);
                            self.buffer.cursor_blink_override = Some(true);
                        }
                        2 => {
                            self.buffer.cursor_style_override =
                                Some(forge_core::config_registry::CursorStyle::Block);
                            self.buffer.cursor_blink_override = Some(false);
                        }
                        3 => {
                            self.buffer.cursor_style_override =
                                Some(forge_core::config_registry::CursorStyle::Underline);
                            self.buffer.cursor_blink_override = Some(true);
                        }
                        4 => {
                            self.buffer.cursor_style_override =
                                Some(forge_core::config_registry::CursorStyle::Underline);
                            self.buffer.cursor_blink_override = Some(false);
                        }
                        5 => {
                            self.buffer.cursor_style_override =
                                Some(forge_core::config_registry::CursorStyle::Beam);
                            self.buffer.cursor_blink_override = Some(true);
                        }
                        6 => {
                            self.buffer.cursor_style_override =
                                Some(forge_core::config_registry::CursorStyle::Beam);
                            self.buffer.cursor_blink_override = Some(false);
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
            (_, b'M') => {
                // reverse index (scroll down)
                // for now just log
                tracing::trace!("Reverse index");
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
                buffer.current_fg = buffer.default_fg;
                buffer.current_bg = buffer.default_bg;
            }
            1 => buffer.attr_bold = true,
            3 => buffer.attr_italic = true,
            4 => buffer.attr_underline = true,
            9 => buffer.attr_strikethrough = true,
            22 => buffer.attr_bold = false,
            23 => buffer.attr_italic = false,
            24 => buffer.attr_underline = false,
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
}

impl VteProcessor {
    pub fn new() -> Self {
        VteProcessor {
            parser: Parser::new(),
            ascii_fast_path_enabled: true,
            parser_is_ground: true,
            charsets: CharsetState::new(),
        }
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
                    buffer.line_feed();
                    offset += 1;
                }
                b'\r' => {
                    buffer.carriage_return();
                    offset += 1;
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
    fn test_ascii_fast_path_preserves_sgr_state() {
        let mut processor = VteProcessor::new();
        let mut buf = test_screen(20, 5);

        processor.process(b"\x1b[31m", &mut buf);
        processor.process(b"red\r\n", &mut buf);

        assert_eq!(buf.visible_row(0)[0].c, 'r');
        assert_eq!(buf.visible_row(0)[0].fg.r, 194);
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
}
