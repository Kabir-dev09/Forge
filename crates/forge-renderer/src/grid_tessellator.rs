use forge_core::cell::Cell;
use super::pipeline::GlyphVertex;
use super::font::atlas::GlyphAtlas;

#[derive(Clone, Default)]
pub struct RowTessellation {
    pub bg_vertices: Vec<GlyphVertex>,
    pub fg_vertices: Vec<GlyphVertex>,
}

pub struct GridTessellator {
    pub vertices: Vec<GlyphVertex>,
    pub rows: Vec<RowTessellation>,
    last_cursor: Option<(usize, usize)>,
    last_cursor_visible: bool,
    last_selection: Option<forge_core::cell::SelectionRange>,
}

impl GridTessellator {
    pub fn new(max_cells: usize) -> Self {
        Self { 
            vertices: Vec::with_capacity(max_cells * 12),
            rows: Vec::new(),
            last_cursor: None,
            last_cursor_visible: true,
            last_selection: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn tessellate(
        &mut self,
        grid: &[&[Cell]],
        dirty_rows: &[bool],
        atlas: &GlyphAtlas,
        cell_w: f32,
        cell_h: f32,
        baseline: f32,
        vp_w: f32,
        vp_h: f32,
        default_bg: [f32; 4],
        cursor_color: [f32; 4],
        cursor: Option<(usize, usize)>, // (col, row)
        cursor_style: forge_core::config_registry::CursorStyle,
        cursor_visible_phase: bool,
        selection: Option<forge_core::cell::SelectionRange>,
        selection_bg: [f32; 4],
        pad_x: f32,
        pad_y: f32,
        scale_x: f32,
        scale_y: f32,
        scrollbar: Option<(f32, f32, f32, f32, f32, f32)>, // (thumb_y, thumb_height, thumb_width, thumb_x, thumb_opacity, track_opacity)
    ) {
        if self.rows.len() != grid.len() {
            self.rows.resize(grid.len(), RowTessellation::default());
        }

        let mut actual_dirty = dirty_rows.to_vec();
        actual_dirty.resize(grid.len(), true);

        // Mark cursor rows dirty if cursor state changed
        if cursor != self.last_cursor || cursor_visible_phase != self.last_cursor_visible {
            if let Some((_, r)) = self.last_cursor {
                if r < actual_dirty.len() { actual_dirty[r] = true; }
            }
            if let Some((_, r)) = cursor {
                if r < actual_dirty.len() { actual_dirty[r] = true; }
            }
        }
        self.last_cursor = cursor;
        self.last_cursor_visible = cursor_visible_phase;

        // Mark selection rows dirty if selection changed
        if selection != self.last_selection {
            if let Some(sel) = &self.last_selection {
                let start = sel.start_row.min(sel.end_row);
                let end = sel.start_row.max(sel.end_row);
                for r in start..=end {
                    if r < actual_dirty.len() { actual_dirty[r] = true; }
                }
            }
            if let Some(sel) = &selection {
                let start = sel.start_row.min(sel.end_row);
                let end = sel.start_row.max(sel.end_row);
                for r in start..=end {
                    if r < actual_dirty.len() { actual_dirty[r] = true; }
                }
            }
        }
        self.last_selection = selection;

        let ndc = |px_x: f32, px_y: f32| -> [f32; 2] {
            [(px_x / vp_w) * 2.0 - 1.0, (px_y / vp_h) * 2.0 - 1.0]
        };

        let color_to_f32 = |c: forge_core::color::Color| -> [f32; 4] {
            let srgb = c.to_srgb_linear();
            [srgb.r, srgb.g, srgb.b, srgb.a]
        };

        let push_quad = |verts: &mut Vec<GlyphVertex>, tl: [f32; 2], br: [f32; 2], uv_tl: [f32; 2], uv_br: [f32; 2], fg: [f32; 4], bg: [f32; 4]| {
            let tr = [br[0], tl[1]];
            let bl = [tl[0], br[1]];
            let uv_tr = [uv_br[0], uv_tl[1]];
            let uv_bl = [uv_tl[0], uv_br[1]];

            verts.push(GlyphVertex { position: tl, tex_coord: uv_tl, fg_color: fg, bg_color: bg });
            verts.push(GlyphVertex { position: tr, tex_coord: uv_tr, fg_color: fg, bg_color: bg });
            verts.push(GlyphVertex { position: bl, tex_coord: uv_bl, fg_color: fg, bg_color: bg });
            verts.push(GlyphVertex { position: tr, tex_coord: uv_tr, fg_color: fg, bg_color: bg });
            verts.push(GlyphVertex { position: br, tex_coord: uv_br, fg_color: fg, bg_color: bg });
            verts.push(GlyphVertex { position: bl, tex_coord: uv_bl, fg_color: fg, bg_color: bg });
        };

        for (row_idx, row) in grid.iter().enumerate() {
            if !actual_dirty[row_idx] { continue; }

            let row_tess = &mut self.rows[row_idx];
            row_tess.bg_vertices.clear();
            row_tess.fg_vertices.clear();

            for (col_idx, cell) in row.iter().enumerate() {
                if cell.c == '\0' { continue; }

                let px_x = col_idx as f32 * cell_w + pad_x;
                let px_y = row_idx as f32 * cell_h + pad_y;
                
                let mut fg = color_to_f32(cell.fg);
                let mut bg = color_to_f32(cell.bg);
                
                // Check if in selection
                let mut in_selection = false;
                if let Some(sel) = selection {
                    let (s_r, s_c, e_r, e_c) = if sel.start_row < sel.end_row || (sel.start_row == sel.end_row && sel.start_col <= sel.end_col) {
                        (sel.start_row, sel.start_col, sel.end_row, sel.end_col)
                    } else {
                        (sel.end_row, sel.end_col, sel.start_row, sel.start_col)
                    };
                    
                    if row_idx > s_r && row_idx < e_r {
                        in_selection = true;
                    } else if row_idx == s_r && row_idx == e_r {
                        if col_idx >= s_c && col_idx <= e_c { in_selection = true; }
                    } else if (row_idx == s_r && col_idx >= s_c) || (row_idx == e_r && col_idx <= e_c) {
                        in_selection = true;
                    }
                }

                if in_selection {
                    bg = selection_bg;
                }
                
                let is_cursor = cursor_visible_phase && cursor.is_some_and(|c| col_idx == c.0 && row_idx == c.1);
                let is_block_cursor = is_cursor && cursor_style == forge_core::config_registry::CursorStyle::Block;
                if is_block_cursor {
                    fg = default_bg;
                    bg = cursor_color;
                }

                let mut quad_w = cell_w;
                if is_cursor || in_selection {
                    let c = cell.c;
                    if c != ' ' {
                        if let Some(glyph) = atlas.get(c, cell.is_bold()) {
                            let actual_w = glyph.width as f32;
                            if actual_w > cell_w {
                                quad_w = actual_w;
                            }
                        }
                    }
                }

                if (bg[3] > 0.0 && bg != default_bg) || in_selection || is_block_cursor {
                    let tl = ndc(px_x, px_y);
                    let br = ndc(px_x + quad_w, px_y + cell_h);
                    let uv = [-1.0, 0.0];
                    push_quad(&mut row_tess.bg_vertices, tl, br, uv, uv, bg, bg);
                }

                if is_cursor && cursor_style == forge_core::config_registry::CursorStyle::Underline {
                    let tl = ndc(px_x, px_y + cell_h - 2.0);
                    let br = ndc(px_x + cell_w, px_y + cell_h);
                    let uv = [-1.0, 0.0];
                    push_quad(&mut row_tess.fg_vertices, tl, br, uv, uv, cursor_color, cursor_color);
                }
                
                if is_cursor && cursor_style == forge_core::config_registry::CursorStyle::Beam {
                    let tl = ndc(px_x, px_y);
                    let br = ndc(px_x + 1.0, px_y + cell_h);
                    let uv = [-1.0, 0.0];
                    push_quad(&mut row_tess.fg_vertices, tl, br, uv, uv, cursor_color, cursor_color);
                }

                let c = cell.c;
                if c != ' ' {
                    match c {
                        '\u{2580}'..='\u{259F}' => {
                            let mut block_tl_x = px_x;
                            let mut block_tl_y = px_y;
                            let mut block_br_x = px_x + cell_w;
                            let mut block_br_y = px_y + cell_h;

                            match c {
                                '\u{2580}' => block_br_y = px_y + (cell_h / 2.0), // Upper Half
                                '\u{2581}'..='\u{2587}' => { // Lower fractions
                                    let step = (c as u32 - 0x2580) as f32;
                                    let h = (cell_h * step) / 8.0;
                                    block_tl_y = px_y + cell_h - h;
                                }
                                '\u{2588}'..='\u{258F}' => { // Left fractions (including Full Block)
                                    let step = 8.0 - (c as u32 - 0x2588) as f32;
                                    let w = (cell_w * step) / 8.0;
                                    block_br_x = px_x + w;
                                }
                                '\u{2590}' => block_tl_x = px_x + (cell_w / 2.0), // Right Half
                                '\u{2594}' => { // Upper One Eighth
                                    let h = cell_h / 8.0;
                                    block_br_y = px_y + h;
                                }
                                '\u{2595}' => { // Right One Eighth
                                    let w = cell_w / 8.0;
                                    block_tl_x = px_x + cell_w - w;
                                }
                                _ => {} // Fallback to full block
                            }

                            let g_tl = ndc(block_tl_x, block_tl_y);
                            let g_br = ndc(block_br_x, block_br_y);
                            let uv = [-1.0, 0.0];
                            push_quad(&mut row_tess.fg_vertices, g_tl, g_br, uv, uv, fg, fg);
                        }
                        '\u{E0B0}' | '\u{E0B2}' | '\u{E0B4}' | '\u{E0B6}' |
                        '\u{E0B8}' | '\u{E0BA}' | '\u{E0BC}' | '\u{E0BE}' => {
                            let mut g_x = px_x;
                            let mut g_w = cell_w;
                            
                            // Right-pointing shapes bleed right
                            if c == '\u{E0B0}' || c == '\u{E0B4}' || c == '\u{E0B8}' || c == '\u{E0BC}' {
                                g_w += 1.0;
                            } 
                            // Left-pointing shapes bleed left
                            else {
                                g_x -= 1.0;
                                g_w += 1.0;
                            }
                            
                            let g_tl = ndc(g_x, px_y);
                            let g_br = ndc(g_x + g_w, px_y + cell_h);
                            let proc_id = match c {
                                '\u{E0B0}' => -2.0,
                                '\u{E0B2}' => -3.0,
                                '\u{E0B4}' => -4.0,
                                '\u{E0B6}' => -5.0,
                                '\u{E0B8}' => -6.0,
                                '\u{E0BA}' => -7.0,
                                '\u{E0BC}' => -8.0,
                                '\u{E0BE}' => -9.0,
                                _ => unreachable!(),
                            };
                            let uv_tl = [proc_id, 0.0];
                            let uv_br = [proc_id + 1.0, 1.0];
                            push_quad(&mut row_tess.fg_vertices, g_tl, g_br, uv_tl, uv_br, fg, bg);
                        }
                        _ => {
                            if let Some((u, d, l, r, rnd)) = decode_box_drawing(c) {
                                let g_tl = ndc(px_x, px_y);
                                let g_br = ndc(px_x + cell_w, px_y + cell_h);
                                let encoded = u | (d << 2) | (l << 4) | (r << 6) | ((rnd as u32) << 8);
                                let proc_id = -100.0 - encoded as f32;
                                let uv_tl = [proc_id, 0.0];
                                let uv_br = [proc_id + 1.0, 1.0];
                                push_quad(&mut row_tess.fg_vertices, g_tl, g_br, uv_tl, uv_br, fg, bg);
                            } else if ('\u{2800}'..='\u{28FF}').contains(&c) {
                                let pattern = c as u32 - 0x2800;
                                let g_tl = ndc(px_x, px_y);
                                let g_br = ndc(px_x + cell_w, px_y + cell_h);
                                let proc_id = -500.0 - pattern as f32;
                                let uv_tl = [proc_id, 0.0];
                                let uv_br = [proc_id + 1.0, 1.0];
                                push_quad(&mut row_tess.fg_vertices, g_tl, g_br, uv_tl, uv_br, fg, bg);
                            } else if let Some(glyph) = atlas.get(c, cell.is_bold()) {
                                let mut g_x = px_x + (glyph.bearing_x as f32 * scale_x);
                                let g_y = px_y + baseline - (glyph.bearing_y as f32 * scale_y);
                                let mut g_w = glyph.width as f32 * scale_x;
                                let g_h = glyph.height as f32 * scale_y;

                                if cell.is_italic() {
                                    g_x += cell_h * 0.2;
                                    g_w += cell_w * 0.2;
                                }

                                // Glyph Inflation for Powerline / connecting symbols
                                if ('\u{E0B0}'..='\u{E0D4}').contains(&c) {
                                    g_x -= 0.75;
                                    g_w += 1.5;
                                }

                                let g_tl = ndc(g_x, g_y);
                                let g_br = ndc(g_x + g_w, g_y + g_h);
                                push_quad(&mut row_tess.fg_vertices, g_tl, g_br, [glyph.u0, glyph.v0], [glyph.u1, glyph.v1], fg, bg);
                            }
                        }
                    }
                }

                if cell.is_underline() {
                    let thickness = 1.0;
                    let mut y = px_y + baseline + 2.0;
                    if y + thickness > px_y + cell_h {
                        y = px_y + cell_h - thickness;
                    }
                    let u_tl = ndc(px_x, y);
                    let u_br = ndc(px_x + cell_w, y + thickness);
                    let u_uv = [-1.0, 0.0];
                    push_quad(&mut row_tess.fg_vertices, u_tl, u_br, u_uv, u_uv, fg, fg);
                }

                if cell.is_strikethrough() {
                    let thickness = 1.0;
                    let y = px_y + baseline - (cell_h * 0.3);
                    let s_tl = ndc(px_x, y);
                    let s_br = ndc(px_x + cell_w, y + thickness);
                    let s_uv = [-1.0, 0.0];
                    push_quad(&mut row_tess.fg_vertices, s_tl, s_br, s_uv, s_uv, fg, fg);
                }
            }
        }

        self.vertices.clear();

        for row in &self.rows {
            self.vertices.extend_from_slice(&row.bg_vertices);
        }

        if let Some((thumb_y, thumb_height, thumb_width, thumb_x, thumb_opacity, track_opacity)) = scrollbar {
            if track_opacity > 0.0 {
                let track_x = thumb_x;
                let tl = ndc(track_x, 4.0);
                let br = ndc(track_x + thumb_width, vp_h - 4.0);
                let uv_tl = [-30.0, 0.0];
                let uv_br = [-29.0, 1.0];
                let bg = [0.5, 0.5, 0.5, track_opacity];
                push_quad(&mut self.vertices, tl, br, uv_tl, uv_br, bg, bg);
            }

            if thumb_opacity > 0.0 {
                let tl = ndc(thumb_x, thumb_y);
                let br = ndc(thumb_x + thumb_width, thumb_y + thumb_height);
                let uv_tl = [-31.0, 0.0];
                let uv_br = [-30.0, 1.0];
                let bg = [0.8, 0.8, 0.8, thumb_opacity * 0.6];
                push_quad(&mut self.vertices, tl, br, uv_tl, uv_br, bg, bg);
            }
        }

        for row in &self.rows {
            self.vertices.extend_from_slice(&row.fg_vertices);
        }
    }
}

fn decode_box_drawing(c: char) -> Option<(u32, u32, u32, u32, bool)> {
    let mut u = 0;
    let mut d = 0;
    let mut l = 0;
    let mut r = 0;
    let mut rnd = false;
    match c {
        '─' => { l = 1; r = 1; }
        '━' => { l = 2; r = 2; }
        '│' => { u = 1; d = 1; }
        '┃' => { u = 2; d = 2; }
        '┌' => { d = 1; r = 1; }
        '┍' => { d = 1; r = 2; }
        '┎' => { d = 2; r = 1; }
        '┏' => { d = 2; r = 2; }
        '┐' => { d = 1; l = 1; }
        '┑' => { d = 1; l = 2; }
        '┒' => { d = 2; l = 1; }
        '┓' => { d = 2; l = 2; }
        '└' => { u = 1; r = 1; }
        '┕' => { u = 1; r = 2; }
        '┖' => { u = 2; r = 1; }
        '┗' => { u = 2; r = 2; }
        '┘' => { u = 1; l = 1; }
        '┙' => { u = 1; l = 2; }
        '┚' => { u = 2; l = 1; }
        '┛' => { u = 2; l = 2; }
        '├' => { u = 1; d = 1; r = 1; }
        '┝' => { u = 1; d = 1; r = 2; }
        '┞' => { u = 2; d = 1; r = 1; }
        '┟' => { u = 1; d = 2; r = 1; }
        '┠' => { u = 2; d = 2; r = 1; }
        '┡' => { u = 2; d = 1; r = 2; }
        '┢' => { u = 1; d = 2; r = 2; }
        '┣' => { u = 2; d = 2; r = 2; }
        '┤' => { u = 1; d = 1; l = 1; }
        '┥' => { u = 1; d = 1; l = 2; }
        '┦' => { u = 2; d = 1; l = 1; }
        '┧' => { u = 1; d = 2; l = 1; }
        '┨' => { u = 2; d = 2; l = 1; }
        '┩' => { u = 2; d = 1; l = 2; }
        '┪' => { u = 1; d = 2; l = 2; }
        '┫' => { u = 2; d = 2; l = 2; }
        '┬' => { d = 1; l = 1; r = 1; }
        '┭' => { d = 1; l = 2; r = 1; }
        '┮' => { d = 1; l = 1; r = 2; }
        '┯' => { d = 1; l = 2; r = 2; }
        '┰' => { d = 2; l = 1; r = 1; }
        '┱' => { d = 2; l = 2; r = 1; }
        '┲' => { d = 2; l = 1; r = 2; }
        '┳' => { d = 2; l = 2; r = 2; }
        '┴' => { u = 1; l = 1; r = 1; }
        '┵' => { u = 1; l = 2; r = 1; }
        '┶' => { u = 1; l = 1; r = 2; }
        '┷' => { u = 1; l = 2; r = 2; }
        '┸' => { u = 2; l = 1; r = 1; }
        '┹' => { u = 2; l = 2; r = 1; }
        '┺' => { u = 2; l = 1; r = 2; }
        '┻' => { u = 2; l = 2; r = 2; }
        '┼' => { u = 1; d = 1; l = 1; r = 1; }
        '┽' => { u = 1; d = 1; l = 2; r = 1; }
        '┾' => { u = 1; d = 1; l = 1; r = 2; }
        '┿' => { u = 1; d = 1; l = 2; r = 2; }
        '╀' => { u = 2; d = 1; l = 1; r = 1; }
        '╁' => { u = 1; d = 2; l = 1; r = 1; }
        '╂' => { u = 2; d = 2; l = 1; r = 1; }
        '╃' => { u = 2; d = 1; l = 2; r = 1; }
        '╄' => { u = 1; d = 2; l = 2; r = 1; }
        '╅' => { u = 2; d = 1; l = 1; r = 2; }
        '╆' => { u = 1; d = 2; l = 1; r = 2; }
        '╇' => { u = 2; d = 2; l = 2; r = 1; }
        '╈' => { u = 2; d = 2; l = 1; r = 2; }
        '╉' => { u = 2; d = 1; l = 2; r = 2; }
        '╊' => { u = 1; d = 2; l = 2; r = 2; }
        '╋' => { u = 2; d = 2; l = 2; r = 2; }
        '═' => { l = 3; r = 3; }
        '║' => { u = 3; d = 3; }
        '╒' => { d = 1; r = 3; }
        '╓' => { d = 3; r = 1; }
        '╔' => { d = 3; r = 3; }
        '╕' => { d = 1; l = 3; }
        '╖' => { d = 3; l = 1; }
        '╗' => { d = 3; l = 3; }
        '╘' => { u = 1; r = 3; }
        '╙' => { u = 3; r = 1; }
        '╚' => { u = 3; r = 3; }
        '╛' => { u = 1; l = 3; }
        '╜' => { u = 3; l = 1; }
        '╝' => { u = 3; l = 3; }
        '╞' => { u = 1; d = 1; r = 3; }
        '╟' => { u = 3; d = 3; r = 1; }
        '╠' => { u = 3; d = 3; r = 3; }
        '╡' => { u = 1; d = 1; l = 3; }
        '╢' => { u = 3; d = 3; l = 1; }
        '╣' => { u = 3; d = 3; l = 3; }
        '╤' => { d = 1; l = 3; r = 3; }
        '╥' => { d = 3; l = 1; r = 1; }
        '╦' => { d = 3; l = 3; r = 3; }
        '╧' => { u = 1; l = 3; r = 3; }
        '╨' => { u = 3; l = 1; r = 1; }
        '╩' => { u = 3; l = 3; r = 3; }
        '╪' => { u = 1; d = 1; l = 3; r = 3; }
        '╫' => { u = 3; d = 3; l = 1; r = 1; }
        '╬' => { u = 3; d = 3; l = 3; r = 3; }
        '╭' => { d = 1; r = 1; rnd = true; }
        '╮' => { d = 1; l = 1; rnd = true; }
        '╯' => { u = 1; l = 1; rnd = true; }
        '╰' => { u = 1; r = 1; rnd = true; }
        '╴' => { l = 1; }
        '╵' => { u = 1; }
        '╶' => { r = 1; }
        '╷' => { d = 1; }
        '╸' => { l = 2; }
        '╹' => { u = 2; }
        '╺' => { r = 2; }
        '╻' => { d = 2; }
        '╼' => { l = 1; r = 2; }
        '╽' => { u = 1; d = 2; }
        '╾' => { l = 2; r = 1; }
        '╿' => { u = 2; d = 1; }
        _ => return None,
    }
    Some((u, d, l, r, rnd))
}
