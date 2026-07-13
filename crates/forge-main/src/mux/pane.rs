#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PaneId(u64);

impl PaneId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl PaneRect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(self) -> f32 {
        self.y + self.height
    }

    pub fn has_positive_area(self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }

    pub fn contains_point(self, x: f32, y: f32) -> bool {
        self.has_positive_area()
            && x >= self.x
            && y >= self.y
            && x < self.right()
            && y < self.bottom()
    }

    pub fn local_point(self, x: f32, y: f32) -> (f32, f32) {
        (x - self.x, y - self.y)
    }

    pub fn snapped(self) -> Self {
        let x = self.x.round();
        let y = self.y.round();
        let right = self.right().round();
        let bottom = self.bottom().round();
        Self {
            x,
            y,
            width: (right - x).max(0.0),
            height: (bottom - y).max(0.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    pub cols: usize,
    pub rows: usize,
}

impl GridSize {
    pub const fn new(cols: usize, rows: usize) -> Self {
        Self { cols, rows }
    }
}

pub struct Pane {
    pub id: PaneId,
    pub pty: Option<forge_pty::Pty>,
    pub snapshot: std::sync::Arc<arc_swap::ArcSwap<forge_pty::snapshot::RenderSnapshot>>,
    pub rect: PaneRect,
    pub grid_size: GridSize,
    pub dirty_layout: bool,
}

impl Pane {
    pub fn new(
        id: PaneId,
        pty: forge_pty::Pty,
        snapshot: std::sync::Arc<arc_swap::ArcSwap<forge_pty::snapshot::RenderSnapshot>>,
        grid_size: GridSize,
    ) -> Self {
        Self {
            id,
            pty: Some(pty),
            snapshot,
            rect: PaneRect::new(0.0, 0.0, 0.0, 0.0),
            grid_size,
            dirty_layout: false,
        }
    }

    #[cfg(test)]
    pub fn layout_only(id: PaneId, grid_size: GridSize) -> Self {
        let snapshot = std::sync::Arc::new(arc_swap::ArcSwap::from_pointee(
            forge_pty::snapshot::RenderSnapshot::empty(
                grid_size.cols.max(1),
                grid_size.rows.max(1),
            ),
        ));

        Self {
            id,
            pty: None,
            snapshot,
            rect: PaneRect::new(0.0, 0.0, 0.0, 0.0),
            grid_size,
            dirty_layout: false,
        }
    }
}
