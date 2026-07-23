use super::state::MuxState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(pub u64);

impl TabId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

pub struct Tab {
    pub id: TabId,
    pub mux: MuxState,
    /// The pending window size (pixels) to apply when this tab becomes active
    /// (set when a resize event happens while this tab is inactive)
    pub pending_resize: Option<(u32, u32)>,
}

impl Tab {
    pub fn new(id: TabId, mux: MuxState) -> Self {
        Self {
            id,
            mux,
            pending_resize: None,
        }
    }
}

pub struct TabManager {
    pub tabs: Vec<Tab>,
    pub active_tab_index: usize,
    pub next_tab_id: u64,
    pub next_global_pane_id: u64,
}

impl TabManager {
    pub fn new(initial_mux: MuxState) -> Self {
        // The initial MuxState may have panes with IDs starting at 1.
        // We compute the max used pane id to ensure future allocations don't conflict.
        let max_pane_id = initial_mux
            .panes
            .keys()
            .map(|pid| pid.get())
            .max()
            .unwrap_or(0);
        let next_global_pane_id = max_pane_id.saturating_add(1);
        let tab = Tab::new(TabId::new(1), initial_mux);
        Self {
            tabs: vec![tab],
            active_tab_index: 0,
            next_tab_id: 2,
            next_global_pane_id,
        }
    }

    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active_tab_index]
    }

    pub fn active_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active_tab_index]
    }

    pub fn active_mux(&self) -> &MuxState {
        &self.active_tab().mux
    }

    pub fn active_mux_mut(&mut self) -> &mut MuxState {
        &mut self.active_tab_mut().mux
    }

    /// Allocate a globally unique pane ID (unique across all tabs).
    pub fn alloc_pane_id(&mut self) -> super::pane::PaneId {
        // Also scan all tabs to ensure no collision.
        let max_used = self
            .tabs
            .iter()
            .flat_map(|tab| tab.mux.panes.keys())
            .map(|pid| pid.get())
            .max()
            .unwrap_or(0);
        let id = self.next_global_pane_id.max(max_used + 1);
        self.next_global_pane_id = id.saturating_add(1);
        super::pane::PaneId::new(id)
    }

    pub fn create_tab(&mut self, mux: MuxState) -> TabId {
        let id = TabId::new(self.next_tab_id);
        self.next_tab_id = self.next_tab_id.saturating_add(1);
        // Update next_global_pane_id based on new mux's pane IDs.
        let max_in_new = mux.panes.keys().map(|pid| pid.get()).max().unwrap_or(0);
        if max_in_new >= self.next_global_pane_id {
            self.next_global_pane_id = max_in_new.saturating_add(1);
        }
        self.tabs.push(Tab::new(id, mux));
        self.active_tab_index = self.tabs.len() - 1;
        id
    }

    /// Close the active tab. Returns true if the application should exit (last tab).
    pub fn close_active_tab(&mut self) -> bool {
        if self.tabs.len() == 1 {
            return true; // last tab - signal exit
        }
        self.tabs.remove(self.active_tab_index);
        if self.active_tab_index >= self.tabs.len() {
            self.active_tab_index = self.tabs.len() - 1;
        }
        false
    }

    pub fn switch_next(&mut self) {
        if self.tabs.len() > 1 {
            self.active_tab_index = (self.active_tab_index + 1) % self.tabs.len();
        }
    }

    pub fn switch_previous(&mut self) {
        if self.tabs.len() > 1 {
            self.active_tab_index = if self.active_tab_index == 0 {
                self.tabs.len() - 1
            } else {
                self.active_tab_index - 1
            };
        }
    }

    pub fn switch_to_index(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_tab_index = index;
        }
    }

    pub fn move_active_left(&mut self) {
        if self.active_tab_index > 0 {
            self.tabs
                .swap(self.active_tab_index, self.active_tab_index - 1);
            self.active_tab_index -= 1;
        }
    }

    pub fn move_active_right(&mut self) {
        if self.active_tab_index + 1 < self.tabs.len() {
            self.tabs
                .swap(self.active_tab_index, self.active_tab_index + 1);
            self.active_tab_index += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Find which tab contains a given pane id. Returns the tab index.
    pub fn find_tab_for_pane(&self, pane_id: super::pane::PaneId) -> Option<usize> {
        self.tabs
            .iter()
            .position(|tab| tab.mux.panes.contains_key(&pane_id))
    }

    /// Get a mux by pane id (searches all tabs)
    pub fn mux_for_pane(&self, pane_id: super::pane::PaneId) -> Option<&MuxState> {
        self.tabs
            .iter()
            .find(|tab| tab.mux.panes.contains_key(&pane_id))
            .map(|tab| &tab.mux)
    }

    /// Get a mux_mut by pane id (searches all tabs)
    pub fn mux_for_pane_mut(&mut self, pane_id: super::pane::PaneId) -> Option<&mut MuxState> {
        self.tabs
            .iter_mut()
            .find(|tab| tab.mux.panes.contains_key(&pane_id))
            .map(|tab| &mut tab.mux)
    }

    pub fn move_detached_pane_to_tab(
        &mut self,
        pane_id: super::pane::PaneId,
        destination_tab_id: TabId,
    ) -> bool {
        let Some(source_index) = self.find_tab_for_pane(pane_id) else {
            return false;
        };
        let Some(destination_index) = self
            .tabs
            .iter()
            .position(|tab| tab.id == destination_tab_id)
        else {
            return false;
        };
        if source_index == destination_index
            || self.tabs[source_index]
                .mux
                .floating_panes
                .contains(&pane_id)
            || self.tabs[destination_index]
                .mux
                .panes
                .contains_key(&pane_id)
        {
            return false;
        }

        let Some((pane, _)) = self.tabs[source_index].mux.take_detached_pane(pane_id) else {
            return false;
        };
        self.tabs[destination_index].mux.insert_detached_pane(pane);
        self.tabs[destination_index].mux.zoomed_pane = None;

        if self.tabs[source_index].mux.panes.is_empty() {
            self.tabs.remove(source_index);
        }
        self.active_tab_index = self
            .tabs
            .iter()
            .position(|tab| tab.id == destination_tab_id)
            .expect("destination tab must remain after pane transfer");
        true
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
    fn tab_manager_creates_and_closes_tabs() {
        let mut mgr = TabManager::new(make_mux(1));
        assert_eq!(mgr.len(), 1);
        mgr.create_tab(make_mux(100));
        assert_eq!(mgr.len(), 2);
        assert_eq!(mgr.active_tab_index, 1);
        let exited = mgr.close_active_tab();
        assert!(!exited);
        assert_eq!(mgr.len(), 1);
        assert_eq!(mgr.active_tab_index, 0);
    }

    #[test]
    fn closing_active_tab_shifts_focus() {
        let mut mgr = TabManager::new(make_mux(1));
        mgr.create_tab(make_mux(100));
        mgr.create_tab(make_mux(200));
        mgr.switch_to_index(1);
        mgr.close_active_tab();
        // Should now be at the tab that was at index 2 (now index 1)
        assert_eq!(mgr.active_tab_index, 1);
        assert_eq!(mgr.len(), 2);
    }

    #[test]
    fn closing_last_tab_signals_exit() {
        let mut mgr = TabManager::new(make_mux(1));
        let should_exit = mgr.close_active_tab();
        assert!(should_exit);
    }

    #[test]
    fn global_pane_lookup_resolves_across_tabs() {
        let mut mgr = TabManager::new(make_mux(1));
        mgr.create_tab(make_mux(100));
        mgr.create_tab(make_mux(200));
        // Pane 1 is in tab 0
        assert_eq!(mgr.find_tab_for_pane(PaneId::new(1)), Some(0));
        // Pane 100 is in tab 1
        assert_eq!(mgr.find_tab_for_pane(PaneId::new(100)), Some(1));
        // Pane 200 is in tab 2
        assert_eq!(mgr.find_tab_for_pane(PaneId::new(200)), Some(2));
        // Non-existent pane
        assert_eq!(mgr.find_tab_for_pane(PaneId::new(999)), None);
    }

    #[test]
    fn switching_tabs_updates_active_index() {
        let mut mgr = TabManager::new(make_mux(1));
        mgr.create_tab(make_mux(100));
        mgr.create_tab(make_mux(200));
        mgr.switch_to_index(0);
        assert_eq!(mgr.active_tab_index, 0);
        mgr.switch_next();
        assert_eq!(mgr.active_tab_index, 1);
        mgr.switch_next();
        assert_eq!(mgr.active_tab_index, 2);
        mgr.switch_next();
        assert_eq!(mgr.active_tab_index, 0); // wraps
        mgr.switch_previous();
        assert_eq!(mgr.active_tab_index, 2); // wraps back
    }

    #[test]
    fn pane_zoom_state_is_tab_local() {
        let mut mgr = TabManager::new(make_mux(1));
        mgr.active_mux_mut().zoomed_pane = Some(PaneId::new(1));
        mgr.create_tab(make_mux(100));

        assert_eq!(mgr.active_mux().zoomed_pane, None);

        mgr.switch_to_index(0);
        assert_eq!(mgr.active_mux().zoomed_pane, Some(PaneId::new(1)));
    }

    #[test]
    fn move_tab_left_right() {
        let mut mgr = TabManager::new(make_mux(1));
        let tab2_id = mgr.create_tab(make_mux(100));
        assert_eq!(mgr.active_tab_index, 1);
        mgr.move_active_left();
        assert_eq!(mgr.active_tab_index, 0);
        assert_eq!(mgr.tabs[0].id, tab2_id);
    }

    #[test]
    fn moving_detached_pane_between_tabs_preserves_owned_state() {
        let mut mgr = TabManager::new(make_mux(1));
        let moved_id = PaneId::new(2);
        mgr.active_mux_mut()
            .insert_detached_pane(Pane::layout_only(moved_id, GridSize::new(37, 19)));
        let snapshot = mgr.active_mux().panes[&moved_id].snapshot.clone();
        let destination_id = mgr.create_tab(make_mux(10));
        mgr.switch_to_index(0);

        assert!(mgr.move_detached_pane_to_tab(moved_id, destination_id));

        assert_eq!(mgr.tabs.len(), 2);
        assert_eq!(mgr.active_tab().id, destination_id);
        assert!(!mgr.tabs[0].mux.panes.contains_key(&moved_id));
        let moved = mgr.active_mux().panes.get(&moved_id).unwrap();
        assert_eq!(moved.grid_size, GridSize::new(37, 19));
        assert!(std::sync::Arc::ptr_eq(&moved.snapshot, &snapshot));
        assert_eq!(mgr.active_mux().active_pane_id(), moved_id);
    }

    #[test]
    fn moving_only_pane_removes_source_tab_and_invalid_destinations_are_noops() {
        let mut mgr = TabManager::new(make_mux(1));
        let source_id = mgr.active_tab().id;
        let destination_id = mgr.create_tab(make_mux(10));
        mgr.switch_to_index(0);

        assert!(!mgr.move_detached_pane_to_tab(PaneId::new(1), source_id));
        assert!(!mgr.move_detached_pane_to_tab(PaneId::new(1), TabId::new(999)));
        assert!(mgr.move_detached_pane_to_tab(PaneId::new(1), destination_id));

        assert_eq!(mgr.tabs.len(), 1);
        assert_eq!(mgr.active_tab().id, destination_id);
        assert!(mgr.active_mux().panes.contains_key(&PaneId::new(1)));
        assert!(mgr.active_mux().panes.contains_key(&PaneId::new(10)));
    }
}
