use super::{
    layout::{DEFAULT_MIN_PANE_COLS, DEFAULT_MIN_PANE_ROWS},
    state::{Direction, SplitAxis},
    GridSize, PaneId,
};
use std::time::{Duration, Instant};

const DEFAULT_SCROLL_ANIMATION_DURATION: Duration = Duration::from_millis(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualPaneRect {
    pub col: i32,
    pub row: i32,
    pub cols: usize,
    pub rows: usize,
}

impl VirtualPaneRect {
    pub const fn new(col: i32, row: i32, cols: usize, rows: usize) -> Self {
        Self {
            col,
            row,
            cols,
            rows,
        }
    }

    fn right(self) -> i32 {
        self.col.saturating_add(self.cols as i32)
    }

    fn bottom(self) -> i32 {
        self.row.saturating_add(self.rows as i32)
    }

    fn contains_cell(self, col: i32, row: i32) -> bool {
        col >= self.col && row >= self.row && col < self.right() && row < self.bottom()
    }

    fn center_col(self) -> f32 {
        self.col as f32 + self.cols as f32 / 2.0
    }

    fn center_row(self) -> f32 {
        self.row as f32 + self.rows as f32 / 2.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrollingPane {
    pub id: PaneId,
    pub virtual_rect: VirtualPaneRect,
    pub grid_size: GridSize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollingOverflowIndicators {
    pub above: bool,
    pub below: bool,
    pub left: bool,
    pub right: bool,
}

impl ScrollingOverflowIndicators {
    pub const NONE: Self = Self {
        above: false,
        below: false,
        left: false,
        right: false,
    };

    pub const fn any(self) -> bool {
        self.above || self.below || self.left || self.right
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleScrollingPane {
    pub pane_id: PaneId,
    pub virtual_rect: VirtualPaneRect,
    pub viewport_col: i32,
    pub viewport_row: i32,
    pub visible_col_start: usize,
    pub visible_row_start: usize,
    pub visible_cols: usize,
    pub visible_rows: usize,
    pub grid_size: GridSize,
    pub overflow: ScrollingOverflowIndicators,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderScrollingPane {
    pub pane_id: PaneId,
    pub virtual_rect: VirtualPaneRect,
    pub viewport_col: f32,
    pub viewport_row: f32,
    pub visible_col_start: usize,
    pub visible_row_start: usize,
    pub visible_cols: usize,
    pub visible_rows: usize,
    pub grid_size: GridSize,
    pub overflow: ScrollingOverflowIndicators,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollingPanePointTarget {
    pub pane_id: PaneId,
    pub viewport_col: usize,
    pub viewport_row: usize,
    pub local_col: usize,
    pub local_row: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollingResizeHandle {
    pub axis: SplitAxis,
    pub first: PaneId,
    pub second: PaneId,
    boundary_start: i32,
    boundary_end: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScrollingResizeSide {
    Before,
    After,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScrollingResizePane {
    pane_id: PaneId,
    side: ScrollingResizeSide,
    initial_rect: VirtualPaneRect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrollingResizeDrag {
    pub handle: ScrollingResizeHandle,
    origin_virtual_col: i32,
    origin_virtual_row: i32,
    panes: Vec<ScrollingResizePane>,
}

impl ScrollingResizeDrag {
    pub fn affected_pane_ids(&self) -> impl Iterator<Item = PaneId> + '_ {
        self.panes.iter().map(|pane| pane.pane_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrollingPaneRemoval {
    pub removed: bool,
    pub grid_changes: Vec<(PaneId, GridSize)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VisibleCacheKey {
    layout_generation: u64,
    scroll_x_cols: i32,
    scroll_y_rows: i32,
    viewport_cols: usize,
    viewport_rows: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisiblePaneCache {
    key: Option<VisibleCacheKey>,
    panes: Vec<VisibleScrollingPane>,
}

impl VisiblePaneCache {
    fn new() -> Self {
        Self {
            key: None,
            panes: Vec::new(),
        }
    }

    pub fn panes(&self) -> &[VisibleScrollingPane] {
        &self.panes
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ScrollAnimation {
    Idle,
    Active {
        start_x: f32,
        start_y: f32,
        target_x: i32,
        target_y: i32,
        started_at: Instant,
        duration: Duration,
    },
}

impl ScrollAnimation {
    fn visual_offset(self, logical_x: i32, logical_y: i32, now: Instant) -> (f32, f32, bool) {
        match self {
            Self::Idle => (logical_x as f32, logical_y as f32, false),
            Self::Active {
                start_x,
                start_y,
                target_x,
                target_y,
                started_at,
                duration,
            } => {
                if duration.is_zero() {
                    return (target_x as f32, target_y as f32, false);
                }
                let elapsed = now.saturating_duration_since(started_at);
                if elapsed >= duration {
                    return (target_x as f32, target_y as f32, false);
                }
                let t = elapsed.as_secs_f32() / duration.as_secs_f32();
                let eased = ease_out_cubic(t.clamp(0.0, 1.0));
                (
                    start_x + (target_x as f32 - start_x) * eased,
                    start_y + (target_y as f32 - start_y) * eased,
                    true,
                )
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScrollingPaneManager {
    panes: Vec<ScrollingPane>,
    active_pane: Option<PaneId>,
    zoomed_pane: Option<PaneId>,
    viewport_cols: usize,
    viewport_rows: usize,
    scroll_x_cols: i32,
    scroll_y_rows: i32,
    pane_width_cols: usize,
    pane_height_rows: usize,
    gap_cols: usize,
    gap_rows: usize,
    next_pane_id: u64,
    layout_generation: u64,
    visible_cache: VisiblePaneCache,
    scroll_animation: ScrollAnimation,
}

impl ScrollingPaneManager {
    pub fn new(
        viewport_cols: usize,
        viewport_rows: usize,
        pane_width_cols: usize,
        pane_height_rows: usize,
    ) -> Self {
        Self {
            panes: Vec::new(),
            active_pane: None,
            zoomed_pane: None,
            viewport_cols: viewport_cols.max(1),
            viewport_rows: viewport_rows.max(1),
            scroll_x_cols: 0,
            scroll_y_rows: 0,
            pane_width_cols: pane_width_cols.max(1),
            pane_height_rows: pane_height_rows.max(1),
            gap_cols: 1,
            gap_rows: 1,
            next_pane_id: 1,
            layout_generation: 1,
            visible_cache: VisiblePaneCache::new(),
            scroll_animation: ScrollAnimation::Idle,
        }
    }

    pub fn with_gap(mut self, gap_cols: usize, gap_rows: usize) -> Self {
        self.gap_cols = gap_cols;
        self.gap_rows = gap_rows;
        self.invalidate_layout();
        self
    }

    pub fn pane_count(&self) -> usize {
        self.panes.len()
    }

    pub fn active_pane(&self) -> Option<PaneId> {
        self.active_pane
    }

    pub fn is_zoomed(&self) -> bool {
        self.zoomed_pane.is_some()
    }

    pub fn layout_generation(&self) -> u64 {
        self.layout_generation
    }

    pub fn scroll_offset(&self) -> (i32, i32) {
        (self.scroll_x_cols, self.scroll_y_rows)
    }

    pub fn visual_scroll_offset(&self, now: Instant) -> (f32, f32, bool) {
        self.scroll_animation
            .visual_offset(self.scroll_x_cols, self.scroll_y_rows, now)
    }

    pub fn has_active_scroll_animation(&self, now: Instant) -> bool {
        self.visual_scroll_offset(now).2
    }

    pub fn cancel_scroll_animation(&mut self) {
        self.scroll_animation = ScrollAnimation::Idle;
    }

    pub fn set_viewport_size(&mut self, cols: usize, rows: usize) -> Vec<(PaneId, GridSize)> {
        let cols = cols.max(1);
        let rows = rows.max(1);
        if self.viewport_cols == cols && self.viewport_rows == rows {
            return Vec::new();
        }
        self.viewport_cols = cols;
        self.viewport_rows = rows;

        let mut changes = Vec::new();
        if let Some(zoomed_id) = self.valid_zoomed_pane() {
            changes.push((zoomed_id, GridSize::new(cols, rows)));
        } else if self.panes.len() == 1 {
            if let Some(pane) = self.panes.first_mut() {
                let new_rect = VirtualPaneRect::new(0, 0, cols, rows);
                if pane.virtual_rect != new_rect || pane.grid_size != GridSize::new(cols, rows) {
                    pane.virtual_rect = new_rect;
                    pane.grid_size = GridSize::new(cols, rows);
                    changes.push((pane.id, pane.grid_size));
                }
            }
        }

        self.clamp_scroll();
        self.cancel_scroll_animation();
        self.invalidate_layout();
        changes
    }

    pub fn add_pane_at(&mut self, col: i32, row: i32) -> PaneId {
        let pane_id = PaneId::new(self.next_pane_id);
        self.next_pane_id = self.next_pane_id.saturating_add(1);
        self.add_existing_pane_at(pane_id, col, row);
        pane_id
    }

    pub fn add_existing_pane_at(&mut self, pane_id: PaneId, col: i32, row: i32) {
        self.next_pane_id = self.next_pane_id.max(pane_id.get().saturating_add(1));
        self.panes.push(ScrollingPane {
            id: pane_id,
            virtual_rect: VirtualPaneRect::new(
                col.max(0),
                row.max(0),
                self.pane_width_cols,
                self.pane_height_rows,
            ),
            grid_size: GridSize::new(self.pane_width_cols, self.pane_height_rows),
        });
        if self.active_pane.is_none() {
            self.active_pane = Some(pane_id);
        }
        self.invalidate_layout();
    }

    pub fn add_pane_right_of_active(&mut self) -> PaneId {
        let (col, row) = self
            .active_pane
            .and_then(|pane_id| self.pane(pane_id))
            .map(|pane| {
                (
                    pane.virtual_rect
                        .right()
                        .saturating_add(self.gap_cols as i32),
                    pane.virtual_rect.row,
                )
            })
            .unwrap_or((0, 0));
        let pane_id = self.add_pane_at(col, row);
        self.active_pane = Some(pane_id);
        self.scroll_pane_into_view(pane_id);
        pane_id
    }

    pub fn add_existing_pane_right_of_active(&mut self, pane_id: PaneId) {
        let _ = self.split_active_with_existing(SplitAxis::Vertical, pane_id);
    }

    pub fn add_existing_pane_below_active(&mut self, pane_id: PaneId) {
        let _ = self.split_active_with_existing(SplitAxis::Horizontal, pane_id);
    }

    pub fn planned_split_grid_size(&self, axis: SplitAxis) -> Option<GridSize> {
        let (_, _, new_rect, _) = self.planned_split_rects(axis)?;
        Some(GridSize::new(new_rect.cols, new_rect.rows))
    }

    pub fn split_active_with_existing(
        &mut self,
        axis: SplitAxis,
        pane_id: PaneId,
    ) -> Option<Vec<(PaneId, GridSize)>> {
        let (active_id, active_rect, new_rect, resized_active) = self.planned_split_rects(axis)?;
        let active_index = self.pane_index(active_id)?;

        self.next_pane_id = self.next_pane_id.max(pane_id.get().saturating_add(1));
        if resized_active {
            self.panes[active_index].virtual_rect = active_rect;
            self.panes[active_index].grid_size = GridSize::new(active_rect.cols, active_rect.rows);
        }
        self.panes.push(ScrollingPane {
            id: pane_id,
            virtual_rect: new_rect,
            grid_size: GridSize::new(new_rect.cols, new_rect.rows),
        });
        self.active_pane = Some(pane_id);
        self.normalize_geometry_gravity();
        self.invalidate_layout();
        self.scroll_pane_into_view(pane_id);

        let mut changes = Vec::with_capacity(2);
        if resized_active {
            changes.push((active_id, GridSize::new(active_rect.cols, active_rect.rows)));
        }
        changes.push((pane_id, GridSize::new(new_rect.cols, new_rect.rows)));
        Some(changes)
    }

    fn planned_split_rects(
        &self,
        axis: SplitAxis,
    ) -> Option<(PaneId, VirtualPaneRect, VirtualPaneRect, bool)> {
        let active_id = self.active_pane?;
        let original = self.pane(active_id)?.virtual_rect;
        match axis {
            SplitAxis::Vertical => {
                if original.cols < self.viewport_cols.saturating_sub(self.gap_cols).max(1) {
                    let new_rect = VirtualPaneRect::new(
                        original.right().saturating_add(self.gap_cols.max(1) as i32),
                        original.row,
                        original.cols,
                        original.rows,
                    );
                    return Some((active_id, original, new_rect, false));
                }

                let gap = self.gap_cols.min(original.cols.saturating_sub(2));
                let available_cols = original.cols.saturating_sub(gap);
                if available_cols < 2 {
                    return None;
                }
                let left_cols = available_cols / 2;
                let right_cols = available_cols.saturating_sub(left_cols);
                if left_cols == 0 || right_cols == 0 {
                    return None;
                }
                let active_rect =
                    VirtualPaneRect::new(original.col, original.row, left_cols, original.rows);
                let new_rect = VirtualPaneRect::new(
                    original
                        .col
                        .saturating_add(left_cols as i32)
                        .saturating_add(gap as i32),
                    original.row,
                    right_cols,
                    original.rows,
                );
                Some((active_id, active_rect, new_rect, true))
            }
            SplitAxis::Horizontal => {
                if original.rows < self.viewport_rows.saturating_sub(self.gap_rows).max(1) {
                    let new_rect = VirtualPaneRect::new(
                        original.col,
                        original
                            .bottom()
                            .saturating_add(self.gap_rows.max(1) as i32),
                        original.cols,
                        original.rows,
                    );
                    return Some((active_id, original, new_rect, false));
                }

                let gap = self.gap_rows.min(original.rows.saturating_sub(2));
                let available_rows = original.rows.saturating_sub(gap);
                if available_rows < 2 {
                    return None;
                }
                let top_rows = available_rows / 2;
                let bottom_rows = available_rows.saturating_sub(top_rows);
                if top_rows == 0 || bottom_rows == 0 {
                    return None;
                }
                let active_rect =
                    VirtualPaneRect::new(original.col, original.row, original.cols, top_rows);
                let new_rect = VirtualPaneRect::new(
                    original.col,
                    original
                        .row
                        .saturating_add(top_rows as i32)
                        .saturating_add(gap as i32),
                    original.cols,
                    bottom_rows,
                );
                Some((active_id, active_rect, new_rect, true))
            }
        }
    }

    pub fn add_existing_pane_right_of_active_floating(&mut self, pane_id: PaneId) {
        let (col, row) = self
            .active_pane
            .and_then(|active_id| self.pane(active_id))
            .map(|pane| {
                (
                    pane.virtual_rect
                        .right()
                        .saturating_add(self.gap_cols as i32),
                    pane.virtual_rect.row,
                )
            })
            .unwrap_or((0, 0));
        self.add_existing_pane_at(pane_id, col, row);
        self.active_pane = Some(pane_id);
        self.scroll_pane_into_view(pane_id);
    }

    pub fn add_pane_below_active(&mut self) -> PaneId {
        let (col, row) = self
            .active_pane
            .and_then(|pane_id| self.pane(pane_id))
            .map(|pane| {
                (
                    pane.virtual_rect.col,
                    pane.virtual_rect
                        .bottom()
                        .saturating_add(self.gap_rows as i32),
                )
            })
            .unwrap_or((0, 0));
        let pane_id = self.add_pane_at(col, row);
        self.active_pane = Some(pane_id);
        self.scroll_pane_into_view(pane_id);
        pane_id
    }

    pub fn add_existing_pane_below_active_floating(&mut self, pane_id: PaneId) {
        let (col, row) = self
            .active_pane
            .and_then(|active_id| self.pane(active_id))
            .map(|pane| {
                (
                    pane.virtual_rect.col,
                    pane.virtual_rect
                        .bottom()
                        .saturating_add(self.gap_rows as i32),
                )
            })
            .unwrap_or((0, 0));
        self.add_existing_pane_at(pane_id, col, row);
        self.active_pane = Some(pane_id);
        self.scroll_pane_into_view(pane_id);
    }

    pub fn remove_pane_with_changes(&mut self, pane_id: PaneId) -> ScrollingPaneRemoval {
        let Some(index) = self.panes.iter().position(|pane| pane.id == pane_id) else {
            return ScrollingPaneRemoval {
                removed: false,
                grid_changes: Vec::new(),
            };
        };
        let removed_rect = self.panes[index].virtual_rect;
        self.panes.remove(index);
        if self.zoomed_pane == Some(pane_id) {
            self.zoomed_pane = None;
        }
        if self.active_pane == Some(pane_id) {
            self.active_pane = self
                .panes
                .get(index)
                .or_else(|| index.checked_sub(1).and_then(|idx| self.panes.get(idx)))
                .map(|pane| pane.id);
        }
        let mut grid_changes = Vec::new();
        if self.panes.len() == 1 {
            if let Some(pane) = self.panes.first_mut() {
                let full_rect = VirtualPaneRect::new(0, 0, self.viewport_cols, self.viewport_rows);
                let full_grid = GridSize::new(self.viewport_cols, self.viewport_rows);
                if pane.virtual_rect != full_rect || pane.grid_size != full_grid {
                    pane.virtual_rect = full_rect;
                    pane.grid_size = full_grid;
                    grid_changes.push((pane.id, full_grid));
                }
                self.active_pane = Some(pane.id);
            }
        } else if !self.panes.is_empty() {
            self.compact_after_removal(removed_rect);
            self.normalize_geometry_gravity();
        }
        let (max_x, max_y) = self.max_scroll();
        self.scroll_x_cols = self.scroll_x_cols.clamp(0, max_x);
        self.scroll_y_rows = self.scroll_y_rows.clamp(0, max_y);
        self.scroll_animation = ScrollAnimation::Idle;
        self.invalidate_layout();
        ScrollingPaneRemoval {
            removed: true,
            grid_changes,
        }
    }

    pub fn remove_pane(&mut self, pane_id: PaneId) -> bool {
        self.remove_pane_with_changes(pane_id).removed
    }

    fn compact_after_removal(&mut self, removed: VirtualPaneRect) {
        let horizontal_plan = self.compaction_plan(removed, SplitAxis::Vertical);
        let vertical_plan = self.compaction_plan(removed, SplitAxis::Horizontal);

        let selected = match (horizontal_plan, vertical_plan) {
            (Some(horizontal), Some(vertical)) => {
                let horizontal_adjacent =
                    self.adjacent_count_for_compaction(removed, SplitAxis::Vertical);
                let vertical_adjacent =
                    self.adjacent_count_for_compaction(removed, SplitAxis::Horizontal);
                if horizontal_adjacent > vertical_adjacent {
                    Some(horizontal)
                } else if vertical_adjacent > horizontal_adjacent {
                    Some(vertical)
                } else {
                    None
                }
            }
            (Some(horizontal), None) => Some(horizontal),
            (None, Some(vertical)) => Some(vertical),
            (None, None) => None,
        };

        let Some(plan) = selected else {
            return;
        };

        for (pane_id, rect) in plan {
            if let Some(index) = self.pane_index(pane_id) {
                self.panes[index].virtual_rect = rect;
            }
        }
    }

    fn compaction_plan(
        &self,
        removed: VirtualPaneRect,
        axis: SplitAxis,
    ) -> Option<Vec<(PaneId, VirtualPaneRect)>> {
        let delta = match axis {
            SplitAxis::Vertical => removed.cols.saturating_add(self.gap_cols) as i32,
            SplitAxis::Horizontal => removed.rows.saturating_add(self.gap_rows) as i32,
        };
        if delta <= 0 {
            return None;
        }

        let mut plan = Vec::new();
        for pane in &self.panes {
            let rect = pane.virtual_rect;
            let should_move = match axis {
                SplitAxis::Vertical => {
                    rect.col >= removed.right().saturating_add(self.gap_cols as i32)
                        && ranges_overlap(rect.row, rect.bottom(), removed.row, removed.bottom())
                }
                SplitAxis::Horizontal => {
                    rect.row >= removed.bottom().saturating_add(self.gap_rows as i32)
                        && ranges_overlap(rect.col, rect.right(), removed.col, removed.right())
                }
            };
            if !should_move {
                continue;
            }

            let moved = match axis {
                SplitAxis::Vertical => VirtualPaneRect::new(
                    rect.col.saturating_sub(delta),
                    rect.row,
                    rect.cols,
                    rect.rows,
                ),
                SplitAxis::Horizontal => VirtualPaneRect::new(
                    rect.col,
                    rect.row.saturating_sub(delta),
                    rect.cols,
                    rect.rows,
                ),
            };
            plan.push((pane.id, moved));
        }

        if plan.is_empty() {
            return None;
        }

        self.compaction_preserves_non_overlap(&plan).then_some(plan)
    }

    fn adjacent_count_for_compaction(&self, removed: VirtualPaneRect, axis: SplitAxis) -> usize {
        self.panes
            .iter()
            .filter(|pane| {
                let rect = pane.virtual_rect;
                match axis {
                    SplitAxis::Vertical => {
                        rect.col == removed.right().saturating_add(self.gap_cols as i32)
                            && ranges_overlap(
                                rect.row,
                                rect.bottom(),
                                removed.row,
                                removed.bottom(),
                            )
                    }
                    SplitAxis::Horizontal => {
                        rect.row == removed.bottom().saturating_add(self.gap_rows as i32)
                            && ranges_overlap(rect.col, rect.right(), removed.col, removed.right())
                    }
                }
            })
            .count()
    }

    fn compaction_preserves_non_overlap(&self, plan: &[(PaneId, VirtualPaneRect)]) -> bool {
        for i in 0..self.panes.len() {
            let a = planned_rect_for(self.panes[i].id, self.panes[i].virtual_rect, plan);
            for b_pane in self.panes.iter().skip(i + 1) {
                let b = planned_rect_for(b_pane.id, b_pane.virtual_rect, plan);
                if rects_overlap(a, b) {
                    return false;
                }
            }
        }
        true
    }

    pub fn focus_pane(&mut self, pane_id: PaneId) -> bool {
        if self.is_zoomed() && self.zoomed_pane != Some(pane_id) {
            return false;
        }
        if self.active_pane == Some(pane_id) || self.pane(pane_id).is_none() {
            return false;
        }
        self.active_pane = Some(pane_id);
        self.scroll_pane_into_view(pane_id);
        true
    }

    pub fn focus_pane_direction(&mut self, dir: Direction) -> bool {
        if self.is_zoomed() {
            return false;
        }
        let Some(active_id) = self.active_pane else {
            return false;
        };
        let Some(active_rect) = self.pane(active_id).map(|pane| pane.virtual_rect) else {
            return false;
        };

        let mut best_pane = None;
        let mut best_score = f32::INFINITY;

        for pane in &self.panes {
            if pane.id == active_id {
                continue;
            }

            let rect = pane.virtual_rect;
            let valid = match dir {
                Direction::Left => rect.right() <= active_rect.col,
                Direction::Right => rect.col >= active_rect.right(),
                Direction::Up => rect.bottom() <= active_rect.row,
                Direction::Down => rect.row >= active_rect.bottom(),
            };

            if !valid {
                continue;
            }

            let (primary_dist, cross_dist) = match dir {
                Direction::Left => (
                    active_rect.col - rect.right(),
                    active_rect.center_row() - rect.center_row(),
                ),
                Direction::Right => (
                    rect.col - active_rect.right(),
                    active_rect.center_row() - rect.center_row(),
                ),
                Direction::Up => (
                    active_rect.row - rect.bottom(),
                    active_rect.center_col() - rect.center_col(),
                ),
                Direction::Down => (
                    rect.row - active_rect.bottom(),
                    active_rect.center_col() - rect.center_col(),
                ),
            };

            let cross_overlap = match dir {
                Direction::Left | Direction::Right => {
                    let overlap_start = active_rect.row.max(rect.row);
                    let overlap_end = active_rect.bottom().min(rect.bottom());
                    overlap_end > overlap_start
                }
                Direction::Up | Direction::Down => {
                    let overlap_start = active_rect.col.max(rect.col);
                    let overlap_end = active_rect.right().min(rect.right());
                    overlap_end > overlap_start
                }
            };

            let cross_penalty = if cross_overlap {
                0.0
            } else {
                cross_dist.abs() * 100.0
            };
            let score = primary_dist as f32 + cross_penalty;

            if score < best_score {
                best_score = score;
                best_pane = Some(pane.id);
            }
        }

        if let Some(pane_id) = best_pane {
            self.active_pane = Some(pane_id);
            self.scroll_pane_into_view(pane_id);
            true
        } else {
            false
        }
    }

    pub fn scroll_by(&mut self, dx_cols: i32, dy_rows: i32) -> bool {
        self.scroll_by_at(dx_cols, dy_rows, Instant::now())
    }

    fn scroll_by_at(&mut self, dx_cols: i32, dy_rows: i32, now: Instant) -> bool {
        if self.is_zoomed() {
            return false;
        }
        self.set_scroll_target_at(
            self.scroll_x_cols.saturating_add(dx_cols),
            self.scroll_y_rows.saturating_add(dy_rows),
            now,
        )
    }

    pub fn visible_panes(&mut self) -> &[VisibleScrollingPane] {
        let key = VisibleCacheKey {
            layout_generation: self.layout_generation,
            scroll_x_cols: self.scroll_x_cols,
            scroll_y_rows: self.scroll_y_rows,
            viewport_cols: self.viewport_cols,
            viewport_rows: self.viewport_rows,
        };
        if self.visible_cache.key != Some(key) {
            self.visible_cache.panes = if let Some(pane_id) = self.valid_zoomed_pane() {
                vec![VisibleScrollingPane {
                    pane_id,
                    virtual_rect: VirtualPaneRect::new(
                        0,
                        0,
                        self.viewport_cols,
                        self.viewport_rows,
                    ),
                    viewport_col: 0,
                    viewport_row: 0,
                    visible_col_start: 0,
                    visible_row_start: 0,
                    visible_cols: self.viewport_cols,
                    visible_rows: self.viewport_rows,
                    grid_size: GridSize::new(self.viewport_cols, self.viewport_rows),
                    overflow: ScrollingOverflowIndicators::NONE,
                }]
            } else {
                compute_visible_panes(
                    &self.panes,
                    self.scroll_x_cols,
                    self.scroll_y_rows,
                    self.viewport_cols,
                    self.viewport_rows,
                )
            };
            self.visible_cache.key = Some(key);
        }
        self.visible_cache.panes()
    }

    pub fn render_visible_panes(&mut self, now: Instant) -> Vec<RenderScrollingPane> {
        if let Some(pane_id) = self.valid_zoomed_pane() {
            self.scroll_animation = ScrollAnimation::Idle;
            return vec![RenderScrollingPane {
                pane_id,
                virtual_rect: VirtualPaneRect::new(0, 0, self.viewport_cols, self.viewport_rows),
                viewport_col: 0.0,
                viewport_row: 0.0,
                visible_col_start: 0,
                visible_row_start: 0,
                visible_cols: self.viewport_cols,
                visible_rows: self.viewport_rows,
                grid_size: GridSize::new(self.viewport_cols, self.viewport_rows),
                overflow: ScrollingOverflowIndicators::NONE,
            }];
        }

        let (visual_x, visual_y, active) = self.visual_scroll_offset(now);
        if !active {
            self.scroll_animation = ScrollAnimation::Idle;
        }
        compute_render_visible_panes(
            &self.panes,
            visual_x,
            visual_y,
            self.scroll_x_cols,
            self.scroll_y_rows,
            self.viewport_cols,
            self.viewport_rows,
        )
    }

    pub fn visible_pane_ids_uncached(&self) -> Vec<PaneId> {
        if let Some(pane_id) = self.valid_zoomed_pane() {
            return vec![pane_id];
        }
        compute_visible_panes(
            &self.panes,
            self.scroll_x_cols,
            self.scroll_y_rows,
            self.viewport_cols,
            self.viewport_rows,
        )
        .into_iter()
        .map(|pane| pane.pane_id)
        .collect()
    }

    pub fn pane_at_cell(&self, viewport_col: usize, viewport_row: usize) -> Option<PaneId> {
        self.point_target(viewport_col, viewport_row)
            .map(|target| target.pane_id)
    }

    pub fn point_target(
        &self,
        viewport_col: usize,
        viewport_row: usize,
    ) -> Option<ScrollingPanePointTarget> {
        if viewport_col >= self.viewport_cols || viewport_row >= self.viewport_rows {
            return None;
        }
        if let Some(pane_id) = self.valid_zoomed_pane() {
            return Some(ScrollingPanePointTarget {
                pane_id,
                viewport_col,
                viewport_row,
                local_col: viewport_col,
                local_row: viewport_row,
            });
        }
        let virtual_col = self.scroll_x_cols.saturating_add(viewport_col as i32);
        let virtual_row = self.scroll_y_rows.saturating_add(viewport_row as i32);
        let pane = self
            .panes
            .iter()
            .rev()
            .find(|pane| pane.virtual_rect.contains_cell(virtual_col, virtual_row))?;
        Some(ScrollingPanePointTarget {
            pane_id: pane.id,
            viewport_col,
            viewport_row,
            local_col: (virtual_col - pane.virtual_rect.col) as usize,
            local_row: (virtual_row - pane.virtual_rect.row) as usize,
        })
    }

    pub fn resize_handle_at_cell(
        &self,
        viewport_col: usize,
        viewport_row: usize,
    ) -> Option<ScrollingResizeHandle> {
        if self.is_zoomed()
            || viewport_col >= self.viewport_cols
            || viewport_row >= self.viewport_rows
        {
            return None;
        }
        let virtual_col = self.scroll_x_cols.saturating_add(viewport_col as i32);
        let virtual_row = self.scroll_y_rows.saturating_add(viewport_row as i32);
        let tolerance = 0_i32;
        let max_col_gap = self.gap_cols as i32 + tolerance;
        let max_row_gap = self.gap_rows as i32 + tolerance;

        for i in 0..self.panes.len() {
            for j in i + 1..self.panes.len() {
                let first = &self.panes[i];
                let second = &self.panes[j];
                let a = first.virtual_rect;
                let b = second.virtual_rect;

                let (left_id, left, right_id, right) = if a.right() <= b.col {
                    (first.id, a, second.id, b)
                } else if b.right() <= a.col {
                    (second.id, b, first.id, a)
                } else {
                    (first.id, a, second.id, b)
                };
                let vertical_gap = right.col - left.right();
                if vertical_gap >= 0 && vertical_gap <= max_col_gap {
                    let row_overlap = left.row.max(right.row)..left.bottom().min(right.bottom());
                    if row_overlap.start < row_overlap.end
                        && virtual_row >= row_overlap.start
                        && virtual_row < row_overlap.end
                        && virtual_col >= left.right()
                        && virtual_col < right.col
                    {
                        return Some(ScrollingResizeHandle {
                            axis: SplitAxis::Vertical,
                            first: left_id,
                            second: right_id,
                            boundary_start: left.right(),
                            boundary_end: right.col,
                        });
                    }
                }

                let (top_id, top, bottom_id, bottom) = if a.bottom() <= b.row {
                    (first.id, a, second.id, b)
                } else if b.bottom() <= a.row {
                    (second.id, b, first.id, a)
                } else {
                    (first.id, a, second.id, b)
                };
                let horizontal_gap = bottom.row - top.bottom();
                if horizontal_gap >= 0 && horizontal_gap <= max_row_gap {
                    let col_overlap = top.col.max(bottom.col)..top.right().min(bottom.right());
                    if col_overlap.start < col_overlap.end
                        && virtual_col >= col_overlap.start
                        && virtual_col < col_overlap.end
                        && virtual_row >= top.bottom()
                        && virtual_row < bottom.row
                    {
                        return Some(ScrollingResizeHandle {
                            axis: SplitAxis::Horizontal,
                            first: top_id,
                            second: bottom_id,
                            boundary_start: top.bottom(),
                            boundary_end: bottom.row,
                        });
                    }
                }
            }
        }

        None
    }

    pub fn start_resize_drag(
        &self,
        handle: ScrollingResizeHandle,
        viewport_col: usize,
        viewport_row: usize,
    ) -> Option<ScrollingResizeDrag> {
        let virtual_col = self.scroll_x_cols.saturating_add(viewport_col as i32);
        let virtual_row = self.scroll_y_rows.saturating_add(viewport_row as i32);
        let panes = self.resize_group_for_handle(handle, virtual_col, virtual_row)?;
        Some(ScrollingResizeDrag {
            handle,
            origin_virtual_col: virtual_col,
            origin_virtual_row: virtual_row,
            panes,
        })
    }

    pub fn resize_drag_to_cell(
        &mut self,
        drag: ScrollingResizeDrag,
        viewport_col: usize,
        viewport_row: usize,
    ) -> Option<Vec<(PaneId, GridSize)>> {
        if drag.panes.is_empty() {
            return None;
        }
        let virtual_col = self.scroll_x_cols.saturating_add(viewport_col as i32);
        let virtual_row = self.scroll_y_rows.saturating_add(viewport_row as i32);
        let raw_delta = match drag.handle.axis {
            SplitAxis::Vertical => virtual_col - drag.origin_virtual_col,
            SplitAxis::Horizontal => virtual_row - drag.origin_virtual_row,
        };
        let delta = self.clamp_resize_delta(&drag, raw_delta)?;
        let mut changed = false;
        let mut changes = Vec::new();

        for affected in &drag.panes {
            let Some(idx) = self.pane_index(affected.pane_id) else {
                continue;
            };
            let initial = affected.initial_rect;
            let new_rect = match (drag.handle.axis, affected.side) {
                (SplitAxis::Vertical, ScrollingResizeSide::Before) => VirtualPaneRect::new(
                    initial.col,
                    initial.row,
                    (initial.cols as i32 + delta).max(1) as usize,
                    initial.rows,
                ),
                (SplitAxis::Vertical, ScrollingResizeSide::After) => VirtualPaneRect::new(
                    initial.col.saturating_add(delta),
                    initial.row,
                    (initial.cols as i32 - delta).max(1) as usize,
                    initial.rows,
                ),
                (SplitAxis::Horizontal, ScrollingResizeSide::Before) => VirtualPaneRect::new(
                    initial.col,
                    initial.row,
                    initial.cols,
                    (initial.rows as i32 + delta).max(1) as usize,
                ),
                (SplitAxis::Horizontal, ScrollingResizeSide::After) => VirtualPaneRect::new(
                    initial.col,
                    initial.row.saturating_add(delta),
                    initial.cols,
                    (initial.rows as i32 - delta).max(1) as usize,
                ),
            };

            if self.panes[idx].virtual_rect != new_rect {
                changed = true;
            }
            let new_grid = GridSize::new(new_rect.cols, new_rect.rows);
            self.panes[idx].virtual_rect = new_rect;
            self.panes[idx].grid_size = new_grid;
            changes.push((affected.pane_id, new_grid));
        }

        if !changed {
            return Some(Vec::new());
        }

        self.normalize_geometry_gravity();
        self.clamp_scroll();
        self.cancel_scroll_animation();
        self.invalidate_layout();
        Some(changes)
    }

    fn normalize_geometry_gravity(&mut self) -> bool {
        if self.panes.len() <= 1 {
            return false;
        }

        let mut moved_any = false;
        let max_passes = self.panes.len().saturating_mul(2).saturating_add(1);
        for _ in 0..max_passes {
            let mut moved_this_pass = false;
            moved_this_pass |= self.compact_left_once();
            moved_this_pass |= self.compact_up_once();
            if !moved_this_pass {
                break;
            }
            moved_any = true;
        }

        if moved_any {
            self.clamp_scroll();
        }
        moved_any
    }

    fn compact_left_once(&mut self) -> bool {
        let mut order: Vec<usize> = (0..self.panes.len()).collect();
        order.sort_by_key(|&idx| {
            let pane = &self.panes[idx];
            (pane.virtual_rect.row, pane.virtual_rect.col, pane.id.get())
        });

        let mut moved = false;
        for idx in order {
            if idx >= self.panes.len() {
                continue;
            }
            let rect = self.panes[idx].virtual_rect;
            let mut target_col = 0;
            for (other_idx, other) in self.panes.iter().enumerate() {
                if other_idx == idx {
                    continue;
                }
                let other_rect = other.virtual_rect;
                if other_rect.right() <= rect.col
                    && ranges_overlap(rect.row, rect.bottom(), other_rect.row, other_rect.bottom())
                {
                    target_col =
                        target_col.max(other_rect.right().saturating_add(self.gap_cols as i32));
                }
            }

            if target_col < rect.col {
                let candidate = VirtualPaneRect::new(target_col, rect.row, rect.cols, rect.rows);
                if self.rect_fits_without_overlap(self.panes[idx].id, candidate) {
                    self.panes[idx].virtual_rect = candidate;
                    moved = true;
                }
            }
        }
        moved
    }

    fn compact_up_once(&mut self) -> bool {
        let mut order: Vec<usize> = (0..self.panes.len()).collect();
        order.sort_by_key(|&idx| {
            let pane = &self.panes[idx];
            (pane.virtual_rect.col, pane.virtual_rect.row, pane.id.get())
        });

        let mut moved = false;
        for idx in order {
            if idx >= self.panes.len() {
                continue;
            }
            let rect = self.panes[idx].virtual_rect;
            let mut target_row = 0;
            for (other_idx, other) in self.panes.iter().enumerate() {
                if other_idx == idx {
                    continue;
                }
                let other_rect = other.virtual_rect;
                if other_rect.bottom() <= rect.row
                    && ranges_overlap(rect.col, rect.right(), other_rect.col, other_rect.right())
                {
                    target_row =
                        target_row.max(other_rect.bottom().saturating_add(self.gap_rows as i32));
                }
            }

            if target_row < rect.row {
                let candidate = VirtualPaneRect::new(rect.col, target_row, rect.cols, rect.rows);
                if self.rect_fits_without_overlap(self.panes[idx].id, candidate) {
                    self.panes[idx].virtual_rect = candidate;
                    moved = true;
                }
            }
        }
        moved
    }

    fn rect_fits_without_overlap(&self, pane_id: PaneId, rect: VirtualPaneRect) -> bool {
        if rect.col < 0 || rect.row < 0 {
            return false;
        }
        self.panes
            .iter()
            .filter(|pane| pane.id != pane_id)
            .all(|pane| {
                !rects_overlap_with_gap(rect, pane.virtual_rect, self.gap_cols, self.gap_rows)
            })
    }

    fn resize_group_for_handle(
        &self,
        handle: ScrollingResizeHandle,
        virtual_col: i32,
        virtual_row: i32,
    ) -> Option<Vec<ScrollingResizePane>> {
        let mut perpendicular_start;
        let mut perpendicular_end;
        match handle.axis {
            SplitAxis::Vertical => {
                let first = self.pane(handle.first)?.virtual_rect;
                let second = self.pane(handle.second)?.virtual_rect;
                perpendicular_start = first.row.max(second.row);
                perpendicular_end = first.bottom().min(second.bottom());
                if virtual_row >= perpendicular_start && virtual_row < perpendicular_end {
                    perpendicular_start = virtual_row;
                    perpendicular_end = virtual_row.saturating_add(1);
                }
            }
            SplitAxis::Horizontal => {
                let first = self.pane(handle.first)?.virtual_rect;
                let second = self.pane(handle.second)?.virtual_rect;
                perpendicular_start = first.col.max(second.col);
                perpendicular_end = first.right().min(second.right());
                if virtual_col >= perpendicular_start && virtual_col < perpendicular_end {
                    perpendicular_start = virtual_col;
                    perpendicular_end = virtual_col.saturating_add(1);
                }
            }
        }

        let mut changed = true;
        while changed {
            changed = false;
            for pane in &self.panes {
                let rect = pane.virtual_rect;
                let touches = match handle.axis {
                    SplitAxis::Vertical => {
                        (rect.right() == handle.boundary_start || rect.col == handle.boundary_end)
                            && ranges_overlap(
                                rect.row,
                                rect.bottom(),
                                perpendicular_start,
                                perpendicular_end,
                            )
                    }
                    SplitAxis::Horizontal => {
                        (rect.bottom() == handle.boundary_start || rect.row == handle.boundary_end)
                            && ranges_overlap(
                                rect.col,
                                rect.right(),
                                perpendicular_start,
                                perpendicular_end,
                            )
                    }
                };
                if !touches {
                    continue;
                }

                let (start, end) = match handle.axis {
                    SplitAxis::Vertical => (rect.row, rect.bottom()),
                    SplitAxis::Horizontal => (rect.col, rect.right()),
                };
                let new_start = perpendicular_start.min(start);
                let new_end = perpendicular_end.max(end);
                if new_start != perpendicular_start || new_end != perpendicular_end {
                    perpendicular_start = new_start;
                    perpendicular_end = new_end;
                    changed = true;
                }
            }
        }

        let mut panes = Vec::new();
        let mut before_count = 0;
        let mut after_count = 0;
        for pane in &self.panes {
            let rect = pane.virtual_rect;
            let side = match handle.axis {
                SplitAxis::Vertical if rect.right() == handle.boundary_start => {
                    Some(ScrollingResizeSide::Before)
                }
                SplitAxis::Vertical if rect.col == handle.boundary_end => {
                    Some(ScrollingResizeSide::After)
                }
                SplitAxis::Horizontal if rect.bottom() == handle.boundary_start => {
                    Some(ScrollingResizeSide::Before)
                }
                SplitAxis::Horizontal if rect.row == handle.boundary_end => {
                    Some(ScrollingResizeSide::After)
                }
                _ => None,
            };
            let Some(side) = side else {
                continue;
            };
            let overlaps = match handle.axis {
                SplitAxis::Vertical => ranges_overlap(
                    rect.row,
                    rect.bottom(),
                    perpendicular_start,
                    perpendicular_end,
                ),
                SplitAxis::Horizontal => ranges_overlap(
                    rect.col,
                    rect.right(),
                    perpendicular_start,
                    perpendicular_end,
                ),
            };
            if !overlaps {
                continue;
            }
            match side {
                ScrollingResizeSide::Before => before_count += 1,
                ScrollingResizeSide::After => after_count += 1,
            }
            panes.push(ScrollingResizePane {
                pane_id: pane.id,
                side,
                initial_rect: rect,
            });
        }

        if before_count == 0 || after_count == 0 {
            return None;
        }
        Some(panes)
    }

    fn clamp_resize_delta(&self, drag: &ScrollingResizeDrag, raw_delta: i32) -> Option<i32> {
        let mut min_delta = i32::MIN / 4;
        let mut max_delta = i32::MAX / 4;

        for affected in &drag.panes {
            let rect = affected.initial_rect;
            match drag.handle.axis {
                SplitAxis::Vertical => {
                    let min_cols = DEFAULT_MIN_PANE_COLS.min(rect.cols).max(1) as i32;
                    match affected.side {
                        ScrollingResizeSide::Before => {
                            min_delta = min_delta.max(min_cols - rect.cols as i32);
                        }
                        ScrollingResizeSide::After => {
                            max_delta = max_delta.min(rect.cols as i32 - min_cols);
                        }
                    }
                }
                SplitAxis::Horizontal => {
                    let min_rows = DEFAULT_MIN_PANE_ROWS.min(rect.rows).max(1) as i32;
                    match affected.side {
                        ScrollingResizeSide::Before => {
                            min_delta = min_delta.max(min_rows - rect.rows as i32);
                        }
                        ScrollingResizeSide::After => {
                            max_delta = max_delta.min(rect.rows as i32 - min_rows);
                        }
                    }
                }
            }
        }
        if min_delta > max_delta {
            return None;
        }
        Some(raw_delta.clamp(min_delta, max_delta))
    }

    fn pane(&self, pane_id: PaneId) -> Option<&ScrollingPane> {
        self.panes.iter().find(|pane| pane.id == pane_id)
    }

    fn pane_index(&self, pane_id: PaneId) -> Option<usize> {
        self.panes.iter().position(|pane| pane.id == pane_id)
    }

    pub fn toggle_zoom_active(&mut self) -> Option<Vec<(PaneId, GridSize)>> {
        let active_id = self.active_pane?;
        if self.zoomed_pane == Some(active_id) {
            self.zoomed_pane = None;
            let rect = self.pane(active_id)?.virtual_rect;
            self.cancel_scroll_animation();
            self.invalidate_layout();
            return Some(vec![(active_id, GridSize::new(rect.cols, rect.rows))]);
        }

        if self.pane(active_id).is_none() {
            return None;
        }
        self.zoomed_pane = Some(active_id);
        self.cancel_scroll_animation();
        self.invalidate_layout();
        Some(vec![(
            active_id,
            GridSize::new(self.viewport_cols, self.viewport_rows),
        )])
    }

    fn valid_zoomed_pane(&self) -> Option<PaneId> {
        self.zoomed_pane
            .filter(|pane_id| self.pane(*pane_id).is_some())
    }

    fn virtual_bounds(&self) -> (usize, usize) {
        let max_col = self
            .panes
            .iter()
            .map(|pane| pane.virtual_rect.right().max(0) as usize)
            .max()
            .unwrap_or(0);
        let max_row = self
            .panes
            .iter()
            .map(|pane| pane.virtual_rect.bottom().max(0) as usize)
            .max()
            .unwrap_or(0);
        (max_col, max_row)
    }

    fn max_scroll(&self) -> (i32, i32) {
        let (virtual_cols, virtual_rows) = self.virtual_bounds();
        (
            virtual_cols.saturating_sub(self.viewport_cols) as i32,
            virtual_rows.saturating_sub(self.viewport_rows) as i32,
        )
    }

    fn clamp_scroll(&mut self) {
        let (max_x, max_y) = self.max_scroll();
        self.scroll_x_cols = self.scroll_x_cols.clamp(0, max_x);
        self.scroll_y_rows = self.scroll_y_rows.clamp(0, max_y);
    }

    fn set_scroll_target_at(&mut self, target_x: i32, target_y: i32, now: Instant) -> bool {
        let old = self.scroll_offset();
        let (start_x, start_y, _) = self.visual_scroll_offset(now);
        self.scroll_x_cols = target_x;
        self.scroll_y_rows = target_y;
        self.clamp_scroll();
        let new_target = self.scroll_offset();
        if old == new_target {
            return false;
        }

        if (start_x - new_target.0 as f32).abs() < f32::EPSILON
            && (start_y - new_target.1 as f32).abs() < f32::EPSILON
        {
            self.scroll_animation = ScrollAnimation::Idle;
        } else {
            self.scroll_animation = ScrollAnimation::Active {
                start_x,
                start_y,
                target_x: new_target.0,
                target_y: new_target.1,
                started_at: now,
                duration: DEFAULT_SCROLL_ANIMATION_DURATION,
            };
        }
        self.invalidate_visible_cache();
        true
    }

    fn scroll_pane_into_view(&mut self, pane_id: PaneId) {
        self.scroll_pane_into_view_at(pane_id, Instant::now());
    }

    fn scroll_pane_into_view_at(&mut self, pane_id: PaneId, now: Instant) {
        let Some(rect) = self.pane(pane_id).map(|pane| pane.virtual_rect) else {
            return;
        };

        let mut target_x = self.scroll_x_cols;
        let mut target_y = self.scroll_y_rows;
        if rect.col < self.scroll_x_cols {
            target_x = rect.col;
        } else if rect.right() > self.scroll_x_cols + self.viewport_cols as i32 {
            target_x = rect.right() - self.viewport_cols as i32;
        }

        if rect.row < self.scroll_y_rows {
            target_y = rect.row;
        } else if rect.bottom() > self.scroll_y_rows + self.viewport_rows as i32 {
            target_y = rect.bottom() - self.viewport_rows as i32;
        }

        self.set_scroll_target_at(target_x, target_y, now);
    }

    fn invalidate_layout(&mut self) {
        self.layout_generation = self.layout_generation.wrapping_add(1);
        self.invalidate_visible_cache();
    }

    fn invalidate_visible_cache(&mut self) {
        self.visible_cache.key = None;
    }
}

fn compute_visible_panes(
    panes: &[ScrollingPane],
    scroll_x_cols: i32,
    scroll_y_rows: i32,
    viewport_cols: usize,
    viewport_rows: usize,
) -> Vec<VisibleScrollingPane> {
    let viewport_right = viewport_cols as i32;
    let viewport_bottom = viewport_rows as i32;
    let mut visible = Vec::new();

    for pane in panes {
        let viewport_col = pane.virtual_rect.col - scroll_x_cols;
        let viewport_row = pane.virtual_rect.row - scroll_y_rows;
        let pane_right = viewport_col + pane.virtual_rect.cols as i32;
        let pane_bottom = viewport_row + pane.virtual_rect.rows as i32;

        let visible_left = viewport_col.max(0);
        let visible_top = viewport_row.max(0);
        let visible_right = pane_right.min(viewport_right);
        let visible_bottom = pane_bottom.min(viewport_bottom);

        if visible_right <= visible_left || visible_bottom <= visible_top {
            continue;
        }

        let mut overflow = pane_local_overflow(
            pane,
            panes,
            scroll_x_cols,
            scroll_y_rows,
            viewport_cols,
            viewport_rows,
        );
        overflow.above &= visible_top == 0;
        overflow.below &= visible_bottom == viewport_bottom;
        overflow.left &= visible_left == 0;
        overflow.right &= visible_right == viewport_right;

        visible.push(VisibleScrollingPane {
            pane_id: pane.id,
            virtual_rect: pane.virtual_rect,
            viewport_col,
            viewport_row,
            visible_col_start: (visible_left - viewport_col) as usize,
            visible_row_start: (visible_top - viewport_row) as usize,
            visible_cols: (visible_right - visible_left) as usize,
            visible_rows: (visible_bottom - visible_top) as usize,
            grid_size: pane.grid_size,
            overflow,
        });
    }

    visible
}

fn compute_render_visible_panes(
    panes: &[ScrollingPane],
    visual_scroll_x_cols: f32,
    visual_scroll_y_rows: f32,
    logical_scroll_x_cols: i32,
    logical_scroll_y_rows: i32,
    viewport_cols: usize,
    viewport_rows: usize,
) -> Vec<RenderScrollingPane> {
    let viewport_right = viewport_cols as f32;
    let viewport_bottom = viewport_rows as f32;
    let mut visible = Vec::new();

    for pane in panes {
        let viewport_col = pane.virtual_rect.col as f32 - visual_scroll_x_cols;
        let viewport_row = pane.virtual_rect.row as f32 - visual_scroll_y_rows;
        let pane_right = viewport_col + pane.virtual_rect.cols as f32;
        let pane_bottom = viewport_row + pane.virtual_rect.rows as f32;

        let visible_left = viewport_col.max(0.0);
        let visible_top = viewport_row.max(0.0);
        let visible_right = pane_right.min(viewport_right);
        let visible_bottom = pane_bottom.min(viewport_bottom);

        if visible_right <= visible_left || visible_bottom <= visible_top {
            continue;
        }

        let col_start =
            ((visible_left - viewport_col).max(0.0).floor() as usize).min(pane.grid_size.cols);
        let row_start =
            ((visible_top - viewport_row).max(0.0).floor() as usize).min(pane.grid_size.rows);
        let col_end = ((visible_right - viewport_col).ceil() as usize).min(pane.grid_size.cols);
        let row_end = ((visible_bottom - viewport_row).ceil() as usize).min(pane.grid_size.rows);
        if col_end <= col_start || row_end <= row_start {
            continue;
        }

        let mut overflow = pane_local_overflow(
            pane,
            panes,
            logical_scroll_x_cols,
            logical_scroll_y_rows,
            viewport_cols,
            viewport_rows,
        );
        overflow.above &= visible_top <= f32::EPSILON;
        overflow.below &= (viewport_bottom - visible_bottom).abs() <= f32::EPSILON;
        overflow.left &= visible_left <= f32::EPSILON;
        overflow.right &= (viewport_right - visible_right).abs() <= f32::EPSILON;

        visible.push(RenderScrollingPane {
            pane_id: pane.id,
            virtual_rect: pane.virtual_rect,
            viewport_col,
            viewport_row,
            visible_col_start: col_start,
            visible_row_start: row_start,
            visible_cols: col_end - col_start,
            visible_rows: row_end - row_start,
            grid_size: pane.grid_size,
            overflow,
        });
    }

    visible
}

fn ease_out_cubic(t: f32) -> f32 {
    let inv = 1.0 - t;
    1.0 - inv * inv * inv
}

fn pane_local_overflow(
    visible_pane: &ScrollingPane,
    panes: &[ScrollingPane],
    scroll_x_cols: i32,
    scroll_y_rows: i32,
    viewport_cols: usize,
    viewport_rows: usize,
) -> ScrollingOverflowIndicators {
    let viewport_left = scroll_x_cols;
    let viewport_top = scroll_y_rows;
    let viewport_right = scroll_x_cols.saturating_add(viewport_cols as i32);
    let viewport_bottom = scroll_y_rows.saturating_add(viewport_rows as i32);
    let current = visible_pane.virtual_rect;
    let mut nearest_left: Option<(i32, bool)> = None;
    let mut nearest_right: Option<(i32, bool)> = None;
    let mut nearest_above: Option<(i32, bool)> = None;
    let mut nearest_below: Option<(i32, bool)> = None;

    for pane in panes {
        if pane.id == visible_pane.id {
            continue;
        }
        let rect = pane.virtual_rect;
        let fully_visible = rect.col >= viewport_left
            && rect.row >= viewport_top
            && rect.right() <= viewport_right
            && rect.bottom() <= viewport_bottom;
        if fully_visible {
            continue;
        }

        let row_overlap = ranges_overlap(rect.row, rect.bottom(), current.row, current.bottom());
        let col_overlap = ranges_overlap(rect.col, rect.right(), current.col, current.right());

        if row_overlap && rect.right() <= current.col {
            let dist = current.col - rect.right();
            if nearest_left.map(|(best, _)| dist < best).unwrap_or(true) {
                nearest_left = Some((dist, !fully_visible));
            }
        }
        if row_overlap && rect.col >= current.right() {
            let dist = rect.col - current.right();
            if nearest_right.map(|(best, _)| dist < best).unwrap_or(true) {
                nearest_right = Some((dist, !fully_visible));
            }
        }
        if col_overlap && rect.bottom() <= current.row {
            let dist = current.row - rect.bottom();
            if nearest_above.map(|(best, _)| dist < best).unwrap_or(true) {
                nearest_above = Some((dist, !fully_visible));
            }
        }
        if col_overlap && rect.row >= current.bottom() {
            let dist = rect.row - current.bottom();
            if nearest_below.map(|(best, _)| dist < best).unwrap_or(true) {
                nearest_below = Some((dist, !fully_visible));
            }
        }
    }

    ScrollingOverflowIndicators {
        above: nearest_above.map(|(_, hidden)| hidden).unwrap_or(false),
        below: nearest_below.map(|(_, hidden)| hidden).unwrap_or(false),
        left: nearest_left.map(|(_, hidden)| hidden).unwrap_or(false),
        right: nearest_right.map(|(_, hidden)| hidden).unwrap_or(false),
    }
}

fn ranges_overlap(first_start: i32, first_end: i32, second_start: i32, second_end: i32) -> bool {
    first_start < second_end && second_start < first_end
}

fn planned_rect_for(
    pane_id: PaneId,
    original: VirtualPaneRect,
    plan: &[(PaneId, VirtualPaneRect)],
) -> VirtualPaneRect {
    plan.iter()
        .find_map(|(planned_id, rect)| (*planned_id == pane_id).then_some(*rect))
        .unwrap_or(original)
}

fn rects_overlap(a: VirtualPaneRect, b: VirtualPaneRect) -> bool {
    ranges_overlap(a.col, a.right(), b.col, b.right())
        && ranges_overlap(a.row, a.bottom(), b.row, b.bottom())
}

fn rects_overlap_with_gap(
    a: VirtualPaneRect,
    b: VirtualPaneRect,
    gap_cols: usize,
    gap_rows: usize,
) -> bool {
    ranges_overlap(
        a.col,
        a.right(),
        b.col.saturating_sub(gap_cols as i32),
        b.right().saturating_add(gap_cols as i32),
    ) && ranges_overlap(
        a.row,
        a.bottom(),
        b.row.saturating_sub(gap_rows as i32),
        b.bottom().saturating_add(gap_rows as i32),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> ScrollingPaneManager {
        ScrollingPaneManager::new(80, 24, 40, 12).with_gap(2, 1)
    }

    #[test]
    fn visible_pane_calculation_returns_intersecting_panes() {
        let mut manager = manager();
        let first = manager.add_pane_at(0, 0);
        let second = manager.add_pane_at(42, 0);
        let third = manager.add_pane_at(100, 0);

        let visible: Vec<_> = manager
            .visible_panes()
            .iter()
            .map(|pane| pane.pane_id)
            .collect();

        assert_eq!(visible, vec![first, second]);
        assert!(!visible.contains(&third));
    }

    #[test]
    fn scroll_clamps_on_x_and_y_axes() {
        let mut manager = manager();
        manager.add_pane_at(0, 0);
        manager.add_pane_at(100, 50);

        assert!(manager.scroll_by(500, 500));
        assert_eq!(manager.scroll_offset(), (60, 38));
        assert!(manager.scroll_by(-500, -500));
        assert_eq!(manager.scroll_offset(), (0, 0));
    }

    #[test]
    fn scroll_animation_updates_logical_offset_immediately() {
        let mut manager = manager();
        manager.add_pane_at(0, 0);
        manager.add_pane_at(100, 50);
        let now = Instant::now();

        assert!(manager.scroll_by_at(500, 500, now));

        assert_eq!(manager.scroll_offset(), (60, 38));
        let (visual_x, visual_y, active) = manager.visual_scroll_offset(now);
        assert!(active);
        assert_eq!(visual_x, 0.0);
        assert_eq!(visual_y, 0.0);
    }

    #[test]
    fn scroll_animation_interpolates_and_finishes_at_target() {
        let mut manager = manager();
        manager.add_pane_at(0, 0);
        manager.add_pane_at(100, 50);
        let now = Instant::now();

        assert!(manager.scroll_by_at(500, 500, now));
        let half = now + DEFAULT_SCROLL_ANIMATION_DURATION / 2;
        let (mid_x, mid_y, active_mid) = manager.visual_scroll_offset(half);
        assert!(active_mid);
        assert!(mid_x > 0.0 && mid_x < 60.0);
        assert!(mid_y > 0.0 && mid_y < 38.0);

        let done = now + DEFAULT_SCROLL_ANIMATION_DURATION;
        let (end_x, end_y, active_end) = manager.visual_scroll_offset(done);
        assert!(!active_end);
        assert_eq!(end_x, 60.0);
        assert_eq!(end_y, 38.0);
        assert!(!manager.has_active_scroll_animation(done));
    }

    #[test]
    fn interrupted_scroll_animation_starts_from_current_visual_offset() {
        let mut manager = manager();
        manager.add_pane_at(0, 0);
        manager.add_pane_at(100, 0);
        let now = Instant::now();

        assert!(manager.scroll_by_at(60, 0, now));
        let interrupt_at = now + DEFAULT_SCROLL_ANIMATION_DURATION / 2;
        let (current_x, _, active) = manager.visual_scroll_offset(interrupt_at);
        assert!(active);
        assert!(manager.scroll_by_at(-30, 0, interrupt_at));

        let (restart_x, restart_y, still_active) = manager.visual_scroll_offset(interrupt_at);
        assert!(still_active);
        assert_eq!(restart_x, current_x);
        assert_eq!(restart_y, 0.0);
        assert_eq!(manager.scroll_offset(), (30, 0));
    }

    #[test]
    fn visual_render_offset_is_separate_from_logical_hit_testing() {
        let mut manager = manager();
        let first = manager.add_pane_at(0, 0);
        let second = manager.add_pane_at(70, 0);
        manager.add_pane_at(120, 0);
        let now = Instant::now();

        assert!(manager.scroll_by_at(20, 0, now));
        let target = manager.point_target(55, 3).unwrap();
        assert_eq!(target.pane_id, second);
        assert_eq!(target.local_col, 5);

        let render_visible = manager.render_visible_panes(now);
        let rendered_first = render_visible
            .iter()
            .find(|pane| pane.pane_id == first)
            .unwrap();
        let rendered_second = render_visible
            .iter()
            .find(|pane| pane.pane_id == second)
            .unwrap();
        assert_eq!(rendered_first.viewport_col, 0.0);
        assert_eq!(rendered_second.viewport_col, 70.0);
    }

    #[test]
    fn render_visible_panes_use_finished_visual_target() {
        let mut manager = manager();
        manager.add_pane_at(0, 0);
        let second = manager.add_pane_at(70, 0);
        manager.add_pane_at(120, 0);
        let now = Instant::now();

        assert!(manager.scroll_by_at(20, 0, now));
        let render_visible = manager.render_visible_panes(now + DEFAULT_SCROLL_ANIMATION_DURATION);
        let rendered_second = render_visible
            .iter()
            .find(|pane| pane.pane_id == second)
            .unwrap();
        assert_eq!(rendered_second.viewport_col, 50.0);
        assert!(!manager.has_active_scroll_animation(
            now + DEFAULT_SCROLL_ANIMATION_DURATION + Duration::from_millis(1)
        ));
    }

    #[test]
    fn viewport_resize_cancels_scroll_animation() {
        let mut manager = manager();
        manager.add_pane_at(0, 0);
        manager.add_pane_at(100, 50);
        let now = Instant::now();

        assert!(manager.scroll_by_at(500, 500, now));
        assert!(manager.has_active_scroll_animation(now));
        manager.set_viewport_size(70, 20);

        assert!(!manager.has_active_scroll_animation(now));
        let (visual_x, visual_y, active) = manager.visual_scroll_offset(now);
        assert!(!active);
        assert_eq!(
            (visual_x, visual_y),
            (manager.scroll_x_cols as f32, manager.scroll_y_rows as f32)
        );
    }

    #[test]
    fn pane_removal_cancels_scroll_animation() {
        let mut manager = manager();
        manager.add_pane_at(0, 0);
        let second = manager.add_pane_at(100, 0);
        let now = Instant::now();

        assert!(manager.scroll_by_at(40, 0, now));
        assert!(manager.has_active_scroll_animation(now));
        assert!(manager.remove_pane(second));

        assert!(!manager.has_active_scroll_animation(now));
    }

    #[test]
    fn partial_visibility_reports_visible_span_inside_pane() {
        let mut manager = manager();
        let pane = manager.add_pane_at(70, 20);

        let visible = manager.visible_panes();

        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].pane_id, pane);
        assert_eq!(visible[0].viewport_col, 70);
        assert_eq!(visible[0].viewport_row, 20);
        assert_eq!(visible[0].visible_cols, 10);
        assert_eq!(visible[0].visible_rows, 4);
        assert_eq!(visible[0].visible_col_start, 0);
        assert_eq!(visible[0].visible_row_start, 0);
    }

    #[test]
    fn hit_testing_accounts_for_scroll_offset() {
        let mut manager = manager();
        let first = manager.add_pane_at(0, 0);
        let second = manager.add_pane_at(42, 0);
        manager.add_pane_at(100, 0);
        assert!(manager.scroll_by(20, 0));

        let target = manager.point_target(25, 3).unwrap();

        assert_eq!(first, PaneId::new(1));
        assert_eq!(target.pane_id, second);
        assert_eq!(target.local_col, 3);
        assert_eq!(target.local_row, 3);
    }

    #[test]
    fn gravity_normalization_removes_diagonal_empty_space_without_resizing() {
        let mut manager = ScrollingPaneManager::new(80, 40, 20, 10).with_gap(1, 1);
        let first = manager.add_pane_at(0, 0);
        let second = manager.add_pane_at(30, 20);
        let first_size = manager.pane(first).unwrap().grid_size;
        let second_size = manager.pane(second).unwrap().grid_size;

        assert!(manager.normalize_geometry_gravity());

        assert_eq!(manager.pane(first).unwrap().grid_size, first_size);
        assert_eq!(manager.pane(second).unwrap().grid_size, second_size);
        assert_eq!(
            manager.pane(first).unwrap().virtual_rect,
            VirtualPaneRect::new(0, 0, 20, 10)
        );
        assert_eq!(
            manager.pane(second).unwrap().virtual_rect,
            VirtualPaneRect::new(0, 11, 20, 10)
        );
    }

    #[test]
    fn gravity_normalization_moves_pane_left_until_blocked_by_gap() {
        let mut manager = ScrollingPaneManager::new(80, 40, 20, 10).with_gap(2, 1);
        let first = manager.add_pane_at(0, 0);
        let second = manager.add_pane_at(30, 0);

        assert!(manager.normalize_geometry_gravity());

        assert_eq!(
            manager.pane(first).unwrap().virtual_rect,
            VirtualPaneRect::new(0, 0, 20, 10)
        );
        assert_eq!(
            manager.pane(second).unwrap().virtual_rect,
            VirtualPaneRect::new(22, 0, 20, 10)
        );
    }

    #[test]
    fn gravity_normalization_moves_pane_up_until_blocked_by_gap() {
        let mut manager = ScrollingPaneManager::new(80, 40, 20, 10).with_gap(1, 2);
        let first = manager.add_pane_at(0, 0);
        let second = manager.add_pane_at(0, 20);

        assert!(manager.normalize_geometry_gravity());

        assert_eq!(
            manager.pane(first).unwrap().virtual_rect,
            VirtualPaneRect::new(0, 0, 20, 10)
        );
        assert_eq!(
            manager.pane(second).unwrap().virtual_rect,
            VirtualPaneRect::new(0, 12, 20, 10)
        );
    }

    #[test]
    fn gravity_normalization_preserves_gap_and_prevents_overlap() {
        let mut manager = ScrollingPaneManager::new(100, 50, 20, 10).with_gap(2, 2);
        manager.add_pane_at(0, 0);
        manager.add_pane_at(30, 0);
        manager.add_pane_at(30, 20);

        assert!(manager.normalize_geometry_gravity());

        for i in 0..manager.panes.len() {
            for pane in manager.panes.iter().skip(i + 1) {
                assert!(
                    !rects_overlap_with_gap(
                        manager.panes[i].virtual_rect,
                        pane.virtual_rect,
                        manager.gap_cols,
                        manager.gap_rows,
                    ),
                    "panes {:?} and {:?} violate protected gap",
                    manager.panes[i].id,
                    pane.id
                );
            }
        }
    }

    #[test]
    fn overflow_indicators_mark_hidden_horizontal_panes() {
        let mut manager = manager();
        manager.add_pane_at(0, 0);
        let second = manager.add_pane_at(42, 0);
        manager.add_pane_at(84, 0);

        let visible = manager.visible_panes();
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].overflow, ScrollingOverflowIndicators::NONE);
        assert_eq!(visible[1].pane_id, second);
        assert!(visible[1].overflow.right);
        assert!(!visible[1].overflow.left);

        assert!(manager.scroll_by(44, 0));
        let visible = manager.visible_panes();
        assert_eq!(visible.len(), 2);
        assert!(visible[0].overflow.left);
        assert!(!visible[0].overflow.right);
        assert_eq!(visible[1].overflow, ScrollingOverflowIndicators::NONE);
    }

    #[test]
    fn overflow_indicators_mark_hidden_vertical_panes() {
        let mut manager = manager();
        manager.add_pane_at(0, 0);
        let second = manager.add_pane_at(0, 13);
        manager.add_pane_at(0, 26);

        let visible = manager.visible_panes();
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].overflow, ScrollingOverflowIndicators::NONE);
        assert_eq!(visible[1].pane_id, second);
        assert!(visible[1].overflow.below);
        assert!(!visible[1].overflow.above);

        assert!(manager.scroll_by(0, 14));
        let visible = manager.visible_panes();
        assert_eq!(visible.len(), 2);
        assert!(visible[0].overflow.above);
        assert!(!visible[0].overflow.below);
        assert_eq!(visible[1].overflow, ScrollingOverflowIndicators::NONE);
    }

    #[test]
    fn add_pane_right_and_below_updates_active_pane() {
        let mut manager = manager();
        let first = manager.add_pane_at(0, 0);
        let right = manager.add_pane_right_of_active();
        let below = manager.add_pane_below_active();

        assert_eq!(manager.pane_count(), 3);
        assert_eq!(manager.active_pane(), Some(below));
        assert_eq!(manager.pane(first).unwrap().virtual_rect.col, 0);
        assert_eq!(manager.pane(right).unwrap().virtual_rect.col, 42);
        assert_eq!(manager.pane(below).unwrap().virtual_rect.row, 13);
    }

    #[test]
    fn split_active_vertical_halves_active_pane() {
        let mut manager = ScrollingPaneManager::new(80, 24, 80, 24);
        let first = manager.add_pane_at(0, 0);

        let changes = manager
            .split_active_with_existing(SplitAxis::Vertical, PaneId::new(2))
            .expect("split should succeed");

        assert_eq!(
            changes,
            vec![
                (first, GridSize::new(39, 24)),
                (PaneId::new(2), GridSize::new(40, 24))
            ]
        );
        assert_eq!(
            manager.pane(first).unwrap().virtual_rect,
            VirtualPaneRect::new(0, 0, 39, 24)
        );
        assert_eq!(
            manager.pane(PaneId::new(2)).unwrap().virtual_rect,
            VirtualPaneRect::new(40, 0, 40, 24)
        );
        assert_eq!(manager.active_pane(), Some(PaneId::new(2)));
    }

    #[test]
    fn repeated_vertical_split_extends_virtual_space_instead_of_shrinking() {
        let mut manager = ScrollingPaneManager::new(80, 24, 80, 24);
        let first = manager.add_pane_at(0, 0);
        manager
            .split_active_with_existing(SplitAxis::Vertical, PaneId::new(2))
            .expect("first split should succeed");

        let changes = manager
            .split_active_with_existing(SplitAxis::Vertical, PaneId::new(3))
            .expect("second split should extend");

        assert_eq!(changes, vec![(PaneId::new(3), GridSize::new(40, 24))]);
        assert_eq!(
            manager.pane(first).unwrap().virtual_rect,
            VirtualPaneRect::new(0, 0, 39, 24)
        );
        assert_eq!(
            manager.pane(PaneId::new(2)).unwrap().virtual_rect,
            VirtualPaneRect::new(40, 0, 40, 24)
        );
        assert_eq!(
            manager.pane(PaneId::new(3)).unwrap().virtual_rect,
            VirtualPaneRect::new(81, 0, 40, 24)
        );
        assert_eq!(manager.scroll_offset(), (41, 0));
        assert_eq!(
            manager.visible_pane_ids_uncached(),
            vec![PaneId::new(2), PaneId::new(3)]
        );
    }

    #[test]
    fn split_active_horizontal_halves_active_pane() {
        let mut manager = ScrollingPaneManager::new(80, 24, 80, 24);
        let first = manager.add_pane_at(0, 0);

        let changes = manager
            .split_active_with_existing(SplitAxis::Horizontal, PaneId::new(2))
            .expect("split should succeed");

        assert_eq!(
            changes,
            vec![
                (first, GridSize::new(80, 11)),
                (PaneId::new(2), GridSize::new(80, 12))
            ]
        );
        assert_eq!(
            manager.pane(first).unwrap().virtual_rect,
            VirtualPaneRect::new(0, 0, 80, 11)
        );
        assert_eq!(
            manager.pane(PaneId::new(2)).unwrap().virtual_rect,
            VirtualPaneRect::new(0, 12, 80, 12)
        );
        assert_eq!(manager.active_pane(), Some(PaneId::new(2)));
    }

    #[test]
    fn repeated_horizontal_split_extends_virtual_space_instead_of_shrinking() {
        let mut manager = ScrollingPaneManager::new(80, 24, 80, 24);
        let first = manager.add_pane_at(0, 0);
        manager
            .split_active_with_existing(SplitAxis::Horizontal, PaneId::new(2))
            .expect("first split should succeed");

        let changes = manager
            .split_active_with_existing(SplitAxis::Horizontal, PaneId::new(3))
            .expect("second split should extend");

        assert_eq!(changes, vec![(PaneId::new(3), GridSize::new(80, 12))]);
        assert_eq!(
            manager.pane(first).unwrap().virtual_rect,
            VirtualPaneRect::new(0, 0, 80, 11)
        );
        assert_eq!(
            manager.pane(PaneId::new(2)).unwrap().virtual_rect,
            VirtualPaneRect::new(0, 12, 80, 12)
        );
        assert_eq!(
            manager.pane(PaneId::new(3)).unwrap().virtual_rect,
            VirtualPaneRect::new(0, 25, 80, 12)
        );
        assert_eq!(manager.scroll_offset(), (0, 13));
        assert_eq!(
            manager.visible_pane_ids_uncached(),
            vec![PaneId::new(2), PaneId::new(3)]
        );
    }

    #[test]
    fn resize_handle_hit_test_finds_scrolling_split_gap() {
        let mut manager = ScrollingPaneManager::new(80, 24, 80, 24);
        manager.add_pane_at(0, 0);
        manager
            .split_active_with_existing(SplitAxis::Vertical, PaneId::new(2))
            .expect("split should succeed");

        let handle = manager.resize_handle_at_cell(39, 3).unwrap();
        assert_eq!(handle.axis, SplitAxis::Vertical);
        assert_eq!(handle.first, PaneId::new(1));
        assert_eq!(handle.second, PaneId::new(2));
        assert_eq!(manager.resize_handle_at_cell(38, 3), None);
        assert_eq!(manager.resize_handle_at_cell(40, 3), None);
    }

    #[test]
    fn resize_drag_updates_adjacent_scrolling_pane_sizes() {
        let mut manager = ScrollingPaneManager::new(80, 24, 80, 24);
        manager.add_pane_at(0, 0);
        manager
            .split_active_with_existing(SplitAxis::Vertical, PaneId::new(2))
            .expect("split should succeed");
        let handle = manager.resize_handle_at_cell(39, 3).unwrap();

        let drag = manager
            .start_resize_drag(handle, 39, 3)
            .expect("drag should start");
        let changes = manager
            .resize_drag_to_cell(drag, 30, 3)
            .expect("resize should apply");

        assert_eq!(
            changes,
            vec![
                (PaneId::new(1), GridSize::new(30, 24)),
                (PaneId::new(2), GridSize::new(49, 24))
            ]
        );
        assert_eq!(
            manager.pane(PaneId::new(1)).unwrap().virtual_rect,
            VirtualPaneRect::new(0, 0, 30, 24)
        );
        assert_eq!(
            manager.pane(PaneId::new(2)).unwrap().virtual_rect,
            VirtualPaneRect::new(31, 0, 49, 24)
        );
    }

    #[test]
    fn resize_drag_updates_all_panes_sharing_vertical_boundary() {
        let mut manager = ScrollingPaneManager::new(100, 40, 100, 40).with_gap(1, 1);
        manager.add_pane_at(0, 0);
        manager.add_pane_at(0, 0);
        manager.add_pane_at(0, 0);
        manager.panes[0].id = PaneId::new(1);
        manager.panes[0].virtual_rect = VirtualPaneRect::new(0, 0, 49, 40);
        manager.panes[0].grid_size = GridSize::new(49, 40);
        manager.panes[1].id = PaneId::new(2);
        manager.panes[1].virtual_rect = VirtualPaneRect::new(50, 0, 50, 19);
        manager.panes[1].grid_size = GridSize::new(50, 19);
        manager.panes[2].id = PaneId::new(3);
        manager.panes[2].virtual_rect = VirtualPaneRect::new(50, 20, 50, 20);
        manager.panes[2].grid_size = GridSize::new(50, 20);
        manager.invalidate_layout();

        let handle = manager.resize_handle_at_cell(49, 8).unwrap();
        assert_eq!(handle.axis, SplitAxis::Vertical);

        let drag = manager
            .start_resize_drag(handle, 49, 8)
            .expect("drag should include shared boundary panes");
        let changes = manager
            .resize_drag_to_cell(drag, 44, 8)
            .expect("resize should apply");

        assert_eq!(
            changes,
            vec![
                (PaneId::new(1), GridSize::new(44, 40)),
                (PaneId::new(2), GridSize::new(55, 19)),
                (PaneId::new(3), GridSize::new(55, 20)),
            ]
        );
        assert_eq!(
            manager.pane(PaneId::new(1)).unwrap().virtual_rect,
            VirtualPaneRect::new(0, 0, 44, 40)
        );
        assert_eq!(
            manager.pane(PaneId::new(2)).unwrap().virtual_rect,
            VirtualPaneRect::new(45, 0, 55, 19)
        );
        assert_eq!(
            manager.pane(PaneId::new(3)).unwrap().virtual_rect,
            VirtualPaneRect::new(45, 20, 55, 20)
        );
    }

    #[test]
    fn resize_drag_updates_all_panes_sharing_horizontal_boundary() {
        let mut manager = ScrollingPaneManager::new(100, 40, 100, 40).with_gap(1, 1);
        manager.add_pane_at(0, 0);
        manager.add_pane_at(0, 0);
        manager.add_pane_at(0, 0);
        manager.panes[0].id = PaneId::new(1);
        manager.panes[0].virtual_rect = VirtualPaneRect::new(0, 0, 49, 19);
        manager.panes[0].grid_size = GridSize::new(49, 19);
        manager.panes[1].id = PaneId::new(2);
        manager.panes[1].virtual_rect = VirtualPaneRect::new(50, 0, 50, 19);
        manager.panes[1].grid_size = GridSize::new(50, 19);
        manager.panes[2].id = PaneId::new(3);
        manager.panes[2].virtual_rect = VirtualPaneRect::new(0, 20, 100, 20);
        manager.panes[2].grid_size = GridSize::new(100, 20);
        manager.invalidate_layout();

        let handle = manager.resize_handle_at_cell(20, 19).unwrap();
        assert_eq!(handle.axis, SplitAxis::Horizontal);

        let drag = manager
            .start_resize_drag(handle, 20, 19)
            .expect("drag should include shared boundary panes");
        let changes = manager
            .resize_drag_to_cell(drag, 20, 15)
            .expect("resize should apply");

        assert_eq!(
            changes,
            vec![
                (PaneId::new(1), GridSize::new(49, 15)),
                (PaneId::new(2), GridSize::new(50, 15)),
                (PaneId::new(3), GridSize::new(100, 24)),
            ]
        );
        assert_eq!(
            manager.pane(PaneId::new(1)).unwrap().virtual_rect,
            VirtualPaneRect::new(0, 0, 49, 15)
        );
        assert_eq!(
            manager.pane(PaneId::new(2)).unwrap().virtual_rect,
            VirtualPaneRect::new(50, 0, 50, 15)
        );
        assert_eq!(
            manager.pane(PaneId::new(3)).unwrap().virtual_rect,
            VirtualPaneRect::new(0, 16, 100, 24)
        );
    }

    #[test]
    fn resize_handle_hit_test_rejects_distant_panes() {
        let mut manager = ScrollingPaneManager::new(120, 24, 40, 12).with_gap(1, 1);
        manager.add_pane_at(0, 0);
        manager.add_pane_at(80, 0);

        assert_eq!(manager.resize_handle_at_cell(50, 3), None);
    }

    #[test]
    fn removing_to_single_pane_restores_full_viewport() {
        let mut manager = ScrollingPaneManager::new(80, 24, 80, 24);
        manager.add_pane_at(0, 0);
        manager
            .split_active_with_existing(SplitAxis::Vertical, PaneId::new(2))
            .expect("split should succeed");

        let removal = manager.remove_pane_with_changes(PaneId::new(2));

        assert!(removal.removed);
        assert_eq!(
            removal.grid_changes,
            vec![(PaneId::new(1), GridSize::new(80, 24))]
        );
        assert_eq!(
            manager.pane(PaneId::new(1)).unwrap().virtual_rect,
            VirtualPaneRect::new(0, 0, 80, 24)
        );
        assert_eq!(manager.scroll_offset(), (0, 0));
    }

    #[test]
    fn removing_middle_horizontal_pane_compacts_right_neighbor_without_resizing() {
        let mut manager = ScrollingPaneManager::new(80, 24, 80, 24);
        manager.add_pane_at(0, 0);
        manager
            .split_active_with_existing(SplitAxis::Vertical, PaneId::new(2))
            .expect("first split should succeed");
        manager
            .split_active_with_existing(SplitAxis::Vertical, PaneId::new(3))
            .expect("second split should extend");

        let right_before = manager.pane(PaneId::new(3)).unwrap().virtual_rect;
        let removal = manager.remove_pane_with_changes(PaneId::new(2));

        assert!(removal.removed);
        assert!(removal.grid_changes.is_empty());
        assert_eq!(
            manager.pane(PaneId::new(1)).unwrap().virtual_rect,
            VirtualPaneRect::new(0, 0, 39, 24)
        );
        assert_eq!(
            manager.pane(PaneId::new(3)).unwrap().virtual_rect,
            VirtualPaneRect::new(40, 0, right_before.cols, right_before.rows)
        );
    }

    #[test]
    fn removing_middle_vertical_pane_compacts_lower_neighbor_without_resizing() {
        let mut manager = ScrollingPaneManager::new(80, 24, 80, 24);
        manager.add_pane_at(0, 0);
        manager
            .split_active_with_existing(SplitAxis::Horizontal, PaneId::new(2))
            .expect("first split should succeed");
        manager
            .split_active_with_existing(SplitAxis::Horizontal, PaneId::new(3))
            .expect("second split should extend");

        let lower_before = manager.pane(PaneId::new(3)).unwrap().virtual_rect;
        let removal = manager.remove_pane_with_changes(PaneId::new(2));

        assert!(removal.removed);
        assert!(removal.grid_changes.is_empty());
        assert_eq!(
            manager.pane(PaneId::new(1)).unwrap().virtual_rect,
            VirtualPaneRect::new(0, 0, 80, 11)
        );
        assert_eq!(
            manager.pane(PaneId::new(3)).unwrap().virtual_rect,
            VirtualPaneRect::new(0, 12, lower_before.cols, lower_before.rows)
        );
    }

    #[test]
    fn removal_compaction_keeps_mixed_layout_non_overlapping() {
        let mut manager = ScrollingPaneManager::new(120, 50, 120, 50).with_gap(1, 1);
        manager.add_pane_at(0, 0);
        manager.add_pane_at(0, 0);
        manager.add_pane_at(0, 0);
        manager.add_pane_at(0, 0);
        manager.panes[0].id = PaneId::new(1);
        manager.panes[0].virtual_rect = VirtualPaneRect::new(0, 0, 39, 20);
        manager.panes[0].grid_size = GridSize::new(39, 20);
        manager.panes[1].id = PaneId::new(2);
        manager.panes[1].virtual_rect = VirtualPaneRect::new(40, 0, 40, 20);
        manager.panes[1].grid_size = GridSize::new(40, 20);
        manager.panes[2].id = PaneId::new(3);
        manager.panes[2].virtual_rect = VirtualPaneRect::new(81, 0, 39, 20);
        manager.panes[2].grid_size = GridSize::new(39, 20);
        manager.panes[3].id = PaneId::new(4);
        manager.panes[3].virtual_rect = VirtualPaneRect::new(0, 21, 120, 20);
        manager.panes[3].grid_size = GridSize::new(120, 20);
        manager.invalidate_layout();

        assert!(manager.remove_pane(PaneId::new(2)));

        for i in 0..manager.panes.len() {
            for pane in manager.panes.iter().skip(i + 1) {
                assert!(
                    !rects_overlap(manager.panes[i].virtual_rect, pane.virtual_rect),
                    "panes {:?} and {:?} overlap",
                    manager.panes[i].id,
                    pane.id
                );
            }
        }
        assert_eq!(
            manager.pane(PaneId::new(3)).unwrap().virtual_rect,
            VirtualPaneRect::new(40, 0, 39, 20)
        );
        assert_eq!(
            manager.pane(PaneId::new(4)).unwrap().virtual_rect,
            VirtualPaneRect::new(0, 21, 120, 20)
        );
    }

    #[test]
    fn viewport_resize_preserves_multi_pane_virtual_rects() {
        let mut manager = ScrollingPaneManager::new(80, 24, 80, 24).with_gap(1, 1);
        manager.add_pane_at(0, 0);
        manager
            .split_active_with_existing(SplitAxis::Vertical, PaneId::new(2))
            .expect("split should succeed");

        let first_before = manager.pane(PaneId::new(1)).unwrap().virtual_rect;
        let second_before = manager.pane(PaneId::new(2)).unwrap().virtual_rect;
        let changes = manager.set_viewport_size(120, 40);

        assert!(changes.is_empty());
        assert_eq!(
            manager.pane(PaneId::new(1)).unwrap().virtual_rect,
            first_before
        );
        assert_eq!(
            manager.pane(PaneId::new(2)).unwrap().virtual_rect,
            second_before
        );
    }

    #[test]
    fn zoom_active_pane_temporarily_uses_full_viewport() {
        let mut manager = ScrollingPaneManager::new(80, 24, 80, 24);
        manager.add_pane_at(0, 0);
        manager
            .split_active_with_existing(SplitAxis::Vertical, PaneId::new(2))
            .expect("split should succeed");

        let zoom = manager.toggle_zoom_active().expect("zoom should toggle");
        assert_eq!(zoom, vec![(PaneId::new(2), GridSize::new(80, 24))]);
        assert_eq!(manager.visible_pane_ids_uncached(), vec![PaneId::new(2)]);

        let restore = manager.toggle_zoom_active().expect("zoom should restore");
        assert_eq!(restore, vec![(PaneId::new(2), GridSize::new(40, 24))]);
        assert_eq!(
            manager.visible_pane_ids_uncached(),
            vec![PaneId::new(1), PaneId::new(2)]
        );
    }

    #[test]
    fn directional_focus_uses_virtual_pane_positions() {
        let mut manager = manager();
        let first = manager.add_pane_at(0, 0);
        let right = manager.add_pane_at(42, 0);
        let below = manager.add_pane_at(0, 13);

        assert_eq!(manager.active_pane(), Some(first));
        assert!(manager.focus_pane_direction(Direction::Right));
        assert_eq!(manager.active_pane(), Some(right));
        assert!(manager.focus_pane_direction(Direction::Left));
        assert_eq!(manager.active_pane(), Some(first));
        assert!(manager.focus_pane_direction(Direction::Down));
        assert_eq!(manager.active_pane(), Some(below));
        assert!(manager.focus_pane_direction(Direction::Up));
        assert_eq!(manager.active_pane(), Some(first));
    }

    #[test]
    fn directional_focus_scrolls_target_into_view() {
        let mut manager = manager();
        let first = manager.add_pane_at(0, 0);
        let second = manager.add_pane_at(42, 0);
        let third = manager.add_pane_at(84, 0);

        assert_eq!(manager.active_pane(), Some(first));
        assert_eq!(manager.scroll_offset(), (0, 0));
        assert!(manager.focus_pane_direction(Direction::Right));
        assert_eq!(manager.active_pane(), Some(second));
        assert_eq!(manager.scroll_offset(), (2, 0));
        assert!(manager.focus_pane_direction(Direction::Right));
        assert_eq!(manager.active_pane(), Some(third));
        assert_eq!(manager.scroll_offset(), (44, 0));
        assert!(!manager.focus_pane_direction(Direction::Right));
    }

    #[test]
    fn removing_active_pane_falls_back_to_next_then_previous() {
        let mut manager = manager();
        let first = manager.add_pane_at(0, 0);
        let second = manager.add_pane_at(42, 0);
        let third = manager.add_pane_at(84, 0);
        assert!(manager.focus_pane(second));

        assert!(manager.remove_pane(second));
        assert_eq!(manager.active_pane(), Some(third));
        assert!(manager.remove_pane(third));
        assert_eq!(manager.active_pane(), Some(first));
        assert!(manager.remove_pane(first));
        assert_eq!(manager.active_pane(), None);
    }

    #[test]
    fn visible_cache_is_reused_and_invalidated() {
        let mut manager = manager();
        manager.add_pane_at(0, 0);

        let first_generation = manager.layout_generation();
        let first_key = {
            manager.visible_panes();
            manager.visible_cache.key
        };
        manager.visible_panes();
        assert_eq!(manager.visible_cache.key, first_key);

        manager.add_pane_at(42, 0);
        assert_ne!(manager.layout_generation(), first_generation);
        assert_eq!(manager.visible_cache.key, None);
    }
}
