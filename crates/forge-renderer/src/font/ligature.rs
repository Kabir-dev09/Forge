use forge_core::cell::{Cell, SelectionRange};
use forge_core::color::Color;

const CANDIDATE_CHARS: &[char] = &[
    '=', '!', '<', '>', '-', '/', '*', ':', '.', '|', '&', '+', '~',
];

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LigatureStyleKey {
    pub fg_rgba: u32,
    pub bg_rgba: u32,
    pub flags: u8,
}

impl LigatureStyleKey {
    pub fn from_cell(cell: &Cell) -> Self {
        Self {
            fg_rgba: color_key(cell.fg),
            bg_rgba: color_key(cell.bg),
            flags: cell.flags
                & (Cell::FLAG_BOLD
                    | Cell::FLAG_ITALIC
                    | Cell::FLAG_UNDERLINE
                    | Cell::FLAG_STRIKETHROUGH
                    | Cell::FLAG_INVERSE),
        }
    }
}

fn color_key(color: Color) -> u32 {
    u32::from_be_bytes([color.r, color.g, color.b, color.a])
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LigatureToken {
    pub start_col: usize,
    pub end_col: usize,
    pub text: String,
    pub style: LigatureStyleKey,
}

fn chars_have_ligature_candidate(chars: impl Iterator<Item = char>) -> bool {
    let mut previous_trigger = false;
    let mut previous_char = '\0';
    let mut previous_previous_char = '\0';
    let mut w_run = 0usize;

    for c in chars {
        if c == '\0' {
            previous_trigger = false;
            previous_char = '\0';
            previous_previous_char = '\0';
            w_run = 0;
            continue;
        }

        if c == 'w' {
            w_run += 1;
            if w_run >= 3 {
                return true;
            }
        } else {
            w_run = 0;
        }

        let is_trigger = CANDIDATE_CHARS.contains(&c);
        let is_home_path_prefix = previous_char == '~'
            && c == '/'
            && (previous_previous_char == '\0' || previous_previous_char.is_whitespace());
        if is_trigger && previous_trigger && !is_home_path_prefix {
            return true;
        }
        previous_trigger = is_trigger;
        previous_previous_char = previous_char;
        previous_char = c;
    }

    false
}

pub fn row_has_ligature_candidate(row: &[Cell]) -> bool {
    chars_have_ligature_candidate(row.iter().map(|cell| cell.c))
}

pub fn tokenize_ligature_candidates(
    row: &[Cell],
    max_token_len: usize,
    cursor_col: Option<usize>,
    selection: Option<SelectionRange>,
    row_idx: usize,
) -> Vec<LigatureToken> {
    let mut tokens = Vec::new();
    let max_token_len = max_token_len.max(2);
    let mut current_start = 0usize;
    let mut current_text = String::new();
    let mut current_style: Option<LigatureStyleKey> = None;

    for (col, cell) in row.iter().enumerate() {
        let style = LigatureStyleKey::from_cell(cell);
        let breaks = cell_breaks_ligature_run(cell)
            || cursor_col == Some(col)
            || selection_contains_cell(selection, row_idx, col)
            || current_style
                .as_ref()
                .is_some_and(|current| *current != style)
            || current_text.chars().count() >= max_token_len;

        if breaks {
            flush_token(
                &mut tokens,
                current_start,
                &mut current_text,
                &current_style,
            );
            current_style = None;
        }

        if cell_breaks_ligature_run(cell)
            || cursor_col == Some(col)
            || selection_contains_cell(selection, row_idx, col)
        {
            continue;
        }

        if current_text.is_empty() {
            current_start = col;
            current_style = Some(style);
        }
        current_text.push(cell.c);
    }

    flush_token(
        &mut tokens,
        current_start,
        &mut current_text,
        &current_style,
    );
    tokens
        .into_iter()
        .filter(|token| row_has_ligature_text_candidate(&token.text))
        .collect()
}

fn flush_token(
    tokens: &mut Vec<LigatureToken>,
    start_col: usize,
    text: &mut String,
    style: &Option<LigatureStyleKey>,
) {
    if text.chars().count() >= 2 {
        if let Some(style) = style {
            let end_col = start_col + text.chars().count() - 1;
            tokens.push(LigatureToken {
                start_col,
                end_col,
                text: std::mem::take(text),
                style: style.clone(),
            });
            return;
        }
    }
    text.clear();
}

fn row_has_ligature_text_candidate(text: &str) -> bool {
    chars_have_ligature_candidate(text.chars())
}

fn selection_contains_cell(selection: Option<SelectionRange>, row: usize, col: usize) -> bool {
    let Some(selection) = selection else {
        return false;
    };
    let (start_row, start_col, end_row, end_col) = if selection.start_row < selection.end_row
        || (selection.start_row == selection.end_row && selection.start_col <= selection.end_col)
    {
        (
            selection.start_row,
            selection.start_col,
            selection.end_row,
            selection.end_col,
        )
    } else {
        (
            selection.end_row,
            selection.end_col,
            selection.start_row,
            selection.start_col,
        )
    };

    if row < start_row || row > end_row {
        return false;
    }
    if start_row == end_row {
        col >= start_col && col <= end_col
    } else if row == start_row {
        col >= start_col
    } else if row == end_row {
        col <= end_col
    } else {
        true
    }
}

pub fn cell_breaks_ligature_run(cell: &Cell) -> bool {
    let c = cell.c;
    c == '\0'
        || c.is_whitespace()
        || cell.width() == forge_core::cell::CellWidth::Wide
        || is_combining_mark(c)
        || is_emoji(c)
        || is_box_drawing(c)
        || is_block_element(c)
        || is_braille(c)
        || is_powerline_or_private_use(c)
}

fn is_combining_mark(c: char) -> bool {
    matches!(
        c as u32,
        0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F
    )
}

fn is_emoji(c: char) -> bool {
    matches!(
        c as u32,
        0x1F000..=0x1FAFF | 0x2600..=0x27BF
    )
}

fn is_box_drawing(c: char) -> bool {
    matches!(c as u32, 0x2500..=0x257F)
}

fn is_block_element(c: char) -> bool {
    matches!(c as u32, 0x2580..=0x259F)
}

fn is_braille(c: char) -> bool {
    matches!(c as u32, 0x2800..=0x28FF)
}

fn is_powerline_or_private_use(c: char) -> bool {
    matches!(c as u32, 0xE000..=0xF8FF)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(c: char) -> Cell {
        Cell {
            c,
            fg: Color::WHITE,
            bg: Color::BLACK,
            flags: 0,
        }
    }

    fn row(text: &str) -> Vec<Cell> {
        text.chars().map(cell).collect()
    }

    #[test]
    fn scanner_detects_common_candidates() {
        for text in [
            "!=", "==", "=>", "->", "::", "&&", "||", "++", "/** */", "www", "~>", "~~>",
        ] {
            assert!(row_has_ligature_candidate(&row(text)), "{text}");
        }
    }

    #[test]
    fn scanner_ignores_plain_text_and_separated_triggers() {
        assert!(!row_has_ligature_candidate(&row("hello world")));
        assert!(!row_has_ligature_candidate(&row("a = b")));
        assert!(!row_has_ligature_candidate(&[
            cell('-'),
            Cell::wide_placeholder(),
            cell('>')
        ]));
    }

    #[test]
    fn scanner_ignores_home_path_prefix_without_hiding_later_operators() {
        assert!(!row_has_ligature_candidate(&row("~/PROJECTS")));
        assert!(!row_has_ligature_candidate(&row("prompt ~/Downloads")));
        assert!(row_has_ligature_candidate(&row("~/project->branch")));
        assert!(row_has_ligature_candidate(&row("prefix~/suffix")));
        assert!(row_has_ligature_candidate(&row("~>")));
    }

    #[test]
    fn tokenizer_does_not_shape_home_path_prefix() {
        assert!(tokenize_ligature_candidates(&row("~/PROJECTS"), 64, None, None, 0).is_empty());

        let tokens = tokenize_ligature_candidates(&row("~/project->branch"), 64, None, None, 0);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].text, "~/project->branch");
    }

    #[test]
    fn tokenizer_splits_at_style_cursor_selection_and_unsafe_symbols() {
        let mut row = row("ab!=cd->ef");
        row[4].set_bold(true);
        row[8].c = '─';
        let tokens = tokenize_ligature_candidates(
            &row,
            64,
            Some(6),
            Some(SelectionRange {
                start_row: 0,
                start_col: 1,
                end_row: 0,
                end_col: 1,
            }),
            0,
        );

        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].text, "!=");
        assert_eq!(tokens[0].start_col, 2);
    }

    #[test]
    fn tokenizer_limits_token_length() {
        let tokens = tokenize_ligature_candidates(&row("aa==bb"), 4, None, None, 0);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].text, "aa==");
    }

    #[test]
    fn inverse_video_is_part_of_the_ligature_style_key() {
        let normal = cell('=');
        let mut inverse = normal;
        inverse.set_inverse(true);

        assert_ne!(
            LigatureStyleKey::from_cell(&normal),
            LigatureStyleKey::from_cell(&inverse)
        );
    }
}
