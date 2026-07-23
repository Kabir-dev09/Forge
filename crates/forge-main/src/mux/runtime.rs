use forge_core::config_registry::PaneManagerMode;

use super::{
    pane::{GridSize, PaneId},
    scrolling::{
        RenderScrollingPane, ScrollingPaneManager, ScrollingPaneRemoval, VirtualPaneRect,
        VisibleScrollingPane,
    },
    state::Direction,
    tab::{Tab, TabId, TabManager},
};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneRuntimeKind {
    Tiling,
    Scrolling,
}

pub enum PaneRuntime {
    Tiling,
    Scrolling(ScrollingTabManager),
}

pub struct ScrollingTab {
    pub id: TabId,
    pub panes: ScrollingPaneManager,
}

pub struct ScrollingTabManager {
    pub tabs: Vec<ScrollingTab>,
    pub active_tab_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrollingPaneTabMove {
    pub pane_id: PaneId,
    pub source_tab_id: TabId,
    pub destination_tab_id: TabId,
    pub source_tab_removed: bool,
    pub source_grid_changes: Vec<(PaneId, GridSize)>,
    pub destination_grid_changes: Vec<(PaneId, GridSize)>,
    pub destination_previous_rects: Vec<(PaneId, VirtualPaneRect)>,
}

impl PaneRuntime {
    pub fn from_config(
        mode: PaneManagerMode,
        animation_duration_ms: u64,
        tiling_tabs: &TabManager,
        viewport_cols: usize,
        viewport_rows: usize,
        fallback_pane_size: GridSize,
    ) -> Self {
        match mode {
            PaneManagerMode::Tiling => Self::Tiling,
            PaneManagerMode::Scrolling => Self::Scrolling(ScrollingTabManager::from_tiling_tabs(
                tiling_tabs,
                viewport_cols,
                viewport_rows,
                fallback_pane_size,
                animation_duration_ms,
            )),
        }
    }

    pub fn kind(&self) -> PaneRuntimeKind {
        match self {
            Self::Tiling => PaneRuntimeKind::Tiling,
            Self::Scrolling(_) => PaneRuntimeKind::Scrolling,
        }
    }

    pub fn is_tiling(&self) -> bool {
        matches!(self, Self::Tiling)
    }

    pub fn scrolling(&self) -> Option<&ScrollingTabManager> {
        match self {
            Self::Tiling => None,
            Self::Scrolling(manager) => Some(manager),
        }
    }

    pub fn scrolling_mut(&mut self) -> Option<&mut ScrollingTabManager> {
        match self {
            Self::Tiling => None,
            Self::Scrolling(manager) => Some(manager),
        }
    }
}

impl ScrollingTabManager {
    pub fn from_tiling_tabs(
        tiling_tabs: &TabManager,
        viewport_cols: usize,
        viewport_rows: usize,
        _fallback_pane_size: GridSize,
        animation_duration_ms: u64,
    ) -> Self {
        let tabs = tiling_tabs
            .tabs
            .iter()
            .map(|tab| {
                let active_pane = tab.mux.active_pane_id();
                let mut panes = ScrollingPaneManager::new(
                    viewport_cols.max(1),
                    viewport_rows.max(1),
                    viewport_cols.max(1),
                    viewport_rows.max(1),
                ).with_animation_duration(animation_duration_ms);
                panes.add_existing_pane_at(active_pane, 0, 0);
                ScrollingTab { id: tab.id, panes }
            })
            .collect();

        Self {
            tabs,
            active_tab_index: tiling_tabs.active_tab_index,
        }
    }

    pub fn active_tab(&self) -> Option<&ScrollingTab> {
        self.tabs.get(self.active_tab_index)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut ScrollingTab> {
        self.tabs.get_mut(self.active_tab_index)
    }

    pub fn sync_active_tab_index(&mut self, active_tab_index: usize) -> bool {
        if active_tab_index >= self.tabs.len() || self.active_tab_index == active_tab_index {
            return false;
        }
        self.active_tab_index = active_tab_index;
        if let Some(tab) = self.active_tab_mut() {
            tab.panes.cancel_scroll_animation();
        }
        true
    }

    pub fn add_tab_from_tiling(&mut self, tab: &Tab, viewport_cols: usize, viewport_rows: usize, animation_duration_ms: u64) {
        let active_pane = tab.mux.active_pane_id();
        let mut panes = ScrollingPaneManager::new(
            viewport_cols.max(1),
            viewport_rows.max(1),
            viewport_cols.max(1),
            viewport_rows.max(1),
        ).with_animation_duration(animation_duration_ms);
        panes.add_existing_pane_at(active_pane, 0, 0);
        self.tabs.push(ScrollingTab { id: tab.id, panes });
        self.active_tab_index = self.tabs.len().saturating_sub(1);
    }

    pub fn remove_tab(&mut self, tab_id: TabId) -> bool {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return false;
        };
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.active_tab_index = 0;
        } else if index < self.active_tab_index {
            self.active_tab_index -= 1;
        } else if self.active_tab_index >= self.tabs.len() {
            self.active_tab_index = self.tabs.len() - 1;
        }
        true
    }

