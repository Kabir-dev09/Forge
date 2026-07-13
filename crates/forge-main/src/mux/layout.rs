use super::{
    pane::{GridSize, PaneId, PaneRect},
    state::{LayoutNode, SplitAxis},
};

pub const DEFAULT_MIN_PANE_COLS: usize = 10;
pub const DEFAULT_MIN_PANE_ROWS: usize = 3;
pub const DEFAULT_SPLIT_BORDER_PX: f32 = 1.0;

const MIN_SPLIT_RATIO: f32 = 0.10;
const MAX_SPLIT_RATIO: f32 = 0.90;

#[derive(Debug, Clone, Copy)]
pub struct LayoutParams {
    pub content_rect: PaneRect,
    pub cell_width: f32,
    pub cell_height: f32,
    pub split_border_px: f32,
    pub pane_padding: forge_core::config_registry::PaddingConfig,
    pub min_cols: usize,
    pub min_rows: usize,
}

impl LayoutParams {
    pub fn new(
        content_rect: PaneRect,
        cell_width: f32,
        cell_height: f32,
        split_border_px: f32,
        pane_padding: forge_core::config_registry::PaddingConfig,
    ) -> Self {
        Self {
            content_rect,
            cell_width,
            cell_height,
            split_border_px,
            pane_padding,
            min_cols: DEFAULT_MIN_PANE_COLS,
            min_rows: DEFAULT_MIN_PANE_ROWS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneLayout {
    pub pane_id: PaneId,
    pub rect: PaneRect,
    pub grid_size: GridSize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SplitBorder {
    pub axis: SplitAxis,
    pub rect: PaneRect,
    /// Path to the split node in the layout tree. false = first, true = second.
    pub path: Vec<bool>,
    /// The bounding rect of the entire split (parent rect).
    pub parent_rect: PaneRect,
    /// The current ratio of this split (0.0 to 1.0).
    pub current_ratio: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutResult {
    pub panes: Vec<PaneLayout>,
    pub borders: Vec<SplitBorder>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LayoutError {
    InvalidContentRect(PaneRect),
    InvalidCellMetrics {
        cell_width: f32,
        cell_height: f32,
    },
    InvalidBorderWidth(f32),
    PaneBelowMinimum {
        pane_id: PaneId,
        grid_size: GridSize,
        min_cols: usize,
        min_rows: usize,
    },
    SplitBelowMinimum {
        axis: SplitAxis,
        rect: PaneRect,
        split_border_px: f32,
    },
}

pub fn compute_layout(
    root: &LayoutNode,
    params: LayoutParams,
) -> Result<LayoutResult, LayoutError> {
    validate_params(params)?;

    let mut result = LayoutResult {
        panes: Vec::new(),
        borders: Vec::new(),
    };
    assign_node(
        root,
        params.content_rect.snapped(),
        params,
        &mut Vec::new(),
        &mut result,
    )?;
    Ok(result)
}

fn validate_params(params: LayoutParams) -> Result<(), LayoutError> {
    if !params.content_rect.has_positive_area() {
        return Err(LayoutError::InvalidContentRect(params.content_rect));
    }
    if params.cell_width <= 0.0 || params.cell_height <= 0.0 {
        return Err(LayoutError::InvalidCellMetrics {
            cell_width: params.cell_width,
            cell_height: params.cell_height,
        });
    }
    if params.split_border_px < 0.0 {
        return Err(LayoutError::InvalidBorderWidth(params.split_border_px));
    }
    Ok(())
}

fn assign_node(
    node: &LayoutNode,
    rect: PaneRect,
    params: LayoutParams,
    current_path: &mut Vec<bool>,
    result: &mut LayoutResult,
) -> Result<(), LayoutError> {
    match node {
        LayoutNode::Leaf(pane_id) => assign_leaf(*pane_id, rect, params, result),
        LayoutNode::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let (first_rect, mut border, second_rect) = split_rect(rect, *axis, *ratio, params)?;
            border.path = current_path.clone();

            current_path.push(false);
            assign_node(first, first_rect, params, current_path, result)?;
            current_path.pop();

            result.borders.push(border);

            current_path.push(true);
            assign_node(second, second_rect, params, current_path, result)?;
            current_path.pop();
            Ok(())
        }
    }
}

fn assign_leaf(
    pane_id: PaneId,
    rect: PaneRect,
    params: LayoutParams,
    result: &mut LayoutResult,
) -> Result<(), LayoutError> {
    let inner_width =
        (rect.width - params.pane_padding.left as f32 - params.pane_padding.right as f32).max(0.0);
    let inner_height =
        (rect.height - params.pane_padding.top as f32 - params.pane_padding.bottom as f32).max(0.0);

    let grid_size = GridSize {
        cols: (inner_width / params.cell_width).floor().max(0.0) as usize,
        rows: (inner_height / params.cell_height).floor().max(0.0) as usize,
    };

    if grid_size.cols < params.min_cols || grid_size.rows < params.min_rows {
        return Err(LayoutError::PaneBelowMinimum {
            pane_id,
            grid_size,
            min_cols: params.min_cols,
            min_rows: params.min_rows,
        });
    }

    result.panes.push(PaneLayout {
        pane_id,
        rect,
        grid_size,
    });
    Ok(())
}

fn split_rect(
    rect: PaneRect,
    axis: SplitAxis,
    ratio: f32,
    params: LayoutParams,
) -> Result<(PaneRect, SplitBorder, PaneRect), LayoutError> {
    let border_px = params.split_border_px.round().max(0.0);
    match axis {
        SplitAxis::Vertical => {
            if rect.width <= border_px {
                return Err(LayoutError::SplitBelowMinimum {
                    axis,
                    rect,
                    split_border_px: border_px,
                });
            }

            let available_width = rect.width - border_px;
            let first_width = (available_width * clamp_ratio(ratio)).round();
            let border_x = rect.x + first_width;
            let second_x = border_x + border_px;

            let first = PaneRect::new(rect.x, rect.y, first_width, rect.height).snapped();
            let border = SplitBorder {
                axis,
                rect: PaneRect::new(border_x, rect.y, border_px, rect.height).snapped(),
                path: vec![], // populated by caller
                parent_rect: rect,
                current_ratio: ratio,
            };
            let second =
                PaneRect::new(second_x, rect.y, rect.right() - second_x, rect.height).snapped();

            Ok((first, border, second))
        }
        SplitAxis::Horizontal => {
            if rect.height <= border_px {
                return Err(LayoutError::SplitBelowMinimum {
                    axis,
                    rect,
                    split_border_px: border_px,
                });
            }

            let available_height = rect.height - border_px;
            let first_height = (available_height * clamp_ratio(ratio)).round();
            let border_y = rect.y + first_height;
            let second_y = border_y + border_px;

            let first = PaneRect::new(rect.x, rect.y, rect.width, first_height).snapped();
            let border = SplitBorder {
                axis,
                rect: PaneRect::new(rect.x, border_y, rect.width, border_px).snapped(),
                path: vec![], // populated by caller
                parent_rect: rect,
                current_ratio: ratio,
            };
            let second =
                PaneRect::new(rect.x, second_y, rect.width, rect.bottom() - second_y).snapped();

            Ok((first, border, second))
        }
    }
}

fn clamp_ratio(ratio: f32) -> f32 {
    if ratio.is_finite() {
        ratio.clamp(MIN_SPLIT_RATIO, MAX_SPLIT_RATIO)
    } else {
        0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(id: u64) -> LayoutNode {
        LayoutNode::leaf(PaneId::new(id))
    }

    fn params(width: f32, height: f32) -> LayoutParams {
        LayoutParams::new(
            PaneRect::new(0.0, 0.0, width, height),
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
    fn single_pane_fills_content_rect() {
        let root = pane(1);
        let result = compute_layout(&root, params(800.0, 600.0)).unwrap();

        assert_eq!(result.borders, vec![]);
        assert_eq!(result.panes.len(), 1);
        assert_eq!(result.panes[0].pane_id, PaneId::new(1));
        assert_eq!(result.panes[0].rect, PaneRect::new(0.0, 0.0, 800.0, 600.0));
        assert_eq!(result.panes[0].grid_size, GridSize::new(80, 30));
    }

    #[test]
    fn vertical_split_produces_left_and_right_panes_with_border() {
        let root = LayoutNode::split(SplitAxis::Vertical, 0.5, pane(1), pane(2));
        let result = compute_layout(&root, params(801.0, 600.0)).unwrap();

        assert_eq!(result.panes.len(), 2);
        assert_eq!(result.borders.len(), 1);
        assert_eq!(result.panes[0].rect, PaneRect::new(0.0, 0.0, 400.0, 600.0));
        assert_eq!(
            result.borders[0].rect,
            PaneRect::new(400.0, 0.0, 1.0, 600.0)
        );
        assert_eq!(
            result.panes[1].rect,
            PaneRect::new(401.0, 0.0, 400.0, 600.0)
        );
        assert_eq!(result.panes[0].grid_size, GridSize::new(40, 30));
        assert_eq!(result.panes[1].grid_size, GridSize::new(40, 30));
    }

    #[test]
    fn horizontal_split_produces_top_and_bottom_panes_with_border() {
        let root = LayoutNode::split(SplitAxis::Horizontal, 0.5, pane(1), pane(2));
        let result = compute_layout(&root, params(800.0, 601.0)).unwrap();

        assert_eq!(result.panes.len(), 2);
        assert_eq!(result.borders.len(), 1);
        assert_eq!(result.panes[0].rect, PaneRect::new(0.0, 0.0, 800.0, 300.0));
        assert_eq!(
            result.borders[0].rect,
            PaneRect::new(0.0, 300.0, 800.0, 1.0)
        );
        assert_eq!(
            result.panes[1].rect,
            PaneRect::new(0.0, 301.0, 800.0, 300.0)
        );
        assert_eq!(result.panes[0].grid_size, GridSize::new(80, 15));
        assert_eq!(result.panes[1].grid_size, GridSize::new(80, 15));
    }

    #[test]
    fn nested_splits_are_deterministic_and_non_overlapping() {
        let root = LayoutNode::split(
            SplitAxis::Vertical,
            0.5,
            pane(1),
            LayoutNode::split(SplitAxis::Horizontal, 0.5, pane(2), pane(3)),
        );
        let result = compute_layout(&root, params(1001.0, 801.0)).unwrap();

        assert_eq!(result.panes.len(), 3);
        assert_eq!(result.borders.len(), 2);
        assert_eq!(result.panes[0].rect, PaneRect::new(0.0, 0.0, 500.0, 801.0));
        assert_eq!(
            result.panes[1].rect,
            PaneRect::new(501.0, 0.0, 500.0, 400.0)
        );
        assert_eq!(
            result.panes[2].rect,
            PaneRect::new(501.0, 401.0, 500.0, 400.0)
        );
    }

    #[test]
    fn ratios_are_clamped_without_mutating_tree() {
        let root = LayoutNode::split(SplitAxis::Vertical, 0.0, pane(1), pane(2));
        let result = compute_layout(&root, params(1001.0, 600.0)).unwrap();

        assert_eq!(result.panes[0].rect.width, 100.0);
        assert_eq!(result.panes[1].rect.width, 900.0);
    }

    #[test]
    fn non_finite_ratio_falls_back_to_half() {
        let root = LayoutNode::split(SplitAxis::Vertical, f32::NAN, pane(1), pane(2));
        let result = compute_layout(&root, params(801.0, 600.0)).unwrap();

        assert_eq!(result.panes[0].rect.width, 400.0);
        assert_eq!(result.panes[1].rect.width, 400.0);
    }

    #[test]
    fn pane_below_minimum_is_rejected() {
        let root = pane(1);
        let err = compute_layout(&root, params(90.0, 60.0)).unwrap_err();

        assert_eq!(
            err,
            LayoutError::PaneBelowMinimum {
                pane_id: PaneId::new(1),
                grid_size: GridSize::new(9, 3),
                min_cols: DEFAULT_MIN_PANE_COLS,
                min_rows: DEFAULT_MIN_PANE_ROWS,
            }
        );
    }

    #[test]
    fn split_that_creates_too_small_child_is_rejected() {
        let root = LayoutNode::split(SplitAxis::Vertical, 0.5, pane(1), pane(2));
        let err = compute_layout(&root, params(199.0, 100.0)).unwrap_err();

        assert!(matches!(
            err,
            LayoutError::PaneBelowMinimum {
                pane_id,
                grid_size: GridSize { cols: 9, rows: 5 },
                ..
            } if pane_id == PaneId::new(1)
        ));
    }

    #[test]
    fn replace_leaf_swaps_only_matching_pane() {
        let mut root = LayoutNode::split(SplitAxis::Vertical, 0.5, pane(1), pane(2));
        let replacement = LayoutNode::split(SplitAxis::Horizontal, 0.5, pane(2), pane(3));

        assert!(root.replace_leaf(PaneId::new(2), replacement));
        assert!(root.contains_pane(PaneId::new(1)));
        assert!(root.contains_pane(PaneId::new(2)));
        assert!(root.contains_pane(PaneId::new(3)));
        assert!(!root.contains_pane(PaneId::new(4)));
    }
}
