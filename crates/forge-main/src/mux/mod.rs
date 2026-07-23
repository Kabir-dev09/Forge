pub mod io;
pub mod layout;
pub mod pane;
pub mod runtime;
pub mod scrolling;
pub mod state;
pub mod tab;

pub use io::{
    PaneIoRegistry, PaneIoStatus, MAX_PTY_READ_BYTES_PER_EVENT, MAX_PTY_READ_ITERATIONS_PER_EVENT,
    PTY_READ_BUFFER_SIZE,
};
pub use layout::{
    compute_layout, LayoutError, LayoutParams, LayoutResult, PaneLayout, SplitBorder,
    DEFAULT_MIN_PANE_COLS, DEFAULT_MIN_PANE_ROWS, DEFAULT_SPLIT_BORDER_PX,
};
pub use pane::{GridSize, Pane, PaneId, PaneRect};
pub use runtime::{
    PaneRuntime, PaneRuntimeKind, ScrollingPaneTabMove, ScrollingTab, ScrollingTabManager,
};
pub use scrolling::{
    ScrollingOverflowIndicators, ScrollingPane, ScrollingPaneManager, ScrollingPanePointTarget,
    ScrollingResizeDrag, ScrollingResizeHandle, VirtualPaneRect, VisiblePaneCache,
    VisibleScrollingPane,
};
pub use state::{
    LayoutNode, MuxState, PaneLayoutChange, PanePointTarget, RelayoutError, RemovePaneResult,
    SplitAxis, SplitPaneError,
};
pub use tab::{Tab, TabId, TabManager};