    pub fn add_pane_right_of_active(&mut self, pane_id: PaneId) -> bool {
        let Some(tab) = self.active_tab_mut() else {
            return false;
        };
        tab.panes.add_existing_pane_right_of_active(pane_id);
        true
    }

    pub fn add_pane_below_active(&mut self, pane_id: PaneId) -> bool {
        let Some(tab) = self.active_tab_mut() else {
            return false;
        };
        tab.panes.add_existing_pane_below_active(pane_id);
        true
    }

    pub fn remove_pane(&mut self, pane_id: PaneId) -> bool {
        self.remove_pane_with_changes(pane_id).removed
    }

    pub fn remove_pane_with_changes(&mut self, pane_id: PaneId) -> ScrollingPaneRemoval {
        self.active_tab_mut()
            .map(|tab| tab.panes.remove_pane_with_changes(pane_id))
            .unwrap_or_else(|| ScrollingPaneRemoval {
                removed: false,
                grid_changes: Vec::new(),
            })
    }

    pub fn remove_pane_any(&mut self, pane_id: PaneId) -> bool {
        self.remove_pane_any_with_changes(pane_id).removed
    }

    pub fn remove_pane_any_with_changes(&mut self, pane_id: PaneId) -> ScrollingPaneRemoval {
        for tab in &mut self.tabs {
            let removal = tab.panes.remove_pane_with_changes(pane_id);
            if removal.removed {
                return removal;
            }
        }
        ScrollingPaneRemoval {
            removed: false,
            grid_changes: Vec::new(),
        }
    }

    pub fn active_pane_id(&self) -> Option<PaneId> {
        self.active_tab().and_then(|tab| tab.panes.active_pane())
    }

    pub fn planned_split_grid_size(&self, axis: super::state::SplitAxis) -> Option<GridSize> {
        self.active_tab()
            .and_then(|tab| tab.panes.planned_split_grid_size(axis))
    }

    pub fn split_active_with_existing(
        &mut self,
        axis: super::state::SplitAxis,
        pane_id: PaneId,
    ) -> Option<Vec<(PaneId, GridSize)>> {
        self.active_tab_mut()
            .and_then(|tab| tab.panes.split_active_with_existing(axis, pane_id))
    }

    pub fn is_zoomed(&self) -> bool {
        self.active_tab()
            .map(|tab| tab.panes.is_zoomed())
            .unwrap_or(false)
    }

    pub fn toggle_zoom_active(&mut self) -> Option<Vec<(PaneId, GridSize)>> {
        self.active_tab_mut()
            .and_then(|tab| tab.panes.toggle_zoom_active())
    }

    pub fn focus_pane_direction(&mut self, dir: Direction) -> bool {
        self.active_tab_mut()
            .map(|tab| tab.panes.focus_pane_direction(dir))
            .unwrap_or(false)
    }

    pub fn move_active_pane_direction(&mut self, dir: Direction) -> bool {
        self.active_tab_mut()
            .map(|tab| tab.panes.move_active_pane_direction(dir))
            .unwrap_or(false)
    }

