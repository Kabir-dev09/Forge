use forge_core::config_registry::PaneManagerMode;

use super::{
    pane::{GridSize, PaneId},
    scrolling::{
        RenderScrollingPane, ScrollingPaneManager, ScrollingPaneRemoval, VisibleScrollingPane,
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

impl PaneRuntime {
    pub fn from_config(
        mode: PaneManagerMode,
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
                );
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

    pub fn add_tab_from_tiling(&mut self, tab: &Tab, viewport_cols: usize, viewport_rows: usize) {
        let active_pane = tab.mux.active_pane_id();
        let mut panes = ScrollingPaneManager::new(
            viewport_cols.max(1),
            viewport_rows.max(1),
            viewport_cols.max(1),
            viewport_rows.max(1),
        );
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
        state::{LayoutNode, MuxState},
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
            &tabs,
            80,
            24,
            GridSize::new(80, 24),
        );
        tabs.create_tab(make_mux(10));
        let scrolling = runtime.scrolling_mut().expect("scrolling runtime");
        scrolling.add_tab_from_tiling(tabs.active_tab(), 80, 24);

        assert_eq!(scrolling.active_pane_id(), Some(PaneId::new(10)));
        assert_eq!(scrolling.visible_pane_ids(), vec![PaneId::new(10)]);
    }

    #[test]
    fn scrolling_runtime_adds_split_panes_to_virtual_space() {
        let tabs = TabManager::new(make_mux(1));
        let mut runtime = PaneRuntime::from_config(
            PaneManagerMode::Scrolling,
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
    fn scrolling_runtime_focuses_clicked_pane() {
        let tabs = TabManager::new(make_mux(1));
        let mut runtime = PaneRuntime::from_config(
            PaneManagerMode::Scrolling,
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
