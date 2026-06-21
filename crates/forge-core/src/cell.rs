use crate::color::Color;
use smallvec::SmallVec;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionRange {
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GraphemeCluster(pub SmallVec<[u8; 4]>);

impl GraphemeCluster {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        GraphemeCluster(SmallVec::from_slice(s.as_bytes()))
    }
    
    pub fn as_str(&self) -> &str {
        debug_assert!(std::str::from_utf8(&self.0).is_ok());
        // SAFETY: bytes were validated as UTF-8 at construction time
        unsafe { std::str::from_utf8_unchecked(&self.0) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CellWidth {
    Narrow,
    Wide,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub grapheme: GraphemeCluster,
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub width: CellWidth,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            grapheme: GraphemeCluster::from_str(" "),
            fg: Color::WHITE,
            bg: Color::BLACK,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            width: CellWidth::Narrow,
        }
    }
}

impl Cell {
    pub fn is_empty(&self) -> bool {
        self.grapheme.0.as_slice() == b" "
            && !self.bold
            && !self.italic
            && !self.underline
            && !self.strikethrough
    }

    pub fn wide_placeholder() -> Self {
        Cell {
            grapheme: GraphemeCluster::from_str("\0"),
            width: CellWidth::Wide,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_cell_grapheme() {
        let cell = Cell::default();
        assert_eq!(cell.grapheme.as_str(), " ");
    }
}