    pub fn move_active_pane_to_tab(
        &mut self,
        destination_tab_id: TabId,
    ) -> Option<ScrollingPaneTabMove> {
        let source_index = self.active_tab_index;
        let destination_index = self
            .tabs
            .iter()
            .position(|tab| tab.id == destination_tab_id)?;
        if source_index == destination_index {
            return None;
        }

        let pane_id = self.tabs.get(source_index)?.panes.active_pane()?;
        let pane = self.tabs[source_index].panes.transferred_pane(pane_id)?;
        if self.tabs[destination_index]
            .panes
            .transferred_pane(pane_id)
            .is_some()
        {
            return None;
        }

        let source_tab_id = self.tabs[source_index].id;
        let destination_previous_rects = self.tabs[destination_index]
            .panes
            .pane_rects()
            .collect();
        let removal = self.tabs[source_index]
            .panes
            .remove_pane_with_changes(pane_id);
        if !removal.removed {
            return None;
        }
        let destination_grid_changes = self.tabs[destination_index]
            .panes
            .insert_transferred_pane(pane)?;

        let source_tab_removed = self.tabs[source_index].panes.pane_count() == 0;
        let source_grid_changes = if source_tab_removed {
            Vec::new()
        } else {
            removal.grid_changes
        };
        if source_tab_removed {
            self.tabs.remove(source_index);
        }
        self.active_tab_index = self
            .tabs
            .iter()
            .position(|tab| tab.id == destination_tab_id)
            .expect("destination scrolling tab must remain after pane transfer");

        Some(ScrollingPaneTabMove {
            pane_id,
            source_tab_id,
            destination_tab_id,
            source_tab_removed,
            source_grid_changes,
            destination_grid_changes,
            destination_previous_rects,
        })
    }

    pub fn focus_pane(&mut self, pane_id: PaneId) -> bool {
        self.active_tab_mut()
            .map(|tab| tab.panes.focus_pane(pane_id))
            .unwrap_or(false)
    }

    pub fn visible_pane_ids(&self) -> Vec<PaneId> {
        self.active_tab()
            .map(|tab| tab.panes.visible_pane_ids_uncached())
            .unwrap_or_default()
    }

    pub fn active_visible_panes(&mut self) -> &[VisibleScrollingPane] {
        self.active_tab_mut()
            .map(|tab| tab.panes.visible_panes())
            .unwrap_or(&[])
    }

    pub fn active_render_visible_panes(&mut self, now: Instant) -> Vec<RenderScrollingPane> {
        self.active_tab_mut()
            .map(|tab| tab.panes.render_visible_panes(now))
            .unwrap_or_default()
    }

    pub fn active_scroll_animation_active(&self, now: Instant) -> bool {
        self.active_tab()
            .map(|tab| tab.panes.has_active_scroll_animation(now))
            .unwrap_or(false)
    }

    pub fn cancel_active_scroll_animation(&mut self) {
        if let Some(tab) = self.active_tab_mut() {
            tab.panes.cancel_scroll_animation();
        }
    }

