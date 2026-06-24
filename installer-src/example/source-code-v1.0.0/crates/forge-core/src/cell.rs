use crate::color::Color;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionRange {
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CellWidth {
    Narrow,
    Wide,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub flags: u8,
}

impl Cell {
    pub const FLAG_BOLD: u8          = 0b0000_0001;
    pub const FLAG_ITALIC: u8        = 0b0000_0010;
    pub const FLAG_UNDERLINE: u8     = 0b0000_0100;
    pub const FLAG_STRIKETHROUGH: u8 = 0b0000_1000;
    pub const FLAG_WIDE: u8          = 0b0001_0000;

    #[inline(always)]
    pub fn is_bold(&self) -> bool { self.flags & Self::FLAG_BOLD != 0 }
    #[inline(always)]
    pub fn is_italic(&self) -> bool { self.flags & Self::FLAG_ITALIC != 0 }
    #[inline(always)]
    pub fn is_underline(&self) -> bool { self.flags & Self::FLAG_UNDERLINE != 0 }
    #[inline(always)]
    pub fn is_strikethrough(&self) -> bool { self.flags & Self::FLAG_STRIKETHROUGH != 0 }
    #[inline(always)]
    pub fn width(&self) -> CellWidth {
        if self.flags & Self::FLAG_WIDE != 0 { CellWidth::Wide } else { CellWidth::Narrow }
    }

    #[inline(always)]
    pub fn set_bold(&mut self, val: bool) {
        if val { self.flags |= Self::FLAG_BOLD; } else { self.flags &= !Self::FLAG_BOLD; }
    }
    #[inline(always)]
    pub fn set_italic(&mut self, val: bool) {
        if val { self.flags |= Self::FLAG_ITALIC; } else { self.flags &= !Self::FLAG_ITALIC; }
    }
    #[inline(always)]
    pub fn set_underline(&mut self, val: bool) {
        if val { self.flags |= Self::FLAG_UNDERLINE; } else { self.flags &= !Self::FLAG_UNDERLINE; }
    }
    #[inline(always)]
    pub fn set_strikethrough(&mut self, val: bool) {
        if val { self.flags |= Self::FLAG_STRIKETHROUGH; } else { self.flags &= !Self::FLAG_STRIKETHROUGH; }
    }
    #[inline(always)]
    pub fn set_width(&mut self, val: CellWidth) {
        if val == CellWidth::Wide { self.flags |= Self::FLAG_WIDE; } else { self.flags &= !Self::FLAG_WIDE; }
    }
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            c: ' ',
            fg: Color::WHITE,
            bg: Color::BLACK,
            flags: 0,
        }
    }
}

impl Cell {
    pub fn is_empty(&self) -> bool {
        self.c == ' ' && self.flags == 0
    }

    pub fn wide_placeholder() -> Self {
        Cell {
            c: '\0',
            flags: Self::FLAG_WIDE,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_cell_c() {
        let cell = Cell::default();
        assert_eq!(cell.c, ' ');
    }
}
