use super::font::atlas::{GlyphAtlas, GlyphKey, GlyphMetrics};
use super::pipeline::GlyphVertex;
use forge_core::cell::Cell;
use std::collections::HashSet;

#[derive(Clone, Default)]
pub struct RowTessellation {
    pub bg_vertices: Vec<GlyphVertex>,
    pub fg_vertices: Vec<GlyphVertex>,
    pub generation: u64,
}

impl RowTessellation {
    fn with_cell_capacity(cols: usize) -> Self {
        Self {
            bg_vertices: Vec::with_capacity(cols * 6),
            fg_vertices: Vec::with_capacity(cols * 6),
            generation: 0,
        }
    }

    fn prepare_for_row(&mut self, cols: usize) {
        self.bg_vertices.clear();
        self.fg_vertices.clear();

        reserve_capacity(&mut self.bg_vertices, cols * 6);
        reserve_capacity(&mut self.fg_vertices, cols * 6);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VertexRange {
    pub start: usize,
    pub count: usize,
}

impl VertexRange {
    fn new(start: usize, count: usize) -> Self {
        Self { start, count }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowVertexRanges {
    pub bg: VertexRange,
    pub fg: VertexRange,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollEvent {
    pub direction: ScrollDirection,
    pub top: usize,
    pub bottom: usize,
    pub lines: usize,
    pub full_viewport: bool,
}

pub struct GridTessellator {
    pub vertices: Vec<GlyphVertex>,
    pub rows: Vec<RowTessellation>,
    pub row_ranges: Vec<RowVertexRanges>,
    pub scrollbar_range: Option<VertexRange>,
    pub rebuilt_rows: Vec<usize>,
    missing_glyphs: HashSet<GlyphKey>,
    actual_dirty: Vec<bool>,
    last_cursor: Option<(usize, usize)>,
    last_cursor_visible: bool,
    last_selection: Option<forge_core::cell::SelectionRange>,
}

fn reserve_capacity<T>(values: &mut Vec<T>, required: usize) {
    if values.capacity() < required {
        values.reserve(required - values.capacity());
    }
}

fn translate_vertices_y(vertices: &mut [GlyphVertex], delta_ndc_y: f32) {
    for vertex in vertices {
        vertex.position[1] += delta_ndc_y;
    }
}

fn scrolled_row_destination(event: ScrollEvent, row: usize, row_count: usize) -> Option<usize> {
    match event.direction {
        ScrollDirection::Up => row
            .checked_sub(event.lines)
            .filter(|&destination| destination < row_count - event.lines),
        ScrollDirection::Down => {
            let destination = row + event.lines;
            if row >= event.lines && destination < row_count {
                Some(destination)
            } else {
                None
            }
        }
    }
}

fn glyph_for<'a>(
    atlas: &'a GlyphAtlas,
    missing_glyphs: &mut HashSet<GlyphKey>,
    c: char,
    is_bold: bool,
) -> Option<&'a GlyphMetrics> {
    if let Some(glyph) = atlas.get_exact(c, is_bold) {
        return Some(glyph);
    }

    missing_glyphs.insert(GlyphKey { c, is_bold });
    atlas.fallback()
}

impl RowTessellation {
    fn translate_y(&mut self, delta_ndc_y: f32) {
        translate_vertices_y(&mut self.bg_vertices, delta_ndc_y);
        translate_vertices_y(&mut self.fg_vertices, delta_ndc_y);
        self.generation = self.generation.wrapping_add(1);
    }
}

impl GridTessellator {
    pub fn new(max_cells: usize) -> Self {
        Self {
            vertices: Vec::with_capacity(max_cells * 12),
            rows: Vec::new(),
            row_ranges: Vec::new(),
            scrollbar_range: None,
            rebuilt_rows: Vec::new(),
            missing_glyphs: HashSet::new(),
            actual_dirty: Vec::new(),
            last_cursor: None,
            last_cursor_visible: true,
            last_selection: None,
        }
    }

    pub fn missing_glyphs(&self) -> &HashSet<GlyphKey> {
        &self.missing_glyphs
    }

    fn apply_scroll_reuse(
        &mut self,
        scroll_event: Option<ScrollEvent>,
        row_count: usize,
        cell_h: f32,
        vp_h: f32,
        selection: Option<forge_core::cell::SelectionRange>,
    ) -> bool {
        let Some(event) = scroll_event else {
            return true;
        };

        if !event.full_viewport
            || event.top != 0
            || event.bottom + 1 != row_count
            || event.lines == 0
            || event.lines >= row_count
            || self.rows.len() != row_count
            || selection.is_some()
            || self.last_selection.is_some()
            || vp_h <= 0.0
        {
            return false;
        }

        let lines = event.lines;
        let delta_ndc_per_row = (cell_h / vp_h) * 2.0;
        match event.direction {
            ScrollDirection::Up => {
                self.rows.rotate_left(lines);
                let translate_count = row_count - lines;
                for row in self.rows.iter_mut().take(translate_count) {
                    row.translate_y(-(lines as f32) * delta_ndc_per_row);
                }
                self.actual_dirty.fill(false);
                for row in row_count - lines..row_count {
                    self.actual_dirty[row] = true;
                }
            }
            ScrollDirection::Down => {
                self.rows.rotate_right(lines);
                for row in self.rows.iter_mut().skip(lines) {
                    row.translate_y(lines as f32 * delta_ndc_per_row);
                }
                self.actual_dirty.fill(false);
                for row in 0..lines {
                    self.actual_dirty[row] = true;
                }
            }
        }

        true
    }

    #[allow(clippy::too_many_arguments)]
    pub fn tessellate(
        &mut self,
        grid: &[&[Cell]],
        dirty_rows: &[bool],
        atlas: &GlyphAtlas,
        cell_w: f32,
        cell_h: f32,
        native_cell_w: f32,
        native_cell_h: f32,
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
        scrollbar: Option<(f32, f32, f32, f32, f32, f32)>, // (thumb_y, thumb_height, thumb_width, thumb_x, thumb_opacity, track_opacity)
        scroll_event: Option<ScrollEvent>,
    ) {
        let _span = tracing::trace_span!(
            "renderer.tessellate_grid",
            rows = grid.len(),
            cols = grid.first().map(|row| row.len()).unwrap_or(0),
            dirty_rows = dirty_rows.iter().filter(|&&dirty| dirty).count()
        )
        .entered();

        if self.rows.len() > grid.len() {
            self.rows.truncate(grid.len());
        }
        if self.row_ranges.len() > grid.len() {
            self.row_ranges.truncate(grid.len());
        }
        while self.rows.len() < grid.len() {
            let cols = grid.get(self.rows.len()).map(|row| row.len()).unwrap_or(0);
            self.rows.push(RowTessellation::with_cell_capacity(cols));
        }
        while self.row_ranges.len() < grid.len() {
            self.row_ranges.push(RowVertexRanges::default());
        }

        self.actual_dirty.clear();
        self.actual_dirty.extend_from_slice(dirty_rows);
        self.actual_dirty.resize(grid.len(), true);
        self.rebuilt_rows.clear();
        self.missing_glyphs.clear();

        let can_reuse_scroll =
            self.apply_scroll_reuse(scroll_event, grid.len(), cell_h, vp_h, selection);
        if !can_reuse_scroll && scroll_event.is_some() {
            self.actual_dirty.fill(true);
        }
        if can_reuse_scroll {
            if let Some(event) = scroll_event {
                if let Some((_, r)) = self.last_cursor {
                    if let Some(destination) = scrolled_row_destination(event, r, grid.len()) {
                        self.actual_dirty[destination] = true;
                    }
                }
                if let Some((_, r)) = cursor {
                    if r < self.actual_dirty.len() {
                        self.actual_dirty[r] = true;
                    }
                }
            }
        }

        // Mark cursor rows dirty if cursor state changed
        if cursor != self.last_cursor || cursor_visible_phase != self.last_cursor_visible {
            if let Some((_, r)) = self.last_cursor {
                if r < self.actual_dirty.len() {
                    self.actual_dirty[r] = true;
                }
            }
            if let Some((_, r)) = cursor {
                if r < self.actual_dirty.len() {
                    self.actual_dirty[r] = true;
                }
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
                    if r < self.actual_dirty.len() {
                        self.actual_dirty[r] = true;
                    }
                }
            }
            if let Some(sel) = &selection {
                let start = sel.start_row.min(sel.end_row);
                let end = sel.start_row.max(sel.end_row);
                for r in start..=end {
                    if r < self.actual_dirty.len() {
                        self.actual_dirty[r] = true;
                    }
                }
            }
        }
        self.last_selection = selection;

        let normalized_selection = selection.map(|sel| {
            if sel.start_row < sel.end_row
                || (sel.start_row == sel.end_row && sel.start_col <= sel.end_col)
            {
                (sel.start_row, sel.start_col, sel.end_row, sel.end_col)
            } else {
                (sel.end_row, sel.end_col, sel.start_row, sel.start_col)
            }
        });

        let inset_x = ((cell_w - native_cell_w).max(0.0) * 0.5).floor();
        let inset_y = ((cell_h - native_cell_h).max(0.0) * 0.5).floor();


        let ndc = |px_x: f32, px_y: f32| -> [f32; 2] {
            [(px_x / vp_w) * 2.0 - 1.0, (px_y / vp_h) * 2.0 - 1.0]
        };

        let color_to_f32 = |c: forge_core::color::Color| -> [f32; 4] {
            let srgb = c.to_srgb_linear();
            [srgb.r, srgb.g, srgb.b, srgb.a]
        };

        let push_quad = |verts: &mut Vec<GlyphVertex>,
                         tl: [f32; 2],
                         br: [f32; 2],
                         uv_tl: [f32; 2],
                         uv_br: [f32; 2],
                         fg: [f32; 4],
                         bg: [f32; 4]| {
            let tr = [br[0], tl[1]];
            let bl = [tl[0], br[1]];
            let uv_tr = [uv_br[0], uv_tl[1]];
            let uv_bl = [uv_tl[0], uv_br[1]];

            verts.push(GlyphVertex {
                position: tl,
                tex_coord: uv_tl,
                fg_color: fg,
                bg_color: bg,
            });
            verts.push(GlyphVertex {
                position: tr,
                tex_coord: uv_tr,
                fg_color: fg,
                bg_color: bg,
            });
            verts.push(GlyphVertex {
                position: bl,
                tex_coord: uv_bl,
                fg_color: fg,
                bg_color: bg,
            });
            verts.push(GlyphVertex {
                position: tr,
                tex_coord: uv_tr,
                fg_color: fg,
                bg_color: bg,
            });
            verts.push(GlyphVertex {
                position: br,
                tex_coord: uv_br,
                fg_color: fg,
                bg_color: bg,
            });
            verts.push(GlyphVertex {
                position: bl,
                tex_coord: uv_bl,
                fg_color: fg,
                bg_color: bg,
            });
        };

        let push_atlas_glyph = |verts: &mut Vec<GlyphVertex>,
                                c: char,
                                cell: &Cell,
                                glyph: &super::font::atlas::GlyphMetrics,
                                px_x: f32,
                                origin_y: f32,
                                fg: [f32; 4],
                                bg: [f32; 4]| {
            let origin_x = (px_x + inset_x).round();
            let mut g_x = (origin_x + glyph.bearing_x as f32).round();
            let g_y = (origin_y + baseline - glyph.bearing_y as f32).round();
            let mut g_w = glyph.width as f32;
            let g_h = glyph.height as f32;

            if cell.is_italic() {
                g_x += (native_cell_h * 0.2).round();
                g_w += (native_cell_w * 0.2).round();
            }

            // Glyph Inflation for Powerline / connecting symbols
            if ('\u{E0B0}'..='\u{E0D4}').contains(&c) {
                g_x -= 0.75;
                g_w += 1.5;
            }

            let g_tl = ndc(g_x, g_y);
            let g_br = ndc(g_x + g_w, g_y + g_h);
            push_quad(
                verts,
                g_tl,
                g_br,
                [glyph.u0, glyph.v0],
                [glyph.u1, glyph.v1],
                fg,
                bg,
            );
        };

        let flush_background_run =
            |row_tess: &mut RowTessellation, start_x: f32, end_x: f32, px_y: f32, bg: [f32; 4]| {
                if end_x <= start_x {
                    return;
                }

                let tl = ndc(start_x, px_y);
                let br = ndc(end_x, px_y + cell_h);
                let uv = [-1.0, 0.0];
                push_quad(&mut row_tess.bg_vertices, tl, br, uv, uv, bg, bg);
            };

        for (row_idx, row) in grid.iter().enumerate() {
            if !self.actual_dirty[row_idx] {
                continue;
            }
            self.rebuilt_rows.push(row_idx);

            let row_tess = &mut self.rows[row_idx];
            row_tess.prepare_for_row(row.len());
            row_tess.generation = row_tess.generation.wrapping_add(1);
            let row_px_y = row_idx as f32 * cell_h + pad_y;
            let origin_y = (row_px_y + inset_y).round();
            let mut background_run: Option<(f32, f32, [f32; 4])> = None;

            for (col_idx, cell) in row.iter().enumerate() {
                if cell.c == '\0' {
                    if let Some((start_x, end_x, bg)) = background_run.take() {
                        flush_background_run(row_tess, start_x, end_x, row_px_y, bg);
                    }
                    continue;
                }

                let px_x = col_idx as f32 * cell_w + pad_x;
                let px_y = row_px_y;

                let mut fg = color_to_f32(cell.fg);
                let mut bg = color_to_f32(cell.bg);

                // Check if in selection
                let mut in_selection = false;
                if let Some((s_r, s_c, e_r, e_c)) = normalized_selection {
                    if row_idx > s_r && row_idx < e_r {
                        in_selection = true;
                    } else if row_idx == s_r && row_idx == e_r {
                        if col_idx >= s_c && col_idx <= e_c {
                            in_selection = true;
                        }
                    } else if (row_idx == s_r && col_idx >= s_c)
                        || (row_idx == e_r && col_idx <= e_c)
                    {
                        in_selection = true;
                    }
                }

                if in_selection {
                    bg = selection_bg;
                }

                let is_cursor = cursor_visible_phase
                    && cursor.is_some_and(|c| col_idx == c.0 && row_idx == c.1);
                let is_block_cursor =
                    is_cursor && cursor_style == forge_core::config_registry::CursorStyle::Block;
                if is_block_cursor {
                    fg = default_bg;
                    bg = cursor_color;
                }

                let mut quad_w = cell_w;
                if is_cursor || in_selection {
                    let c = cell.c;
                    if c != ' ' {
                        if let Some(glyph) =
                            glyph_for(atlas, &mut self.missing_glyphs, c, cell.is_bold())
                        {
                            let actual_w = glyph.width as f32;
                            if actual_w > cell_w {
                                quad_w = actual_w;
                            }
                        }
                    }
                }

                let needs_background =
                    (bg[3] > 0.0 && bg != default_bg) || in_selection || is_block_cursor;
                if needs_background {
                    let end_x = px_x + quad_w;
                    match background_run.as_mut() {
                        Some((_, run_end_x, run_bg))
                            if *run_bg == bg && (*run_end_x - px_x).abs() < 0.001 =>
                        {
                            *run_end_x = end_x;
                        }
                        Some(_) => {
                            if let Some((start_x, run_end_x, run_bg)) =
                                background_run.replace((px_x, end_x, bg))
                            {
                                flush_background_run(
                                    row_tess, start_x, run_end_x, row_px_y, run_bg,
                                );
                            }
                        }
                        None => {
                            background_run = Some((px_x, end_x, bg));
                        }
                    }
                } else if let Some((start_x, end_x, bg)) = background_run.take() {
                    flush_background_run(row_tess, start_x, end_x, row_px_y, bg);
                }

                if is_cursor && cursor_style == forge_core::config_registry::CursorStyle::Underline
                {
                    let tl = ndc(px_x, px_y + cell_h - 2.0);
                    let br = ndc(px_x + cell_w, px_y + cell_h);
                    let uv = [-1.0, 0.0];
                    push_quad(
                        &mut row_tess.fg_vertices,
                        tl,
                        br,
                        uv,
                        uv,
                        cursor_color,
                        cursor_color,
                    );
                }

                if is_cursor && cursor_style == forge_core::config_registry::CursorStyle::Beam {
                    let tl = ndc(px_x, px_y);
                    let br = ndc(px_x + 1.0, px_y + cell_h);
                    let uv = [-1.0, 0.0];
                    push_quad(
                        &mut row_tess.fg_vertices,
                        tl,
                        br,
                        uv,
                        uv,
                        cursor_color,
                        cursor_color,
                    );
                }

                let c = cell.c;
                if c != ' ' {
                    if c.is_ascii() {
                        if let Some(glyph) =
                            glyph_for(atlas, &mut self.missing_glyphs, c, cell.is_bold())
                        {
                            push_atlas_glyph(
                                &mut row_tess.fg_vertices,
                                c,
                                cell,
                                glyph,
                                px_x,
                                origin_y,
                                fg,
                                bg,
                            );
                        }
                    } else {
                        match c {
                            '\u{2580}'..='\u{259F}' => {
                                let mut block_tl_x = px_x;
                                let mut block_tl_y = px_y;
                                let mut block_br_x = px_x + cell_w;
                                let mut block_br_y = px_y + cell_h;

                                match c {
                                    '\u{2580}' => block_br_y = px_y + (cell_h / 2.0), // Upper Half
                                    '\u{2581}'..='\u{2587}' => {
                                        // Lower fractions
                                        let step = (c as u32 - 0x2580) as f32;
                                        let h = (cell_h * step) / 8.0;
                                        block_tl_y = px_y + cell_h - h;
                                    }
                                    '\u{2588}'..='\u{258F}' => {
                                        // Left fractions (including Full Block)
                                        let step = 8.0 - (c as u32 - 0x2588) as f32;
                                        let w = (cell_w * step) / 8.0;
                                        block_br_x = px_x + w;
                                    }
                                    '\u{2590}' => block_tl_x = px_x + (cell_w / 2.0), // Right Half
                                    '\u{2594}' => {
                                        // Upper One Eighth
                                        let h = cell_h / 8.0;
                                        block_br_y = px_y + h;
                                    }
                                    '\u{2595}' => {
                                        // Right One Eighth
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
                            '\u{E0B0}' | '\u{E0B2}' | '\u{E0B4}' | '\u{E0B6}' | '\u{E0B8}'
                            | '\u{E0BA}' | '\u{E0BC}' | '\u{E0BE}' => {
                                let mut g_x = px_x;
                                let mut g_w = cell_w;

                                // Right-pointing shapes bleed right
                                if c == '\u{E0B0}'
                                    || c == '\u{E0B4}'
                                    || c == '\u{E0B8}'
                                    || c == '\u{E0BC}'
                                {
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
                                push_quad(
                                    &mut row_tess.fg_vertices,
                                    g_tl,
                                    g_br,
                                    uv_tl,
                                    uv_br,
                                    fg,
                                    bg,
                                );
                            }
                            _ => {
                                if let Some((u, d, l, r, rnd)) = decode_box_drawing(c) {
                                    let g_tl = ndc(px_x, px_y);
                                    let g_br = ndc(px_x + cell_w, px_y + cell_h);
                                    let encoded =
                                        u | (d << 2) | (l << 4) | (r << 6) | ((rnd as u32) << 8);
                                    let proc_id = -100.0 - encoded as f32;
                                    let uv_tl = [proc_id, 0.0];
                                    let uv_br = [proc_id + 1.0, 1.0];
                                    push_quad(
                                        &mut row_tess.fg_vertices,
                                        g_tl,
                                        g_br,
                                        uv_tl,
                                        uv_br,
                                        fg,
                                        bg,
                                    );
                                } else if ('\u{2800}'..='\u{28FF}').contains(&c) {
                                    let pattern = c as u32 - 0x2800;
                                    let g_tl = ndc(px_x, px_y);
                                    let g_br = ndc(px_x + cell_w, px_y + cell_h);
                                    let proc_id = -500.0 - pattern as f32;
                                    let uv_tl = [proc_id, 0.0];
                                    let uv_br = [proc_id + 1.0, 1.0];
                                    push_quad(
                                        &mut row_tess.fg_vertices,
                                        g_tl,
                                        g_br,
                                        uv_tl,
                                        uv_br,
                                        fg,
                                        bg,
                                    );
                                } else if let Some(glyph) =
                                    glyph_for(atlas, &mut self.missing_glyphs, c, cell.is_bold())
                                {
                                    push_atlas_glyph(
                                        &mut row_tess.fg_vertices,
                                        c,
                                        cell,
                                        glyph,
                                        px_x,
                                        origin_y,
                                        fg,
                                        bg,
                                    );
                                }
                            }
                        }
                    }
                }

                if cell.is_underline() {
                    let thickness = 1.0;
                    let mut y = (origin_y + baseline + 2.0).round();
                    if y + thickness > px_y + cell_h {
                        y = (px_y + cell_h - thickness).floor();
                    }
                    let u_tl = ndc(px_x, y);
                    let u_br = ndc(px_x + cell_w, y + thickness);
                    let u_uv = [-1.0, 0.0];
                    push_quad(&mut row_tess.fg_vertices, u_tl, u_br, u_uv, u_uv, fg, fg);
                }

                if cell.is_strikethrough() {
                    let thickness = 1.0;
                    let y = (origin_y + baseline - (native_cell_h * 0.3)).round();
                    let s_tl = ndc(px_x, y);
                    let s_br = ndc(px_x + cell_w, y + thickness);
                    let s_uv = [-1.0, 0.0];
                    push_quad(&mut row_tess.fg_vertices, s_tl, s_br, s_uv, s_uv, fg, fg);
                }
            }

            if let Some((start_x, end_x, bg)) = background_run.take() {
                flush_background_run(row_tess, start_x, end_x, row_px_y, bg);
            }
        }

        self.vertices.clear();
        self.scrollbar_range = None;

        for (row_idx, row) in self.rows.iter().enumerate() {
            let start = self.vertices.len();
            self.vertices.extend_from_slice(&row.bg_vertices);
            self.row_ranges[row_idx].bg = VertexRange::new(start, row.bg_vertices.len());
            self.row_ranges[row_idx].generation = row.generation;
        }

        if let Some((thumb_y, thumb_height, thumb_width, thumb_x, thumb_opacity, track_opacity)) =
            scrollbar
        {
            let scrollbar_start = self.vertices.len();
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
            let scrollbar_count = self.vertices.len() - scrollbar_start;
            if scrollbar_count > 0 {
                self.scrollbar_range = Some(VertexRange::new(scrollbar_start, scrollbar_count));
            }
        }

        for (row_idx, row) in self.rows.iter().enumerate() {
            let start = self.vertices.len();
            self.vertices.extend_from_slice(&row.fg_vertices);
            self.row_ranges[row_idx].fg = VertexRange::new(start, row.fg_vertices.len());
            self.row_ranges[row_idx].generation = row.generation;
        }

        tracing::trace!(
            vertices = self.vertices.len(),
            vertex_capacity = self.vertices.capacity(),
            rows = self.rows.len(),
            "Tessellation complete"
        );
    }
}

const NO_BOX_DRAWING: u16 = 0xFFFF;

const BOX_DRAWING_TABLE: [u16; 128] = [
    0x050,
    0x0A0,
    0x005,
    0x00A,
    NO_BOX_DRAWING,
    NO_BOX_DRAWING,
    NO_BOX_DRAWING,
    NO_BOX_DRAWING,
    NO_BOX_DRAWING,
    NO_BOX_DRAWING,
    NO_BOX_DRAWING,
    NO_BOX_DRAWING,
    0x044,
    0x084,
    0x048,
    0x088,
    0x014,
    0x024,
    0x018,
    0x028,
    0x041,
    0x081,
    0x042,
    0x082,
    0x011,
    0x021,
    0x012,
    0x022,
    0x045,
    0x085,
    0x046,
    0x049,
    0x04A,
    0x086,
    0x089,
    0x08A,
    0x015,
    0x025,
    0x016,
    0x019,
    0x01A,
    0x026,
    0x029,
    0x02A,
    0x054,
    0x064,
    0x094,
    0x0A4,
    0x058,
    0x068,
    0x098,
    0x0A8,
    0x051,
    0x061,
    0x091,
    0x0A1,
    0x052,
    0x062,
    0x092,
    0x0A2,
    0x055,
    0x065,
    0x095,
    0x0A5,
    0x056,
    0x059,
    0x05A,
    0x066,
    0x069,
    0x096,
    0x099,
    0x06A,
    0x09A,
    0x0A6,
    0x0A9,
    0x0AA,
    NO_BOX_DRAWING,
    NO_BOX_DRAWING,
    NO_BOX_DRAWING,
    NO_BOX_DRAWING,
    0x0F0,
    0x00F,
    0x0C4,
    0x04C,
    0x0CC,
    0x034,
    0x01C,
    0x03C,
    0x0C1,
    0x043,
    0x0C3,
    0x031,
    0x013,
    0x033,
    0x0C5,
    0x04F,
    0x0CF,
    0x035,
    0x01F,
    0x03F,
    0x0F4,
    0x05C,
    0x0FC,
    0x0F1,
    0x053,
    0x0F3,
    0x0F5,
    0x05F,
    0x0FF,
    0x144,
    0x114,
    0x111,
    0x141,
    NO_BOX_DRAWING,
    NO_BOX_DRAWING,
    NO_BOX_DRAWING,
    0x010,
    0x001,
    0x040,
    0x004,
    0x020,
    0x002,
    0x080,
    0x008,
    0x090,
    0x009,
    0x060,
    0x006,
];

fn decode_box_drawing(c: char) -> Option<(u32, u32, u32, u32, bool)> {
    let code = c as usize;
    if !(0x2500..=0x257F).contains(&code) {
        return None;
    }

    let encoded = BOX_DRAWING_TABLE[code - 0x2500];
    if encoded == NO_BOX_DRAWING {
        return None;
    }

    Some((
        (encoded & 0x0003) as u32,
        ((encoded >> 2) & 0x0003) as u32,
        ((encoded >> 4) & 0x0003) as u32,
        ((encoded >> 6) & 0x0003) as u32,
        (encoded & 0x0100) != 0,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::atlas::{GlyphAtlas, GlyphAtlasDescriptor, GlyphMetrics};
    use forge_core::cell::Cell;
    use forge_core::color::Color;
    use forge_core::config_registry::CursorStyle;
    use std::collections::HashMap;

    fn test_atlas() -> GlyphAtlas {
        let mut glyphs = HashMap::new();
        for c in 0x20_u8..=0x7e {
            glyphs.insert(
                c as char,
                GlyphMetrics {
                    u0: 0.0,
                    v0: 0.0,
                    u1: 1.0,
                    v1: 1.0,
                    width: 8,
                    height: 16,
                    bearing_y: 14,
                    bearing_x: 0,
                },
            );
        }

        GlyphAtlas {
            pixels: vec![255; 4],
            atlas_width: 1,
            atlas_height: 1,
            glyphs,
            glyphs_bold: HashMap::new(),
            descriptor: GlyphAtlasDescriptor::dummy(),
            atlas_cell_width: 1,
            atlas_cell_height: 1,
            next_dynamic_slot: 1,
            total_slots: 1,
        }
    }

    fn test_grid(cols: usize, rows: usize) -> Vec<Vec<Cell>> {
        let fg = Color {
            r: 192,
            g: 202,
            b: 245,
            a: 255,
        };
        let bg = Color {
            r: 26,
            g: 27,
            b: 38,
            a: 255,
        };

        (0..rows)
            .map(|row| {
                (0..cols)
                    .map(|col| Cell {
                        c: (b'a' + ((row + col) % 26) as u8) as char,
                        fg,
                        bg,
                        flags: 0,
                    })
                    .collect()
            })
            .collect()
    }

    fn tessellate_test_grid(
        tessellator: &mut GridTessellator,
        atlas: &GlyphAtlas,
        grid: &[Vec<Cell>],
    ) {
        let dirty_rows = vec![true; grid.len()];
        tessellate_test_grid_with_dirty(tessellator, atlas, grid, &dirty_rows, None);
    }

    fn tessellate_test_grid_with_dirty(
        tessellator: &mut GridTessellator,
        atlas: &GlyphAtlas,
        grid: &[Vec<Cell>],
        dirty_rows: &[bool],
        scrollbar: Option<(f32, f32, f32, f32, f32, f32)>,
    ) {
        tessellate_test_grid_with_cursor(
            tessellator,
            atlas,
            grid,
            dirty_rows,
            Some((1, 1)),
            true,
            scrollbar,
            None,
        );
    }

    fn tessellate_test_grid_with_cursor(
        tessellator: &mut GridTessellator,
        atlas: &GlyphAtlas,
        grid: &[Vec<Cell>],
        dirty_rows: &[bool],
        cursor: Option<(usize, usize)>,
        cursor_visible: bool,
        scrollbar: Option<(f32, f32, f32, f32, f32, f32)>,
        scroll_event: Option<ScrollEvent>,
    ) {
        let refs: Vec<&[Cell]> = grid.iter().map(Vec::as_slice).collect();
        tessellator.tessellate(
            &refs,
            dirty_rows,
            atlas,
            10.0,
            20.0,
            10.0,
            20.0,
            16.0,
            1200.0,
            800.0,
            [0.01, 0.01, 0.01, 1.0],
            [0.8, 0.8, 0.8, 1.0],
            cursor,
            CursorStyle::Block,
            cursor_visible,
            None,
            [0.1, 0.1, 0.2, 0.8],
            4.0,
            4.0,
            scrollbar,
            scroll_event,
        );
    }

    fn px_x(ndc_x: f32, viewport_w: f32) -> f32 {
        ((ndc_x + 1.0) * 0.5) * viewport_w
    }

    fn px_y(ndc_y: f32, viewport_h: f32) -> f32 {
        ((ndc_y + 1.0) * 0.5) * viewport_h
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.0001,
            "expected {actual} to be within tolerance of {expected}"
        );
    }

    fn assert_vertices_equal(actual: &[GlyphVertex], expected: &[GlyphVertex]) {
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert_eq!(actual.position, expected.position);
            assert_eq!(actual.tex_coord, expected.tex_coord);
            assert_eq!(actual.fg_color, expected.fg_color);
            assert_eq!(actual.bg_color, expected.bg_color);
        }
    }

    #[test]
    fn tessellation_reuses_capacity_after_warmup() {
        let atlas = test_atlas();
        let grid = test_grid(20, 6);
        let mut tessellator = GridTessellator::new(20 * 6);

        tessellate_test_grid(&mut tessellator, &atlas, &grid);
        let first_vertex_count = tessellator.vertices.len();
        let capacities: Vec<(usize, usize)> = tessellator
            .rows
            .iter()
            .map(|row| (row.bg_vertices.capacity(), row.fg_vertices.capacity()))
            .collect();
        let vertex_capacity = tessellator.vertices.capacity();
        let dirty_capacity = tessellator.actual_dirty.capacity();

        tessellate_test_grid(&mut tessellator, &atlas, &grid);

        assert_eq!(tessellator.vertices.len(), first_vertex_count);
        assert_eq!(tessellator.vertices.capacity(), vertex_capacity);
        assert_eq!(tessellator.actual_dirty.capacity(), dirty_capacity);
        let second_capacities: Vec<(usize, usize)> = tessellator
            .rows
            .iter()
            .map(|row| (row.bg_vertices.capacity(), row.fg_vertices.capacity()))
            .collect();
        assert_eq!(second_capacities, capacities);
    }

    #[test]
    fn tessellation_drops_removed_row_buffers_on_shrink() {
        let atlas = test_atlas();
        let large_grid = test_grid(10, 6);
        let small_grid = test_grid(10, 2);
        let mut tessellator = GridTessellator::new(10 * 6);

        tessellate_test_grid(&mut tessellator, &atlas, &large_grid);
        assert_eq!(tessellator.rows.len(), 6);

        tessellate_test_grid(&mut tessellator, &atlas, &small_grid);
        assert_eq!(tessellator.rows.len(), 2);
        assert_eq!(tessellator.row_ranges.len(), 2);
    }

    #[test]
    fn row_vertex_ranges_match_final_vertex_assembly() {
        let atlas = test_atlas();
        let grid = test_grid(8, 3);
        let mut tessellator = GridTessellator::new(8 * 3);

        tessellate_test_grid_with_dirty(
            &mut tessellator,
            &atlas,
            &grid,
            &[true, true, true],
            Some((20.0, 80.0, 5.0, 1180.0, 1.0, 0.5)),
        );

        assert_eq!(tessellator.row_ranges.len(), tessellator.rows.len());

        for (row_idx, row) in tessellator.rows.iter().enumerate() {
            let ranges = tessellator.row_ranges[row_idx];
            let bg_end = ranges.bg.start + ranges.bg.count;
            let fg_end = ranges.fg.start + ranges.fg.count;

            assert_vertices_equal(
                &tessellator.vertices[ranges.bg.start..bg_end],
                &row.bg_vertices,
            );
            assert_vertices_equal(
                &tessellator.vertices[ranges.fg.start..fg_end],
                &row.fg_vertices,
            );
            assert_eq!(ranges.generation, row.generation);
        }

        let scrollbar_range = tessellator
            .scrollbar_range
            .expect("visible scrollbar should have an assembled range");
        let bg_end = tessellator
            .row_ranges
            .iter()
            .map(|ranges| ranges.bg.start + ranges.bg.count)
            .max()
            .unwrap_or(0);
        let first_fg_start = tessellator
            .row_ranges
            .iter()
            .map(|ranges| ranges.fg.start)
            .min()
            .unwrap_or(tessellator.vertices.len());

        assert!(scrollbar_range.start >= bg_end);
        assert!(scrollbar_range.start + scrollbar_range.count <= first_fg_start);
    }

    #[test]
    fn adjacent_equal_backgrounds_are_merged_into_one_quad() {
        let atlas = test_atlas();
        let grid = test_grid(4, 1);
        let mut tessellator = GridTessellator::new(4);

        tessellate_test_grid_with_cursor(
            &mut tessellator,
            &atlas,
            &grid,
            &[true],
            None,
            false,
            None,
            None,
        );

        assert_eq!(tessellator.rows[0].bg_vertices.len(), 6);
        assert_close(
            px_x(tessellator.rows[0].bg_vertices[0].position[0], 1200.0),
            4.0,
        );
        assert_close(
            px_x(tessellator.rows[0].bg_vertices[4].position[0], 1200.0),
            44.0,
        );
    }

    #[test]
    fn background_runs_split_across_different_background_cells() {
        let atlas = test_atlas();
        let mut grid = test_grid(4, 1);
        grid[0][1].bg = Color {
            r: 3,
            g: 3,
            b: 3,
            a: 255,
        };
        let mut tessellator = GridTessellator::new(4);

        tessellate_test_grid_with_cursor(
            &mut tessellator,
            &atlas,
            &grid,
            &[true],
            None,
            false,
            None,
            None,
        );

        assert_eq!(tessellator.rows[0].bg_vertices.len(), 18);
    }

    #[test]
    fn dirty_row_rebuild_updates_only_that_row_generation() {
        let atlas = test_atlas();
        let grid = test_grid(8, 3);
        let mut tessellator = GridTessellator::new(8 * 3);

        tessellate_test_grid_with_dirty(&mut tessellator, &atlas, &grid, &[true, true, true], None);
        let first_generations: Vec<u64> = tessellator
            .row_ranges
            .iter()
            .map(|ranges| ranges.generation)
            .collect();

        tessellate_test_grid_with_dirty(
            &mut tessellator,
            &atlas,
            &grid,
            &[false, true, false],
            None,
        );
        let second_generations: Vec<u64> = tessellator
            .row_ranges
            .iter()
            .map(|ranges| ranges.generation)
            .collect();

        assert_eq!(second_generations[0], first_generations[0]);
        assert_eq!(second_generations[1], first_generations[1] + 1);
        assert_eq!(second_generations[2], first_generations[2]);
    }

    #[test]
    fn cursor_blink_rebuilds_only_cursor_row_when_cells_are_clean() {
        let atlas = test_atlas();
        let grid = test_grid(8, 3);
        let mut tessellator = GridTessellator::new(8 * 3);

        tessellate_test_grid_with_cursor(
            &mut tessellator,
            &atlas,
            &grid,
            &[true, true, true],
            Some((1, 1)),
            true,
            None,
            None,
        );
        tessellate_test_grid_with_cursor(
            &mut tessellator,
            &atlas,
            &grid,
            &[false, false, false],
            Some((1, 1)),
            false,
            None,
            None,
        );

        assert_eq!(tessellator.rebuilt_rows, vec![1]);
    }

    #[test]
    fn cursor_move_rebuilds_old_and_new_cursor_rows_when_cells_are_clean() {
        let atlas = test_atlas();
        let grid = test_grid(8, 4);
        let mut tessellator = GridTessellator::new(8 * 4);

        tessellate_test_grid_with_cursor(
            &mut tessellator,
            &atlas,
            &grid,
            &[true, true, true, true],
            Some((1, 1)),
            true,
            None,
            None,
        );
        tessellate_test_grid_with_cursor(
            &mut tessellator,
            &atlas,
            &grid,
            &[false, false, false, false],
            Some((1, 2)),
            true,
            None,
            None,
        );

        assert_eq!(tessellator.rebuilt_rows, vec![1, 2]);
    }

    #[test]
    fn full_viewport_scroll_up_reuses_translated_rows_and_rebuilds_exposed_bottom() {
        let atlas = test_atlas();
        let grid = test_grid(8, 4);
        let mut scrolled_grid = vec![grid[1].clone(), grid[2].clone(), grid[3].clone()];
        scrolled_grid.push(test_grid(8, 1).remove(0));
        let mut tessellator = GridTessellator::new(8 * 4);

        tessellate_test_grid_with_cursor(
            &mut tessellator,
            &atlas,
            &grid,
            &[true, true, true, true],
            None,
            true,
            None,
            None,
        );
        let initial_generations: Vec<u64> = tessellator
            .row_ranges
            .iter()
            .map(|ranges| ranges.generation)
            .collect();

        tessellate_test_grid_with_cursor(
            &mut tessellator,
            &atlas,
            &scrolled_grid,
            &[true, true, true, true],
            None,
            true,
            None,
            Some(ScrollEvent {
                direction: ScrollDirection::Up,
                top: 0,
                bottom: 3,
                lines: 1,
                full_viewport: true,
            }),
        );

        assert_eq!(tessellator.rebuilt_rows, vec![3]);
        assert_eq!(
            tessellator.row_ranges[0].generation,
            initial_generations[1] + 1
        );
        assert_eq!(
            tessellator.row_ranges[1].generation,
            initial_generations[2] + 1
        );
        assert_eq!(
            tessellator.row_ranges[2].generation,
            initial_generations[3] + 1
        );
    }

    #[test]
    fn full_viewport_scroll_down_reuses_translated_rows_and_rebuilds_exposed_top() {
        let atlas = test_atlas();
        let grid = test_grid(8, 4);
        let mut scrolled_grid = vec![test_grid(8, 1).remove(0)];
        scrolled_grid.extend_from_slice(&grid[..3]);
        let mut tessellator = GridTessellator::new(8 * 4);

        tessellate_test_grid_with_cursor(
            &mut tessellator,
            &atlas,
            &grid,
            &[true, true, true, true],
            None,
            true,
            None,
            None,
        );
        let initial_generations: Vec<u64> = tessellator
            .row_ranges
            .iter()
            .map(|ranges| ranges.generation)
            .collect();

        tessellate_test_grid_with_cursor(
            &mut tessellator,
            &atlas,
            &scrolled_grid,
            &[true, true, true, true],
            None,
            true,
            None,
            Some(ScrollEvent {
                direction: ScrollDirection::Down,
                top: 0,
                bottom: 3,
                lines: 1,
                full_viewport: true,
            }),
        );

        assert_eq!(tessellator.rebuilt_rows, vec![0]);
        assert_eq!(
            tessellator.row_ranges[1].generation,
            initial_generations[0] + 1
        );
        assert_eq!(
            tessellator.row_ranges[2].generation,
            initial_generations[1] + 1
        );
        assert_eq!(
            tessellator.row_ranges[3].generation,
            initial_generations[2] + 1
        );
    }

    #[test]
    fn scroll_reuse_falls_back_to_full_rebuild_when_selection_is_active() {
        let atlas = test_atlas();
        let grid = test_grid(8, 4);
        let refs: Vec<&[Cell]> = grid.iter().map(Vec::as_slice).collect();
        let mut tessellator = GridTessellator::new(8 * 4);

        tessellator.tessellate(
            &refs,
            &[true, true, true, true],
            &atlas,
            10.0,
            20.0,
            10.0,
            20.0,
            16.0,
            1200.0,
            800.0,
            [0.01, 0.01, 0.01, 1.0],
            [0.8, 0.8, 0.8, 1.0],
            None,
            CursorStyle::Block,
            true,
            Some(forge_core::cell::SelectionRange {
                start_row: 1,
                start_col: 0,
                end_row: 1,
                end_col: 2,
            }),
            [0.1, 0.1, 0.2, 0.8],
            4.0,
            4.0,
            None,
            Some(ScrollEvent {
                direction: ScrollDirection::Up,
                top: 0,
                bottom: 3,
                lines: 1,
                full_viewport: true,
            }),
        );

        assert_eq!(tessellator.rebuilt_rows, vec![0, 1, 2, 3]);
    }

    #[test]
    fn atlas_glyphs_remain_native_sized_and_pixel_snapped_in_filled_cells() {
        let atlas = test_atlas();
        let fg = Color {
            r: 192,
            g: 202,
            b: 245,
            a: 255,
        };
        let bg = Color {
            r: 26,
            g: 27,
            b: 38,
            a: 255,
        };
        let grid = vec![vec![Cell {
            c: 'A',
            fg,
            bg,
            flags: 0,
        }]];
        let refs: Vec<&[Cell]> = grid.iter().map(Vec::as_slice).collect();
        let dirty_rows = vec![true];
        let mut tessellator = GridTessellator::new(1);

        tessellator.tessellate(
            &refs,
            &dirty_rows,
            &atlas,
            10.5,
            21.5,
            10.0,
            20.0,
            16.0,
            100.0,
            100.0,
            [0.01, 0.01, 0.01, 1.0],
            [0.8, 0.8, 0.8, 1.0],
            None,
            CursorStyle::Block,
            true,
            None,
            [0.1, 0.1, 0.2, 0.8],
            4.25,
            3.75,
            None,
            None,
        );

        let vertices = &tessellator.rows[0].fg_vertices;
        assert_eq!(vertices.len(), 6);

        let left = px_x(vertices[0].position[0], 100.0);
        let top = px_y(vertices[0].position[1], 100.0);
        let right = px_x(vertices[1].position[0], 100.0);
        let bottom = px_y(vertices[2].position[1], 100.0);

        assert_close(left, 4.0);
        assert_close(top, 6.0);
        assert_close(right - left, 8.0);
        assert_close(bottom - top, 16.0);
    }

    #[test]
    fn box_drawing_table_decodes_representative_glyphs() {
        assert_eq!(decode_box_drawing('─'), Some((0, 0, 1, 1, false)));
        assert_eq!(decode_box_drawing('┃'), Some((2, 2, 0, 0, false)));
        assert_eq!(decode_box_drawing('╋'), Some((2, 2, 2, 2, false)));
        assert_eq!(decode_box_drawing('╭'), Some((0, 1, 0, 1, true)));
        assert_eq!(decode_box_drawing('╿'), Some((2, 1, 0, 0, false)));
        assert_eq!(decode_box_drawing('┄'), None);
        assert_eq!(decode_box_drawing('A'), None);
    }

    #[test]
    fn tessellation_records_missing_atlas_glyphs() {
        let atlas = test_atlas();
        let fg = Color {
            r: 192,
            g: 202,
            b: 245,
            a: 255,
        };
        let bg = Color {
            r: 26,
            g: 27,
            b: 38,
            a: 255,
        };
        let grid = vec![vec![Cell {
            c: 'Ω',
            fg,
            bg,
            flags: 0,
        }]];
        let refs: Vec<&[Cell]> = grid.iter().map(Vec::as_slice).collect();
        let mut tessellator = GridTessellator::new(1);

        tessellator.tessellate(
            &refs,
            &[true],
            &atlas,
            10.0,
            20.0,
            10.0,
            20.0,
            16.0,
            100.0,
            100.0,
            [0.01, 0.01, 0.01, 1.0],
            [0.8, 0.8, 0.8, 1.0],
            None,
            CursorStyle::Block,
            true,
            None,
            [0.1, 0.1, 0.2, 0.8],
            0.0,
            0.0,
            None,
            None,
        );

        assert!(tessellator.missing_glyphs().contains(&GlyphKey {
            c: 'Ω',
            is_bold: false,
        }));
        assert!(!tessellator.rows[0].fg_vertices.is_empty());
    }

    #[test]
    fn tessellation_does_not_record_procedural_box_drawing_as_missing() {
        let atlas = test_atlas();
        let fg = Color {
            r: 192,
            g: 202,
            b: 245,
            a: 255,
        };
        let bg = Color {
            r: 26,
            g: 27,
            b: 38,
            a: 255,
        };
        let grid = vec![vec![Cell {
            c: '─',
            fg,
            bg,
            flags: 0,
        }]];
        let refs: Vec<&[Cell]> = grid.iter().map(Vec::as_slice).collect();
        let mut tessellator = GridTessellator::new(1);

        tessellator.tessellate(
            &refs,
            &[true],
            &atlas,
            10.0,
            20.0,
            10.0,
            20.0,
            16.0,
            100.0,
            100.0,
            [0.01, 0.01, 0.01, 1.0],
            [0.8, 0.8, 0.8, 1.0],
            None,
            CursorStyle::Block,
            true,
            None,
            [0.1, 0.1, 0.2, 0.8],
            0.0,
            0.0,
            None,
            None,
        );

        assert!(tessellator.missing_glyphs().is_empty());
    }
}