    pub fn set_active_viewport_size(
        &mut self,
        cols: usize,
        rows: usize,
    ) -> Vec<(PaneId, GridSize)> {
        self.active_tab_mut()
            .map(|tab| tab.panes.set_viewport_size(cols, rows))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mux::{
        pane::{GridSize, Pane, PaneId},
        state::{LayoutNode, MuxState, SplitAxis},
    };
    use std::collections::HashMap;

    fn make_mux(first_pane_id: u64) -> MuxState {
        let pane_id = PaneId::new(first_pane_id);
        let pane = Pane::layout_only(pane_id, GridSize::new(80, 24));
        let mut panes = HashMap::new();
        panes.insert(pane_id, pane);
        MuxState {
            root: LayoutNode::leaf(pane_id),
            panes,
            active_pane: pane_id,
            zoomed_pane: None,
            next_pane_id: first_pane_id + 1,
            layout_generation: 0,
            last_borders: vec![],
            floating_panes: Vec::new(),
        }
    }

    #[test]
    fn default_config_constructs_tiling_runtime_without_scrolling_manager() {
        let tabs = TabManager::new(make_mux(1));
        let runtime = PaneRuntime::from_config(
            PaneManagerMode::Tiling,
            120,
            &tabs,
            80,
            24,
            GridSize::new(80, 24),
        );

        assert!(runtime.is_tiling());
        assert_eq!(runtime.kind(), PaneRuntimeKind::Tiling);
        assert!(runtime.scrolling().is_none());
    }

    #[test]
    fn scrolling_config_constructs_scrolling_runtime() {
        let tabs = TabManager::new(make_mux(7));
        let mut runtime = PaneRuntime::from_config(
            PaneManagerMode::Scrolling,
            120,
            &tabs,
            120,
            40,
            GridSize::new(80, 24),
        );

        assert_eq!(runtime.kind(), PaneRuntimeKind::Scrolling);
        let scrolling = runtime.scrolling_mut().expect("scrolling runtime");
        assert_eq!(scrolling.active_pane_id(), Some(PaneId::new(7)));
        assert_eq!(scrolling.visible_pane_ids(), vec![PaneId::new(7)]);
        assert_eq!(
            scrolling.active_visible_panes()[0].grid_size,
            GridSize::new(120, 40)
        );
    }

    #[test]
    fn scrolling_viewport_resize_reports_grid_changes() {
        let tabs = TabManager::new(make_mux(1));
        let mut runtime = PaneRuntime::from_config(
            PaneManagerMode::Scrolling,
            120,
            &tabs,
            160,
            24,
            GridSize::new(80, 24),
        );
        let scrolling = runtime.scrolling_mut().expect("scrolling runtime");

        let changes = scrolling.set_active_viewport_size(100, 30);

        assert_eq!(changes, vec![(PaneId::new(1), GridSize::new(100, 30))]);
        assert_eq!(scrolling.visible_pane_ids(), vec![PaneId::new(1)]);
    }

    #[test]
    fn scrolling_render_subset_excludes_invisible_panes() {
        let tabs = TabManager::new(make_mux(1));
        let mut runtime = PaneRuntime::from_config(
            PaneManagerMode::Scrolling,
            120,
            &tabs,
            80,
            24,
            GridSize::new(80, 24),
        );
        let scrolling = runtime.scrolling_mut().expect("scrolling runtime");
        let tab = scrolling.active_tab_mut().expect("active scrolling tab");
        tab.panes.add_existing_pane_at(PaneId::new(2), 200, 0);

        let visible: Vec<_> = scrolling
            .active_visible_panes()
            .iter()
            .map(|pane| pane.pane_id)
            .collect();

        assert_eq!(visible, vec![PaneId::new(1)]);
    }

    #[test]
    fn scrolling_runtime_tracks_new_active_tab() {
        let mut tabs = TabManager::new(make_mux(1));
        let mut runtime = PaneRuntime::from_config(
            PaneManagerMode::Scrolling,
            120,
            &tabs,
            80,
            24,
            GridSize::new(80, 24),
        );
        tabs.create_tab(make_mux(10));
        let scrolling = runtime.scrolling_mut().expect("scrolling runtime");
        scrolling.add_tab_from_tiling(tabs.active_tab(), 80, 24, 120);

        assert_eq!(scrolling.active_pane_id(), Some(PaneId::new(10)));
        assert_eq!(scrolling.visible_pane_ids(), vec![PaneId::new(10)]);
    }

    #[test]
    fn scrolling_runtime_adds_split_panes_to_virtual_space() {
        let tabs = TabManager::new(make_mux(1));
        let mut runtime = PaneRuntime::from_config(
            PaneManagerMode::Scrolling,
            120,
            &tabs,
            200,
            80,
            GridSize::new(80, 24),
        );
        let scrolling = runtime.scrolling_mut().expect("scrolling runtime");

        assert!(scrolling.add_pane_right_of_active(PaneId::new(2)));
        assert!(scrolling.add_pane_below_active(PaneId::new(3)));

        assert_eq!(scrolling.active_pane_id(), Some(PaneId::new(3)));
        assert_eq!(
            scrolling.visible_pane_ids(),
            vec![PaneId::new(1), PaneId::new(2), PaneId::new(3)]
        );
    }

    #[test]
    fn scrolling_runtime_focuses_panes_by_direction() {
        let tabs = TabManager::new(make_mux(1));
        let mut runtime = PaneRuntime::from_config(
            PaneManagerMode::Scrolling,
            120,
            &tabs,
            200,
            80,
            GridSize::new(80, 24),
        );
        let scrolling = runtime.scrolling_mut().expect("scrolling runtime");
        assert!(scrolling.add_pane_right_of_active(PaneId::new(2)));
        assert!(scrolling.add_pane_below_active(PaneId::new(3)));

        assert_eq!(scrolling.active_pane_id(), Some(PaneId::new(3)));
        assert!(scrolling.focus_pane_direction(Direction::Up));
        assert_eq!(scrolling.active_pane_id(), Some(PaneId::new(2)));
        assert!(scrolling.focus_pane_direction(Direction::Left));
        assert_eq!(scrolling.active_pane_id(), Some(PaneId::new(1)));
    }

    #[test]
    fn scrolling_runtime_moves_active_pane_without_changing_focus() {
        let tabs = TabManager::new(make_mux(1));
        let mut runtime = PaneRuntime::from_config(
            PaneManagerMode::Scrolling,
            120,
            &tabs,
            200,
            80,
            GridSize::new(80, 24),
        );
        let scrolling = runtime.scrolling_mut().expect("scrolling runtime");
        assert!(scrolling.add_pane_right_of_active(PaneId::new(2)));
        assert!(scrolling.focus_pane(PaneId::new(1)));

        assert!(scrolling.move_active_pane_direction(Direction::Right));
        assert_eq!(scrolling.active_pane_id(), Some(PaneId::new(1)));
    }

    #[test]
    fn scrolling_runtime_splits_single_pane_destination_and_reports_resizes() {
        let mut tabs = TabManager::new(make_mux(1));
        let destination_id = tabs.create_tab(make_mux(10));
        tabs.switch_to_index(0);
        let mut runtime = PaneRuntime::from_config(
            PaneManagerMode::Scrolling,
            120,
            &tabs,
            80,
            24,
            GridSize::new(80, 24),
        );
        let scrolling = runtime.scrolling_mut().unwrap();
        scrolling
            .active_tab_mut()
            .unwrap()
            .panes
            .split_active_with_existing(SplitAxis::Vertical, PaneId::new(2))
            .unwrap();
        let moved_before = scrolling
            .active_tab()
            .unwrap()
            .panes
            .transferred_pane(PaneId::new(2))
            .unwrap();

        let moved = scrolling
            .move_active_pane_to_tab(destination_id)
            .expect("destination tab exists");

        assert_eq!(moved.pane_id, PaneId::new(2));
        assert!(!moved.source_tab_removed);
        assert_eq!(
            moved.destination_previous_rects,
            vec![(PaneId::new(10), VirtualPaneRect::new(0, 0, 80, 24))]
        );
        assert_eq!(
            moved.destination_grid_changes,
            vec![
                (PaneId::new(10), GridSize::new(39, 24)),
                (PaneId::new(2), GridSize::new(40, 24)),
            ]
        );
        assert_eq!(scrolling.active_tab().unwrap().id, destination_id);
        let destination_pane = scrolling
            .active_tab()
            .unwrap()
            .panes
            .transferred_pane(PaneId::new(2))
            .unwrap();
        assert_eq!(destination_pane.grid_size, GridSize::new(40, 24));
        assert_eq!(destination_pane.virtual_rect.cols, 40);
        assert_eq!(destination_pane.virtual_rect.rows, moved_before.virtual_rect.rows);
    }

    #[test]
    fn scrolling_runtime_removes_empty_source_tab_after_transfer() {
        let mut tabs = TabManager::new(make_mux(1));
        let destination_id = tabs.create_tab(make_mux(10));
        tabs.switch_to_index(0);
        let mut runtime = PaneRuntime::from_config(
            PaneManagerMode::Scrolling,
            120,
            &tabs,
            80,
            24,
            GridSize::new(80, 24),
        );
        let scrolling = runtime.scrolling_mut().unwrap();

        let moved = scrolling
            .move_active_pane_to_tab(destination_id)
            .expect("destination tab exists");

        assert!(moved.source_tab_removed);
        assert_eq!(scrolling.tabs.len(), 1);
        assert_eq!(scrolling.active_tab().unwrap().id, destination_id);
        assert_eq!(scrolling.active_pane_id(), Some(PaneId::new(1)));
        assert!(scrolling.move_active_pane_to_tab(destination_id).is_none());
        assert!(scrolling.move_active_pane_to_tab(TabId::new(999)).is_none());
    }

    #[test]
    fn scrolling_runtime_focuses_clicked_pane() {
        let tabs = TabManager::new(make_mux(1));
        let mut runtime = PaneRuntime::from_config(
            PaneManagerMode::Scrolling,
            120,
            &tabs,
            200,
            80,
            GridSize::new(80, 24),
        );
        let scrolling = runtime.scrolling_mut().expect("scrolling runtime");
        assert!(scrolling.add_pane_right_of_active(PaneId::new(2)));

        assert!(scrolling.focus_pane(PaneId::new(1)));
        assert!(scrolling.focus_pane(PaneId::new(2)));
        assert_eq!(scrolling.active_pane_id(), Some(PaneId::new(2)));
        assert!(!scrolling.focus_pane(PaneId::new(99)));
        assert_eq!(scrolling.active_pane_id(), Some(PaneId::new(2)));
    }
}
