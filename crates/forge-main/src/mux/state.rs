use std::collections::HashMap;

use super::{
    layout::{compute_layout, LayoutError, LayoutParams, LayoutResult},
    pane::{GridSize, Pane, PaneId, PaneRect},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LayoutNode {
    Leaf(PaneId),
    Split {
        axis: SplitAxis,
        ratio: f32,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}

impl LayoutNode {
    pub fn leaf(pane_id: PaneId) -> Self {
        Self::Leaf(pane_id)
    }

    pub fn split(axis: SplitAxis, ratio: f32, first: LayoutNode, second: LayoutNode) -> Self {
        Self::Split {
            axis,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    pub fn contains_pane(&self, pane_id: PaneId) -> bool {
        match self {
            Self::Leaf(id) => *id == pane_id,
            Self::Split { first, second, .. } => {
                first.contains_pane(pane_id) || second.contains_pane(pane_id)
            }
        }
    }

    pub fn replace_leaf(&mut self, pane_id: PaneId, replacement: LayoutNode) -> bool {
        match self {
            Self::Leaf(id) if *id == pane_id => {
                *self = replacement;
                true
            }
            Self::Leaf(_) => false,
            Self::Split { first, second, .. } => {
                if first.replace_leaf(pane_id, replacement.clone()) {
                    true
                } else {
                    second.replace_leaf(pane_id, replacement)
                }
            }
        }
    }

    pub fn set_split_ratio(&mut self, path: &[bool], new_ratio: f32) -> bool {
        if path.is_empty() {
            if let Self::Split { ratio, .. } = self {
                *ratio = new_ratio;
                return true;
            }
            return false;
        }
        if let Self::Split { first, second, .. } = self {
            if path[0] {
                second.set_split_ratio(&path[1..], new_ratio)
            } else {
                first.set_split_ratio(&path[1..], new_ratio)
            }
        } else {
            false
        }
    }

    pub fn first_pane(&self) -> PaneId {
        match self {
            Self::Leaf(id) => *id,
            Self::Split { first, .. } => first.first_pane(),
        }
    }

    fn remove_pane(self, pane_id: PaneId) -> RemoveNodeResult {
        match self {
            Self::Leaf(id) if id == pane_id => RemoveNodeResult::RemovedRoot,
            Self::Leaf(_) => RemoveNodeResult::NotFound(self),
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => match first.remove_pane(pane_id) {
                RemoveNodeResult::RemovedRoot => {
                    let preferred_focus = second.first_pane();
                    RemoveNodeResult::Removed {
                        node: *second,
                        preferred_focus,
                    }
                }
                RemoveNodeResult::Removed {
                    node,
                    preferred_focus,
                } => RemoveNodeResult::Removed {
                    node: Self::split(axis, ratio, node, *second),
                    preferred_focus,
                },
                RemoveNodeResult::NotFound(first) => match second.remove_pane(pane_id) {
                    RemoveNodeResult::RemovedRoot => {
                        let preferred_focus = first.first_pane();
                        RemoveNodeResult::Removed {
                            node: first,
                            preferred_focus,
                        }
                    }
                    RemoveNodeResult::Removed {
                        node,
                        preferred_focus,
                    } => RemoveNodeResult::Removed {
                        node: Self::split(axis, ratio, first, node),
                        preferred_focus,
                    },
                    RemoveNodeResult::NotFound(second) => {
                        RemoveNodeResult::NotFound(Self::split(axis, ratio, first, second))
                    }
                },
            },
        }
    }
}

enum RemoveNodeResult {
    RemovedRoot,
    Removed {
        node: LayoutNode,
        preferred_focus: PaneId,
    },
    NotFound(LayoutNode),
}

pub struct MuxState {
    pub root: LayoutNode,
    pub panes: HashMap<PaneId, Pane>,
    pub active_pane: PaneId,
    pub zoomed_pane: Option<PaneId>,
    pub next_pane_id: u64,
    pub layout_generation: u64,
    pub last_borders: Vec<crate::mux::layout::SplitBorder>,
    pub floating_panes: Vec<PaneId>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanePointTarget {
    pub pane_id: PaneId,
    pub rect: PaneRect,
    pub local_x: f32,
    pub local_y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneLayoutChange {
    pub pane_id: PaneId,
    pub old_grid_size: GridSize,
    pub new_grid_size: GridSize,
    pub old_rect: PaneRect,
    pub new_rect: PaneRect,
}

impl PaneLayoutChange {
    pub fn grid_changed(self) -> bool {
        self.old_grid_size != self.new_grid_size
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RelayoutError {
    Layout(LayoutError),
    MissingPane(PaneId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug)]
pub enum SplitError {
    MissingActivePane(PaneId),

    Layout(LayoutError),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SplitPaneError {
    MissingActivePane(PaneId),
    Zoomed(PaneId),

    Layout(LayoutError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovePaneResult {
    Removed {
        new_active: PaneId,
        removed_active: bool,
    },
    RemovedLastPane,
    MissingPane,
}

impl From<LayoutError> for RelayoutError {
    fn from(error: LayoutError) -> Self {
        Self::Layout(error)
    }
}

impl From<LayoutError> for SplitPaneError {
    fn from(error: LayoutError) -> Self {
        Self::Layout(error)
    }
}

impl MuxState {
    pub fn single_pane(
        pty: forge_pty::Pty,
        snapshot: std::sync::Arc<arc_swap::ArcSwap<forge_pty::snapshot::RenderSnapshot>>,
        grid_size: GridSize,
    ) -> Self {
        let pane_id = PaneId::new(1);
        let pane = Pane::new(pane_id, pty, snapshot, grid_size);
        let mut panes = HashMap::new();
        panes.insert(pane_id, pane);

        Self {
            root: LayoutNode::leaf(pane_id),
            panes,
            active_pane: pane_id,
            zoomed_pane: None,
            next_pane_id: 2,
            layout_generation: 0,
            last_borders: Vec::new(),
            floating_panes: Vec::new(),
        }
    }

    /// Like `single_pane` but uses the given `pane_id` instead of always using PaneId::new(1).
    /// This ensures globally unique pane IDs across tabs.
    pub fn with_single_pane_id(
        pane_id: PaneId,
        pty: forge_pty::Pty,
        snapshot: std::sync::Arc<arc_swap::ArcSwap<forge_pty::snapshot::RenderSnapshot>>,
        grid_size: GridSize,
    ) -> Self {
        let pane = Pane::new(pane_id, pty, snapshot, grid_size);
        let mut panes = HashMap::new();
        panes.insert(pane_id, pane);

        Self {
            root: LayoutNode::leaf(pane_id),
            panes,
            floating_panes: Vec::new(),
            active_pane: pane_id,
            zoomed_pane: None,
            next_pane_id: pane_id.get().saturating_add(1),
            layout_generation: 0,
            last_borders: Vec::new(),
        }
    }

    pub fn active_pane(&self) -> &Pane {
        self.panes
            .get(&self.active_pane)
            .expect("active pane must exist in mux state")
    }

    pub fn active_pane_id(&self) -> PaneId {
        self.active_pane
    }

    pub fn next_pane_id(&self) -> PaneId {
        PaneId::new(self.next_pane_id)
    }

    pub fn add_floating_pane(&mut self, mut pane: Pane) -> PaneId {
        let pane_id = pane.id;
        pane.dirty_layout = true;
        self.next_pane_id = self.next_pane_id.max(pane_id.get() + 1);
        self.floating_panes.push(pane_id);
        self.panes.insert(pane_id, pane);
        self.active_pane = pane_id;
        pane_id
    }

    pub fn remove_floating_pane(&mut self, pane_id: PaneId) -> RemovePaneResult {
        if !self.floating_panes.contains(&pane_id) {
            return RemovePaneResult::MissingPane;
        }
        self.floating_panes.retain(|&id| id != pane_id);
        self.panes.remove(&pane_id);

        let removed_active = self.active_pane == pane_id;
        if removed_active {
            // Focus top-most floating pane, or fallback to first pane
            self.active_pane = self.floating_panes.last().copied().unwrap_or_else(|| self.root.first_pane());
        }

        if self.panes.is_empty() {
            RemovePaneResult::RemovedLastPane
        } else {
            RemovePaneResult::Removed {
                new_active: self.active_pane,
                removed_active,
            }
        }
    }

    pub fn bring_floating_to_front(&mut self, pane_id: PaneId) {
        if let Some(pos) = self.floating_panes.iter().position(|&x| x == pane_id) {
            self.floating_panes.remove(pos);
            self.floating_panes.push(pane_id);
        }
    }

    pub fn focus_pane(&mut self, pane_id: PaneId) -> bool {
        if self.is_zoomed() && self.zoomed_pane != Some(pane_id) {
            return false;
        }
        if !self.panes.contains_key(&pane_id) || self.active_pane == pane_id {
            return false;
        }
        self.active_pane = pane_id;
        true
    }

    pub fn focus_pane_direction(&mut self, dir: Direction) -> bool {
        if self.is_zoomed() {
            return false;
        }
        let active_pane = self.panes.get(&self.active_pane).unwrap();
        let active_rect = active_pane.rect;

        let mut best_pane: Option<PaneId> = None;
        let mut best_score = f32::INFINITY;

        for (pane_id, pane) in &self.panes {
            if *pane_id == self.active_pane {
                continue;
            }

            let rect = pane.rect;

            let valid = match dir {
                Direction::Left => rect.x + rect.width <= active_rect.x,
                Direction::Right => rect.x >= active_rect.x + active_rect.width,
                Direction::Up => rect.y + rect.height <= active_rect.y,
                Direction::Down => rect.y >= active_rect.y + active_rect.height,
            };

            if !valid {
                continue;
            }

            // Calculate a score. Primary score is distance along the primary axis.
            // Secondary score is distance along the cross axis (center to center or overlap).
            let (primary_dist, cross_dist) = match dir {
                Direction::Left => (
                    active_rect.x - (rect.x + rect.width),
                    (active_rect.y + active_rect.height / 2.0) - (rect.y + rect.height / 2.0),
                ),
                Direction::Right => (
                    rect.x - (active_rect.x + active_rect.width),
                    (active_rect.y + active_rect.height / 2.0) - (rect.y + rect.height / 2.0),
                ),
                Direction::Up => (
                    active_rect.y - (rect.y + rect.height),
                    (active_rect.x + active_rect.width / 2.0) - (rect.x + rect.width / 2.0),
                ),
                Direction::Down => (
                    rect.y - (active_rect.y + active_rect.height),
                    (active_rect.x + active_rect.width / 2.0) - (rect.x + rect.width / 2.0),
                ),
            };

            // If the cross distance is within the bounds of the pane, give it a heavy bonus or treat it as cross_dist=0
            let cross_overlap = match dir {
                Direction::Left | Direction::Right => {
                    let overlap_start = active_rect.y.max(rect.y);
                    let overlap_end =
                        (active_rect.y + active_rect.height).min(rect.y + rect.height);
                    overlap_end > overlap_start
                }
                Direction::Up | Direction::Down => {
                    let overlap_start = active_rect.x.max(rect.x);
                    let overlap_end = (active_rect.x + active_rect.width).min(rect.x + rect.width);
                    overlap_end > overlap_start
                }
            };

            let cross_penalty = if cross_overlap {
                0.0
            } else {
                cross_dist.abs() * 100.0
            };
            let score = primary_dist + cross_penalty;

            if score < best_score {
                best_score = score;
                best_pane = Some(*pane_id);
            }
        }

        if let Some(p) = best_pane {
            self.focus_pane(p);
            true
        } else {
            false
        }
    }

    pub fn get_pane(&self, pane_id: PaneId) -> Option<&Pane> {
        self.panes.get(&pane_id)
    }

    pub fn get_pane_mut(&mut self, pane_id: PaneId) -> Option<&mut Pane> {
        self.panes.get_mut(&pane_id)
    }

    pub fn pane_snapshot(
        &self,
        pane_id: PaneId,
    ) -> Option<std::sync::Arc<arc_swap::ArcSwap<forge_pty::snapshot::RenderSnapshot>>> {
        self.panes.get(&pane_id).map(|pane| pane.snapshot.clone())
    }

    pub fn pane_pty_mut(&mut self, pane_id: PaneId) -> Option<&mut forge_pty::Pty> {
        self.get_pane_mut(pane_id)
            .and_then(|pane| pane.pty.as_mut())
    }

    pub fn active_pane_mut(&mut self) -> &mut Pane {
        self.panes
            .get_mut(&self.active_pane)
            .expect("active pane must exist in mux state")
    }

    pub fn active_snapshot(
        &self,
    ) -> &std::sync::Arc<arc_swap::ArcSwap<forge_pty::snapshot::RenderSnapshot>> {
        &self.active_pane().snapshot
    }

    pub fn active_pty_mut(&mut self) -> Option<&mut forge_pty::Pty> {
        self.active_pane_mut().pty.as_mut()
    }

    pub fn pane_at_point(&self, x: f32, y: f32) -> Option<PaneId> {
        if let Some(pane_id) = self.valid_zoomed_pane() {
            return self
                .panes
                .get(&pane_id)
                .filter(|pane| pane.rect.contains_point(x, y))
                .map(|pane| pane.id);
        }

        self.panes
            .values()
            .filter(|pane| pane.rect.contains_point(x, y))
            .max_by_key(|pane| (pane.id == self.active_pane, pane.id))
            .map(|pane| pane.id)
    }

    pub fn point_target(&self, x: f32, y: f32) -> Option<PanePointTarget> {
        let pane_id = self.pane_at_point(x, y)?;
        self.point_target_for_pane(pane_id, x, y)
    }

    pub fn point_target_for_pane(
        &self,
        pane_id: PaneId,
        x: f32,
        y: f32,
    ) -> Option<PanePointTarget> {
        let pane = self.get_pane(pane_id)?;
        let (local_x, local_y) = pane.rect.local_point(x, y);
        Some(PanePointTarget {
            pane_id,
            rect: pane.rect,
            local_x,
            local_y,
        })
    }

    fn split_root_for_active(
        &self,
        axis: SplitAxis,
        new_pane_id: PaneId,
    ) -> Result<LayoutNode, SplitPaneError> {
        if let Some(pane_id) = self.valid_zoomed_pane() {
            return Err(SplitPaneError::Zoomed(pane_id));
        }
        if !self.panes.contains_key(&self.active_pane) || !self.root.contains_pane(self.active_pane)
        {
            return Err(SplitPaneError::MissingActivePane(self.active_pane));
        }

        let mut root = self.root.clone();
        let replacement = LayoutNode::split(
            axis,
            0.5,
            LayoutNode::leaf(self.active_pane),
            LayoutNode::leaf(new_pane_id),
        );
        if !root.replace_leaf(self.active_pane, replacement) {
            return Err(SplitPaneError::MissingActivePane(self.active_pane));
        }

        Ok(root)
    }

    pub fn preview_split_active(
        &self,
        axis: SplitAxis,
        params: LayoutParams,
    ) -> Result<(PaneId, LayoutResult), SplitPaneError> {
        let new_pane_id = self.next_pane_id();
        let root = self.split_root_for_active(axis, new_pane_id)?;
        let layout = compute_layout(&root, params)?;
        Ok((new_pane_id, layout))
    }

    pub fn commit_split_active(
        &mut self,
        axis: SplitAxis,
        pane: Pane,
    ) -> Result<PaneId, SplitPaneError> {
        let new_pane_id = pane.id;
        let root = self.split_root_for_active(axis, new_pane_id)?;
        self.root = root;
        self.panes.insert(new_pane_id, pane);
        self.active_pane = new_pane_id;
        self.next_pane_id = std::cmp::max(self.next_pane_id, new_pane_id.get().saturating_add(1));
        self.layout_generation = self.layout_generation.wrapping_add(1);
        Ok(new_pane_id)
    }

    pub fn insert_detached_pane(&mut self, pane: Pane) -> PaneId {
        let pane_id = pane.id;
        self.panes.insert(pane_id, pane);
        self.active_pane = pane_id;
        self.next_pane_id = std::cmp::max(self.next_pane_id, pane_id.get().saturating_add(1));
        self.layout_generation = self.layout_generation.wrapping_add(1);
        pane_id
    }

    pub fn remove_detached_pane(&mut self, pane_id: PaneId) -> RemovePaneResult {
        if !self.panes.contains_key(&pane_id) {
            return RemovePaneResult::MissingPane;
        }

        if self.zoomed_pane == Some(pane_id) {
            self.zoomed_pane = None;
        }
        let removed_active = self.active_pane == pane_id;
        self.panes.remove(&pane_id);
        self.layout_generation = self.layout_generation.wrapping_add(1);

        if self.panes.is_empty() {
            return RemovePaneResult::RemovedLastPane;
        }

        let new_active = if !removed_active && self.panes.contains_key(&self.active_pane) {
            self.active_pane
        } else {
            self.panes
                .keys()
                .copied()
                .min_by_key(|pane_id| pane_id.get())
                .expect("non-empty pane map must have a fallback active pane")
        };

        self.active_pane = new_active;
        if !self.root.contains_pane(self.root.first_pane()) || self.root.contains_pane(pane_id) {
            self.root = LayoutNode::leaf(new_active);
            self.last_borders.clear();
        }

        RemovePaneResult::Removed {
            new_active,
            removed_active,
        }
    }

    pub fn remove_pane(&mut self, pane_id: PaneId) -> RemovePaneResult {
        if !self.panes.contains_key(&pane_id) || !self.root.contains_pane(pane_id) {
            return RemovePaneResult::MissingPane;
        }

        if self.zoomed_pane == Some(pane_id) {
            self.zoomed_pane = None;
        }
        let removed_active = self.active_pane == pane_id;
        if self.panes.len() == 1 {
            self.panes.remove(&pane_id);
            self.layout_generation = self.layout_generation.wrapping_add(1);
            return RemovePaneResult::RemovedLastPane;
        }

        let root = std::mem::replace(&mut self.root, LayoutNode::leaf(pane_id));
        let (new_root, preferred_focus) = match root.remove_pane(pane_id) {
            RemoveNodeResult::Removed {
                node,
                preferred_focus,
            } => (node, preferred_focus),
            RemoveNodeResult::RemovedRoot => {
                self.panes.remove(&pane_id);
                self.layout_generation = self.layout_generation.wrapping_add(1);
                return RemovePaneResult::RemovedLastPane;
            }
            RemoveNodeResult::NotFound(root) => {
                self.root = root;
                return RemovePaneResult::MissingPane;
            }
        };

        self.panes.remove(&pane_id);
        self.root = new_root;

        let new_active = if !removed_active && self.panes.contains_key(&self.active_pane) {
            self.active_pane
        } else if self.panes.contains_key(&preferred_focus) {
            preferred_focus
        } else {
            self.root.first_pane()
        };

        self.active_pane = new_active;
        if let Some(zoomed) = self.zoomed_pane {
            if !self.panes.contains_key(&zoomed) {
                self.zoomed_pane = None;
            }
        }
        self.layout_generation = self.layout_generation.wrapping_add(1);

        RemovePaneResult::Removed {
            new_active,
            removed_active,
        }
    }

    pub fn relayout(
        &mut self,
        params: LayoutParams,
    ) -> Result<Vec<PaneLayoutChange>, RelayoutError> {
        if let Some(pane_id) = self.valid_zoomed_pane() {
            return self.relayout_zoomed_pane(pane_id, params);
        }

        self.relayout_root(params)
    }

    pub fn toggle_zoom(
        &mut self,
        params: LayoutParams,
    ) -> Result<Vec<PaneLayoutChange>, RelayoutError> {
        if self.panes.len() <= 1 {
            return Ok(Vec::new());
        }

        if self.is_zoomed() {
            self.zoomed_pane = None;
            self.layout_generation = self.layout_generation.wrapping_add(1);
            self.relayout_root(params)
        } else {
            let pane_id = self.active_pane;
            if !self.panes.contains_key(&pane_id) {
                return Err(RelayoutError::MissingPane(pane_id));
            }
            self.zoomed_pane = Some(pane_id);
            self.layout_generation = self.layout_generation.wrapping_add(1);
            self.relayout_zoomed_pane(pane_id, params)
        }
    }

    pub fn is_zoomed(&self) -> bool {
        self.valid_zoomed_pane().is_some()
    }

    pub fn visible_pane_ids(&self) -> Vec<PaneId> {
        if let Some(pane_id) = self.valid_zoomed_pane() {
            vec![pane_id]
        } else {
            self.panes.keys().copied().collect()
        }
    }

    pub fn visible_borders(&self) -> &[crate::mux::layout::SplitBorder] {
        if self.is_zoomed() {
            &[]
        } else {
            &self.last_borders
        }
    }

    fn valid_zoomed_pane(&self) -> Option<PaneId> {
        self.zoomed_pane
            .filter(|pane_id| self.panes.contains_key(pane_id))
    }

    fn relayout_root(
        &mut self,
        params: LayoutParams,
    ) -> Result<Vec<PaneLayoutChange>, RelayoutError> {
        let layout = compute_layout(&self.root, params)?;
        self.apply_layout(layout)
    }

    fn relayout_zoomed_pane(
        &mut self,
        pane_id: PaneId,
        params: LayoutParams,
    ) -> Result<Vec<PaneLayoutChange>, RelayoutError> {
        let layout = compute_layout(&LayoutNode::leaf(pane_id), params)?;
        self.apply_layout(layout)
    }

    fn apply_layout(
        &mut self,
        layout: LayoutResult,
    ) -> Result<Vec<PaneLayoutChange>, RelayoutError> {
        self.last_borders = layout.borders.clone();
        let mut changes = Vec::new();

        for pane_layout in layout.panes {
            let pane = self
                .panes
                .get_mut(&pane_layout.pane_id)
                .ok_or(RelayoutError::MissingPane(pane_layout.pane_id))?;
            let old_grid_size = pane.grid_size;
            let old_rect = pane.rect;

            if old_grid_size != pane_layout.grid_size || old_rect != pane_layout.rect {
                pane.grid_size = pane_layout.grid_size;
                pane.rect = pane_layout.rect;
                pane.dirty_layout = true;
                changes.push(PaneLayoutChange {
                    pane_id: pane.id,
                    old_grid_size,
                    new_grid_size: pane.grid_size,
                    old_rect,
                    new_rect: pane.rect,
                });
            } else {
                pane.dirty_layout = false;
            }
        }

        if !changes.is_empty() {
            self.layout_generation = self.layout_generation.wrapping_add(1);
        }

        Ok(changes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mux_with_single_pane(grid_size: GridSize) -> MuxState {
        let pane_id = PaneId::new(1);
        let pane = Pane::layout_only(pane_id, grid_size);
        let mut panes = HashMap::new();
        panes.insert(pane_id, pane);

        MuxState {
            root: LayoutNode::leaf(pane_id),
            panes,
            active_pane: pane_id,
            zoomed_pane: None,
            next_pane_id: 2,
            layout_generation: 0,
            last_borders: Vec::new(),
            floating_panes: Vec::new(),
        }
    }

    fn mux_with_two_panes() -> MuxState {
        let first_id = PaneId::new(1);
        let second_id = PaneId::new(2);
        let mut first = Pane::layout_only(first_id, GridSize::new(40, 30));
        let mut second = Pane::layout_only(second_id, GridSize::new(40, 30));
        first.rect = PaneRect::new(0.0, 0.0, 400.0, 600.0);
        second.rect = PaneRect::new(400.0, 0.0, 400.0, 600.0);

        let mut panes = HashMap::new();
        panes.insert(first_id, first);
        panes.insert(second_id, second);

        MuxState {
            root: LayoutNode::split(
                SplitAxis::Vertical,
                0.5,
                LayoutNode::leaf(first_id),
                LayoutNode::leaf(second_id),
            ),
            panes,
            active_pane: first_id,
            zoomed_pane: None,
            next_pane_id: 3,
            layout_generation: 0,
            last_borders: Vec::new(),
            floating_panes: Vec::new(),
        }
    }

    fn params(width: f32, height: f32) -> LayoutParams {
        LayoutParams::new(
            PaneRect::new(4.0, 8.0, width, height),
            10.0,
            20.0,
            1.0,
            forge_core::config_registry::PaddingConfig {
                left: 0,
                right: 0,
                top: 0,
                bottom: 0,
            },
        )
    }

    #[test]
    fn relayout_sets_single_pane_rect_without_grid_change() {
        let mut mux = mux_with_single_pane(GridSize::new(80, 30));
        let changes = mux.relayout(params(800.0, 600.0)).unwrap();

        assert_eq!(changes.len(), 1);
        assert!(!changes[0].grid_changed());
        assert_eq!(changes[0].old_grid_size, GridSize::new(80, 30));
        assert_eq!(changes[0].new_grid_size, GridSize::new(80, 30));
        assert_eq!(
            mux.active_pane().rect,
            PaneRect::new(4.0, 8.0, 800.0, 600.0)
        );
        assert_eq!(mux.layout_generation, 1);
    }

    #[test]
    fn unchanged_relayout_reports_no_changes() {
        let mut mux = mux_with_single_pane(GridSize::new(80, 30));

        assert_eq!(mux.relayout(params(800.0, 600.0)).unwrap().len(), 1);
        assert_eq!(mux.relayout(params(800.0, 600.0)).unwrap(), vec![]);
        assert_eq!(mux.layout_generation, 1);
    }

    #[test]
    fn changed_relayout_reports_grid_change() {
        let mut mux = mux_with_single_pane(GridSize::new(80, 30));
        mux.relayout(params(800.0, 600.0)).unwrap();

        let changes = mux.relayout(params(1000.0, 640.0)).unwrap();

        assert_eq!(changes.len(), 1);
        assert!(changes[0].grid_changed());
        assert_eq!(changes[0].old_grid_size, GridSize::new(80, 30));
        assert_eq!(changes[0].new_grid_size, GridSize::new(100, 32));
        assert_eq!(mux.active_pane().grid_size, GridSize::new(100, 32));
        assert_eq!(mux.layout_generation, 2);
    }

    #[test]
    fn relayout_preserves_minimum_pane_constraints() {
        let mut mux = mux_with_single_pane(GridSize::new(80, 30));
        let err = mux.relayout(params(90.0, 60.0)).unwrap_err();

        assert!(matches!(
            err,
            RelayoutError::Layout(LayoutError::PaneBelowMinimum {
                pane_id,
                grid_size: GridSize { cols: 9, rows: 3 },
                ..
            }) if pane_id == PaneId::new(1)
        ));
    }

    #[test]
    fn focus_pane_changes_active_pane_only_for_known_panes() {
        let mut mux = mux_with_two_panes();

        assert_eq!(mux.active_pane_id(), PaneId::new(1));
        assert!(mux.focus_pane(PaneId::new(2)));
        assert_eq!(mux.active_pane_id(), PaneId::new(2));
        assert!(!mux.focus_pane(PaneId::new(2)));
        assert!(!mux.focus_pane(PaneId::new(99)));
        assert_eq!(mux.active_pane_id(), PaneId::new(2));
    }

    #[test]
    fn hit_testing_returns_pane_under_point() {
        let mux = mux_with_two_panes();

        assert_eq!(mux.pane_at_point(10.0, 10.0), Some(PaneId::new(1)));
        assert_eq!(mux.pane_at_point(410.0, 10.0), Some(PaneId::new(2)));
        assert_eq!(mux.pane_at_point(900.0, 10.0), None);
    }

    #[test]
    fn point_target_translates_to_pane_local_coordinates() {
        let mux = mux_with_two_panes();
        let target = mux.point_target(450.0, 25.0).unwrap();

        assert_eq!(target.pane_id, PaneId::new(2));
        assert_eq!(target.rect, PaneRect::new(400.0, 0.0, 400.0, 600.0));
        assert_eq!(target.local_x, 50.0);
        assert_eq!(target.local_y, 25.0);
    }

    #[test]
    fn zoom_renders_only_active_pane_and_restores_layout() {
        let mut mux = mux_with_two_panes();
        mux.relayout(params(800.0, 600.0)).unwrap();
        let original_root = mux.root.clone();
        let original_second_rect = mux.get_pane(PaneId::new(2)).unwrap().rect;

        let changes = mux.toggle_zoom(params(800.0, 600.0)).unwrap();

        assert_eq!(mux.zoomed_pane, Some(PaneId::new(1)));
        assert_eq!(mux.visible_pane_ids(), vec![PaneId::new(1)]);
        assert!(mux.visible_borders().is_empty());
        assert_eq!(mux.root, original_root);
        assert_eq!(
            mux.get_pane(PaneId::new(1)).unwrap().rect,
            PaneRect::new(4.0, 8.0, 800.0, 600.0)
        );
        assert_eq!(
            mux.get_pane(PaneId::new(2)).unwrap().rect,
            original_second_rect
        );
        assert_eq!(changes.len(), 1);

        mux.toggle_zoom(params(800.0, 600.0)).unwrap();

        assert_eq!(mux.zoomed_pane, None);
        assert_eq!(mux.root, original_root);
        assert_eq!(mux.visible_pane_ids().len(), 2);
        assert_eq!(mux.visible_borders().len(), 1);
        assert_eq!(
            mux.get_pane(PaneId::new(2)).unwrap().rect,
            original_second_rect
        );
    }

    #[test]
    fn zoomed_hit_testing_targets_only_zoomed_pane() {
        let mut mux = mux_with_two_panes();
        mux.relayout(params(800.0, 600.0)).unwrap();
        mux.focus_pane(PaneId::new(2));
        mux.toggle_zoom(params(800.0, 600.0)).unwrap();

        assert_eq!(mux.pane_at_point(20.0, 20.0), Some(PaneId::new(2)));
        assert_eq!(
            mux.point_target(20.0, 20.0).unwrap().pane_id,
            PaneId::new(2)
        );
        assert!(!mux.focus_pane(PaneId::new(1)));
    }

    #[test]
    fn split_is_rejected_while_zoomed() {
        let mut mux = mux_with_two_panes();
        mux.relayout(params(800.0, 600.0)).unwrap();
        mux.toggle_zoom(params(800.0, 600.0)).unwrap();

        let err = mux
            .preview_split_active(SplitAxis::Vertical, params(800.0, 600.0))
            .unwrap_err();

        assert_eq!(err, SplitPaneError::Zoomed(PaneId::new(1)));
    }

    #[test]
    fn preview_split_active_validates_layout_without_mutating_mux() {
        let mux = mux_with_single_pane(GridSize::new(80, 30));
        let (new_pane_id, layout) = mux
            .preview_split_active(SplitAxis::Vertical, params(800.0, 600.0))
            .unwrap();

        assert_eq!(new_pane_id, PaneId::new(2));
        assert_eq!(layout.panes.len(), 2);
        assert_eq!(layout.borders.len(), 1);
        assert_eq!(mux.active_pane_id(), PaneId::new(1));
        assert_eq!(mux.panes.len(), 1);
        assert_eq!(mux.next_pane_id(), PaneId::new(2));
    }

    #[test]
    fn preview_split_active_rejects_too_small_pane() {
        let mux = mux_with_single_pane(GridSize::new(10, 3));
        let err = mux
            .preview_split_active(SplitAxis::Horizontal, params(100.0, 60.0))
            .unwrap_err();

        assert!(matches!(err, SplitPaneError::Layout(_)));
        assert_eq!(mux.panes.len(), 1);
    }

    #[test]
    fn commit_split_active_replaces_active_leaf_and_focuses_new_pane() {
        let mut mux = mux_with_single_pane(GridSize::new(80, 30));
        let new_pane = Pane::layout_only(mux.next_pane_id(), GridSize::new(40, 30));

        let new_pane_id = mux
            .commit_split_active(SplitAxis::Vertical, new_pane)
            .unwrap();

        assert_eq!(new_pane_id, PaneId::new(2));
        assert_eq!(mux.active_pane_id(), PaneId::new(2));
        assert_eq!(mux.next_pane_id(), PaneId::new(3));
        assert_eq!(mux.panes.len(), 2);
        assert!(matches!(
            mux.root,
            LayoutNode::Split {
                axis: SplitAxis::Vertical,
                ..
            }
        ));
    }

    #[test]
    fn insert_detached_pane_does_not_mutate_layout_tree() {
        let mut mux = mux_with_single_pane(GridSize::new(80, 30));
        let original_root = mux.root.clone();
        let new_pane = Pane::layout_only(mux.next_pane_id(), GridSize::new(80, 30));

        let new_pane_id = mux.insert_detached_pane(new_pane);

        assert_eq!(new_pane_id, PaneId::new(2));
        assert_eq!(mux.root, original_root);
        assert_eq!(mux.active_pane_id(), PaneId::new(2));
        assert_eq!(mux.next_pane_id(), PaneId::new(3));
        assert!(mux.panes.contains_key(&PaneId::new(1)));
        assert!(mux.panes.contains_key(&PaneId::new(2)));
    }

    #[test]
    fn remove_detached_root_leaf_repairs_root_to_remaining_pane() {
        let mut mux = mux_with_single_pane(GridSize::new(80, 30));
        mux.insert_detached_pane(Pane::layout_only(PaneId::new(2), GridSize::new(80, 30)));
        mux.focus_pane(PaneId::new(1));

        let result = mux.remove_detached_pane(PaneId::new(1));

        assert_eq!(
            result,
            RemovePaneResult::Removed {
                new_active: PaneId::new(2),
                removed_active: true,
            }
        );
        assert_eq!(mux.root, LayoutNode::leaf(PaneId::new(2)));
        assert_eq!(mux.active_pane_id(), PaneId::new(2));
        assert!(!mux.panes.contains_key(&PaneId::new(1)));
    }

    #[test]
    fn remove_leaf_from_two_pane_split_promotes_surviving_sibling() {
        let mut mux = mux_with_two_panes();

        let result = mux.remove_pane(PaneId::new(2));

        assert_eq!(
            result,
            RemovePaneResult::Removed {
                new_active: PaneId::new(1),
                removed_active: false,
            }
        );
        assert_eq!(mux.root, LayoutNode::leaf(PaneId::new(1)));
        assert_eq!(mux.active_pane_id(), PaneId::new(1));
        assert_eq!(mux.panes.len(), 1);
        assert!(!mux.panes.contains_key(&PaneId::new(2)));
    }

    #[test]
    fn remove_pane_from_nested_layout_preserves_sibling_subtree() {
        let mut mux = mux_with_two_panes();
        let third_id = PaneId::new(3);
        mux.panes
            .insert(third_id, Pane::layout_only(third_id, GridSize::new(40, 15)));
        mux.root = LayoutNode::split(
            SplitAxis::Vertical,
            0.5,
            LayoutNode::leaf(PaneId::new(1)),
            LayoutNode::split(
                SplitAxis::Horizontal,
                0.5,
                LayoutNode::leaf(PaneId::new(2)),
                LayoutNode::leaf(third_id),
            ),
        );

        let result = mux.remove_pane(PaneId::new(1));

        assert_eq!(
            result,
            RemovePaneResult::Removed {
                new_active: PaneId::new(2),
                removed_active: true,
            }
        );
        assert_eq!(
            mux.root,
            LayoutNode::split(
                SplitAxis::Horizontal,
                0.5,
                LayoutNode::leaf(PaneId::new(2)),
                LayoutNode::leaf(PaneId::new(3)),
            )
        );
        assert_eq!(mux.active_pane_id(), PaneId::new(2));
        assert_eq!(mux.panes.len(), 2);
    }

    #[test]
    fn removing_active_pane_focuses_surviving_sibling() {
        let mut mux = mux_with_two_panes();
        mux.focus_pane(PaneId::new(2));

        let result = mux.remove_pane(PaneId::new(2));

        assert_eq!(
            result,
            RemovePaneResult::Removed {
                new_active: PaneId::new(1),
                removed_active: true,
            }
        );
        assert_eq!(mux.active_pane_id(), PaneId::new(1));
    }

    #[test]
    fn removing_inactive_pane_preserves_active_pane_when_valid() {
        let mut mux = mux_with_two_panes();
        mux.focus_pane(PaneId::new(2));

        let result = mux.remove_pane(PaneId::new(1));

        assert_eq!(
            result,
            RemovePaneResult::Removed {
                new_active: PaneId::new(2),
                removed_active: false,
            }
        );
        assert_eq!(mux.active_pane_id(), PaneId::new(2));
    }

    #[test]
    fn removing_last_pane_reports_terminal_should_exit() {
        let mut mux = mux_with_single_pane(GridSize::new(80, 30));

        let result = mux.remove_pane(PaneId::new(1));

        assert_eq!(result, RemovePaneResult::RemovedLastPane);
        assert!(mux.panes.is_empty());
    }

    #[test]
    fn invalid_pane_removal_is_safe() {
        let mut mux = mux_with_single_pane(GridSize::new(80, 30));

        let result = mux.remove_pane(PaneId::new(99));

        assert_eq!(result, RemovePaneResult::MissingPane);
        assert_eq!(mux.active_pane_id(), PaneId::new(1));
        assert_eq!(mux.root, LayoutNode::leaf(PaneId::new(1)));
        assert_eq!(mux.panes.len(), 1);
    }
}
