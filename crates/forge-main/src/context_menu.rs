use forge_renderer::grid_tessellator::{ContextMenuRenderData, ContextMenuRenderItem};

const SPLIT_ITEM_INDEX: usize = 2;

const EMPTY_ITEM: ContextMenuItem = ContextMenuItem {
    label: "",
    action: ContextMenuAction::Copy,
    right_label: None,
};

const EMPTY_RENDER_ITEM: ContextMenuRenderItem<'static> = ContextMenuRenderItem {
    label: "",
    right_label: None,
};

const SPLIT_RENDER_ITEMS: [ContextMenuRenderItem<'static>; 2] = [
    ContextMenuRenderItem {
        label: "Horizontal",
        right_label: None,
    },
    ContextMenuRenderItem {
        label: "Vertical",
        right_label: None,
    },
];

const SPLIT_ITEMS: [ContextMenuItem; 2] = [
    ContextMenuItem {
        label: "Horizontal",
        action: ContextMenuAction::SplitHorizontal,
        right_label: None,
    },
    ContextMenuItem {
        label: "Vertical",
        action: ContextMenuAction::SplitVertical,
        right_label: None,
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextMenuAction {
    Copy,
    Paste,
    Split,
    SplitHorizontal,
    SplitVertical,
    ZoomPane,
    TogglePaneFloating,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextMenuItem {
    pub label: &'static str,
    pub action: ContextMenuAction,
    pub right_label: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContextMenuState {
    x: f64,
    y: f64,
    hovered_item: Option<usize>,
    hovered_split_item: Option<usize>,
    split_submenu_open: bool,
    target_pane: Option<crate::mux::PaneId>,
    items: [ContextMenuItem; 5],
    render_items: [ContextMenuRenderItem<'static>; 5],
    num_items: usize,
}

impl ContextMenuState {
    pub fn open(x: f64, y: f64) -> Self {
        Self::open_for_pane(x, y, None, true, false)
    }

    pub fn open_for_pane(
        x: f64,
        y: f64,
        target_pane: Option<crate::mux::PaneId>,
        can_zoom: bool,
        is_floating: bool,
    ) -> Self {
        let mut items = [EMPTY_ITEM; 5];
        let mut render_items = [EMPTY_RENDER_ITEM; 5];
        let mut num_items = 0;

        let mut add_item = |label: &'static str, action: ContextMenuAction, right_label: Option<&'static str>| {
            items[num_items] = ContextMenuItem { label, action, right_label };
            render_items[num_items] = ContextMenuRenderItem { label, right_label };
            num_items += 1;
        };

        add_item("Copy", ContextMenuAction::Copy, None);
        add_item("Paste", ContextMenuAction::Paste, None);
        add_item("Split", ContextMenuAction::Split, Some(">"));

        if can_zoom {
            add_item("Zoom Pane", ContextMenuAction::ZoomPane, None);
        }

        if is_floating {
            add_item("Dock Pane", ContextMenuAction::TogglePaneFloating, None);
        }

        Self {
            x,
            y,
            hovered_item: None,
            hovered_split_item: None,
            split_submenu_open: false,
            target_pane,
            items,
            render_items,
            num_items,
        }
    }

    pub fn target_pane(self) -> Option<crate::mux::PaneId> {
        self.target_pane
    }

    pub fn target_pane_id(&self) -> Option<crate::mux::PaneId> {
        self.target_pane
    }

    pub fn update_hover(
        &mut self,
        x: f64,
        y: f64,
        window_width: f64,
        window_height: f64,
        cell_w: f64,
        cell_h: f64,
    ) -> bool {
        let hovered_item = self.hit_test(x, y, window_width, window_height, cell_w, cell_h);
        let hovered_split_item =
            self.hit_test_split_submenu(x, y, window_width, window_height, cell_w, cell_h);
        let split_submenu_open = match hovered_item {
            Some(SPLIT_ITEM_INDEX) => true,
            Some(_) => false,
            None => hovered_split_item.is_some() || self.split_submenu_open,
        };

        if hovered_item == self.hovered_item
            && hovered_split_item == self.hovered_split_item
            && split_submenu_open == self.split_submenu_open
        {
            return false;
        }
        self.hovered_item = hovered_item;
        self.hovered_split_item = hovered_split_item;
        self.split_submenu_open = split_submenu_open;
        true
    }

    pub fn action_at(
        &self,
        x: f64,
        y: f64,
        window_width: f64,
        window_height: f64,
        cell_w: f64,
        cell_h: f64,
    ) -> Option<ContextMenuAction> {
        if let Some(index) =
            self.hit_test_split_submenu(x, y, window_width, window_height, cell_w, cell_h)
        {
            return Some(SPLIT_ITEMS[index].action);
        }

        self.hit_test(x, y, window_width, window_height, cell_w, cell_h)
            .map(|index| self.items[index].action)
    }

    pub fn open_split_submenu(&mut self) {
        self.split_submenu_open = true;
        self.hovered_item = Some(SPLIT_ITEM_INDEX);
    }

    pub fn render_data<'a>(
        &'a self,
        window_width: f64,
        window_height: f64,
        cell_w: f32,
        cell_h: f32,
        background_color: [f32; 4],
    ) -> ContextMenuRenderData<'a> {
        let (x, y) = self.origin(window_width, window_height, cell_w as f64, cell_h as f64);
        let submenu = self.split_submenu_open.then(|| {
            let (submenu_x, submenu_y) = self.split_submenu_origin(
                window_width,
                window_height,
                cell_w as f64,
                cell_h as f64,
            );
            forge_renderer::grid_tessellator::ContextMenuPanelRenderData {
                x: submenu_x as f32,
                y: submenu_y as f32,
                width: Self::items_width(&SPLIT_ITEMS, cell_w as f64) as f32,
                item_height: cell_h,
                hovered_item: self.hovered_split_item,
                items: &SPLIT_RENDER_ITEMS,
            }
        });

        ContextMenuRenderData {
            x: x as f32,
            y: y as f32,
            width: self.width(cell_w as f64) as f32,
            item_height: cell_h,
            hovered_item: self.hovered_item,
            items: &self.render_items[..self.num_items],
            submenu,
            background_color,
        }
    }

    pub fn contains(
        &self,
        x: f64,
        y: f64,
        window_width: f64,
        window_height: f64,
        cell_w: f64,
        cell_h: f64,
    ) -> bool {
        let (origin_x, origin_y) = self.origin(window_width, window_height, cell_w, cell_h);
        let relative_x = x - origin_x;
        let relative_y = y - origin_y;
        let in_main = relative_x >= 0.0
            && relative_y >= 0.0
            && relative_x < self.width(cell_w)
            && relative_y < self.height(cell_h);
        in_main
            || self
                .hit_test_split_submenu(x, y, window_width, window_height, cell_w, cell_h)
                .is_some()
    }

    fn hit_test(
        &self,
        x: f64,
        y: f64,
        window_width: f64,
        window_height: f64,
        cell_w: f64,
        cell_h: f64,
    ) -> Option<usize> {
        let (origin_x, origin_y) = self.origin(window_width, window_height, cell_w, cell_h);
        let relative_x = x - origin_x;
        let relative_y = y - origin_y;

        let width = self.width(cell_w);
        let height = self.height(cell_h);

        if relative_x < 0.0 || relative_y < 0.0 || relative_x >= width || relative_y >= height {
            return None;
        }

        // Inside the grid.
        // We only return an index if we are inside the item rows (not borders)
        // Item rows start at y = cell_h, and end at y = height - cell_h
        if relative_y < cell_h || relative_y >= height - cell_h {
            return None; // Clicking top or bottom border
        }

        // Exclude left/right border clicks? Actually, clicking anywhere on the item row is fine.
        let index = ((relative_y - cell_h) / cell_h).floor() as usize;
        (index < self.num_items).then_some(index)
    }

    fn origin(
        &self,
        window_width: f64,
        window_height: f64,
        cell_w: f64,
        cell_h: f64,
    ) -> (f64, f64) {
        let max_x = (window_width - self.width(cell_w)).max(0.0);
        let max_y = (window_height - self.height(cell_h)).max(0.0);
        (self.x.min(max_x).max(0.0), self.y.min(max_y).max(0.0))
    }

    fn split_submenu_origin(
        &self,
        window_width: f64,
        window_height: f64,
        cell_w: f64,
        cell_h: f64,
    ) -> (f64, f64) {
        let (main_x, main_y) = self.origin(window_width, window_height, cell_w, cell_h);
        let submenu_width = Self::items_width(&SPLIT_ITEMS, cell_w);
        let right_x = main_x + self.width(cell_w);
        let x = if right_x + submenu_width <= window_width {
            right_x
        } else {
            (main_x - submenu_width).max(0.0)
        };
        let y = (main_y + ((SPLIT_ITEM_INDEX + 1) as f64 * cell_h))
            .min((window_height - Self::items_height(&SPLIT_ITEMS, cell_h)).max(0.0))
            .max(0.0);
        (x, y)
    }

    fn hit_test_split_submenu(
        &self,
        x: f64,
        y: f64,
        window_width: f64,
        window_height: f64,
        cell_w: f64,
        cell_h: f64,
    ) -> Option<usize> {
        if !self.split_submenu_open {
            return None;
        }

        let (origin_x, origin_y) =
            self.split_submenu_origin(window_width, window_height, cell_w, cell_h);
        Self::hit_test_items(&SPLIT_ITEMS, x - origin_x, y - origin_y, cell_w, cell_h)
    }

    fn width(&self, cell_w: f64) -> f64 {
        Self::items_width(&self.items[..self.num_items], cell_w)
    }

    fn height(&self, cell_h: f64) -> f64 {
        Self::items_height(&self.items[..self.num_items], cell_h)
    }

    fn items_width(items: &[ContextMenuItem], cell_w: f64) -> f64 {
        let max_len = items
            .iter()
            .map(|item| {
                item.label.chars().count()
                    + item
                        .right_label
                        .map(|right| right.chars().count() + 1)
                        .unwrap_or(0)
            })
            .max()
            .unwrap_or(0);
        let cols = max_len + 4;
        cols as f64 * cell_w
    }

    fn items_height(items: &[ContextMenuItem], cell_h: f64) -> f64 {
        let rows = items.len() + 2;
        rows as f64 * cell_h
    }

    fn hit_test_items(
        items: &[ContextMenuItem],
        relative_x: f64,
        relative_y: f64,
        cell_w: f64,
        cell_h: f64,
    ) -> Option<usize> {
        let width = Self::items_width(items, cell_w);
        let height = Self::items_height(items, cell_h);

        if relative_x < 0.0 || relative_y < 0.0 || relative_x >= width || relative_y >= height {
            return None;
        }

        if relative_y < cell_h || relative_y >= height - cell_h {
            return None;
        }

        let index = ((relative_y - cell_h) / cell_h).floor() as usize;
        (index < items.len()).then_some(index)
    }
}

fn point_inside_rounded_rect(x: f64, y: f64, width: f64, height: f64, radius: f64) -> bool {
    let radius = radius.min(width * 0.5).min(height * 0.5);
    let corner_x = if x < radius {
        radius
    } else if x > width - radius {
        width - radius
    } else {
        x
    };
    let corner_y = if y < radius {
        radius
    } else if y > height - radius {
        height - radius
    } else {
        y
    };
    let dx = x - corner_x;
    let dy = y - corner_y;
    dx * dx + dy * dy <= radius * radius
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_tests_menu_items() {
        let menu = ContextMenuState::open(10.0, 20.0);

        assert_eq!(
            menu.action_at(20.0, 35.0, 800.0, 600.0, 10.0, 10.0),
            Some(ContextMenuAction::Copy)
        );
        assert_eq!(
            menu.action_at(20.0, 45.0, 800.0, 600.0, 10.0, 10.0),
            Some(ContextMenuAction::Paste)
        );
        assert_eq!(
            menu.action_at(20.0, 55.0, 800.0, 600.0, 10.0, 10.0),
            Some(ContextMenuAction::Split)
        );
        assert_eq!(
            menu.action_at(20.0, 65.0, 800.0, 600.0, 10.0, 10.0),
            Some(ContextMenuAction::ZoomPane)
        );
        assert_eq!(menu.action_at(200.0, 70.0, 800.0, 600.0, 10.0, 10.0), None);
    }

    #[test]
    fn menu_does_not_include_config_reload() {
        let menu = ContextMenuState::open(10.0, 20.0);
        let popup_background = [0.1, 0.2, 0.3, 0.9];
        let render = menu.render_data(
            800.0,
            600.0,
            10.0,
            10.0,
            popup_background,
        );

        assert!(render
            .items
            .iter()
            .all(|item| item.label != "Reload Configuration"));
        assert_eq!(render.background_color, popup_background);
    }

    #[test]
    fn split_submenu_opens_on_hover_and_returns_split_actions() {
        let mut menu = ContextMenuState::open(10.0, 20.0);

        assert!(menu.update_hover(20.0, 55.0, 800.0, 600.0, 10.0, 10.0));
        let render = menu.render_data(800.0, 600.0, 10.0, 10.0, [0.0; 4]);
        let submenu = render.submenu.expect("split submenu should be visible");

        assert_eq!(
            menu.action_at(
                submenu.x as f64 + 20.0,
                submenu.y as f64 + 15.0,
                800.0,
                600.0,
                10.0,
                10.0
            ),
            Some(ContextMenuAction::SplitHorizontal)
        );
        assert_eq!(
            menu.action_at(
                submenu.x as f64 + 20.0,
                submenu.y as f64 + 25.0,
                800.0,
                600.0,
                10.0,
                10.0
            ),
            Some(ContextMenuAction::SplitVertical)
        );
    }

    #[test]
    fn split_submenu_stays_open_until_another_main_item_is_hovered() {
        let mut menu = ContextMenuState::open(10.0, 20.0);

        assert!(menu.update_hover(20.0, 55.0, 800.0, 600.0, 10.0, 10.0));
        assert!(menu.render_data(800.0, 600.0, 10.0, 10.0, [0.0; 4]).submenu.is_some());

        assert!(menu.update_hover(260.0, 55.0, 800.0, 600.0, 10.0, 10.0));
        assert!(menu.render_data(800.0, 600.0, 10.0, 10.0, [0.0; 4]).submenu.is_some());

        assert!(menu.update_hover(20.0, 35.0, 800.0, 600.0, 10.0, 10.0));
        assert!(menu.render_data(800.0, 600.0, 10.0, 10.0, [0.0; 4]).submenu.is_none());
    }

    #[test]
    fn hit_testing_ignores_transparent_rounded_corners() {
        let menu = ContextMenuState::open(10.0, 20.0);

        assert_eq!(menu.action_at(11.0, 21.0, 800.0, 600.0, 10.0, 10.0), None);
        assert_eq!(
            menu.action_at(18.0, 35.0, 800.0, 600.0, 10.0, 10.0),
            Some(ContextMenuAction::Copy)
        );
    }

    #[test]
    fn clamps_to_window_edges() {
        let menu = ContextMenuState::open(780.0, 590.0);
        let render = menu.render_data(800.0, 600.0, 10.0, 10.0, [0.0; 4]);

        assert_eq!(render.x, 670.0);
        assert_eq!(render.y, 540.0);
    }

    #[test]
    fn provides_target_pane() {
        let pane_id = crate::mux::PaneId::new(42);
        let menu = ContextMenuState::open_for_pane(10.0, 20.0, Some(pane_id), true, false);
        assert_eq!(menu.target_pane(), Some(pane_id));
        assert_eq!(menu.target_pane_id(), Some(pane_id));
    }
}
