use crate::renderer::{PaneRenderId, PaneRenderLayer, PaneRenderRect};
use forge_core::config_registry::{CursorStyle, CursorTrailConfig};
use std::time::{Duration, Instant};

const CORNER_X: [usize; 4] = [0, 1, 1, 0];
const CORNER_Y: [usize; 4] = [0, 0, 1, 1];
const CONVERGENCE_PX: f32 = 0.5;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct TrailPoint {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CursorTrailSample {
    pub pane_id: PaneRenderId,
    pub pane_rect: PaneRenderRect,
    pub layer: PaneRenderLayer,
    pub origin_x: f32,
    pub origin_y: f32,
    pub cell_width: f32,
    pub cell_height: f32,
    pub cursor_col: usize,
    pub cursor_row: usize,
    pub cursor_style: CursorStyle,
    pub cursor_color: [f32; 4],
    pub pane_opacity: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CursorTrailContext {
    pane_id: PaneRenderId,
    pane_rect: PaneRenderRect,
    layer: PaneRenderLayer,
    origin_x: f32,
    origin_y: f32,
    cell_width: f32,
    cell_height: f32,
    cursor_style: CursorStyle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PendingTarget {
    rect: PaneRenderRect,
    ready_at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CursorTrailVisual {
    pub pane_id: PaneRenderId,
    pub corners: [TrailPoint; 4],
    pub cursor_rect: PaneRenderRect,
    pub pane_rect: PaneRenderRect,
    pub layer: PaneRenderLayer,
    pub color: [f32; 4],
    pub cell_width: f32,
    pub cell_height: f32,
    pub segmented: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct CursorTrail {
    fast_decay: Duration,
    slow_decay: Duration,
    minimum_distance_x: u32,
    minimum_distance_y: u32,
    trigger_delay: Duration,
    color: Option<[f32; 4]>,
    segmented: bool,
    context: Option<CursorTrailContext>,
    corners: [TrailPoint; 4],
    target: PaneRenderRect,
    observed_rect: PaneRenderRect,
    observed_since: Instant,
    pending: Option<PendingTarget>,
    updated_at: Instant,
    active: bool,
}

impl CursorTrail {
    pub(crate) fn new(config: &CursorTrailConfig, now: Instant) -> Option<Self> {
        config.enabled.then(|| Self {
            fast_decay: Duration::from_millis(config.fast_decay_ms as u64),
            slow_decay: Duration::from_millis(config.slow_decay_ms as u64),
            minimum_distance_x: config.minimum_distance_x,
            minimum_distance_y: config.minimum_distance_y,
            trigger_delay: Duration::from_millis(config.trigger_delay_ms as u64),
            color: config.parsed_color.map(|color| {
                let color = color.to_srgb_linear();
                [color.r, color.g, color.b, color.a]
            }),
            segmented: config.segmented,
            context: None,
            corners: [TrailPoint::default(); 4],
            target: PaneRenderRect::default(),
            observed_rect: PaneRenderRect::default(),
            observed_since: now,
            pending: None,
            updated_at: now,
            active: false,
        })
    }

    pub(crate) fn reset(&mut self) {
        self.context = None;
        self.pending = None;
        self.active = false;
    }

    pub(crate) fn wants_redraw(&self, now: Instant) -> bool {
        self.active || self.pending.is_some_and(|pending| now >= pending.ready_at)
    }

    pub(crate) fn next_wakeup(&self, now: Instant) -> Option<Duration> {
        self.pending
            .and_then(|pending| pending.ready_at.checked_duration_since(now))
    }

    pub(crate) fn update(
        &mut self,
        sample: Option<CursorTrailSample>,
        now: Instant,
    ) -> Option<CursorTrailVisual> {
        let Some(sample) = sample else {
            self.reset();
            return None;
        };
        let context = sample.context();
        let cursor_rect = sample.cursor_rect();

        if self.context != Some(context) {
            self.context = Some(context);
            self.target = cursor_rect;
            self.observed_rect = cursor_rect;
            self.observed_since = now;
            self.snap_to_target();
            self.pending = None;
            self.active = false;
            self.updated_at = now;
            return None;
        }

        self.advance(now);

        let source_was_stable = cursor_rect != self.observed_rect
            && now.saturating_duration_since(self.observed_since) >= self.trigger_delay;
        if cursor_rect != self.observed_rect {
            self.observed_rect = cursor_rect;
            self.observed_since = now;
        }

        if cursor_rect != self.target {
            if self.active
                || (source_was_stable && self.exceeds_start_threshold(cursor_rect, sample))
            {
                self.target = cursor_rect;
                self.pending = None;
                self.active = true;
                self.updated_at = now;
            } else if self
                .pending
                .is_none_or(|pending| pending.rect != cursor_rect)
            {
                self.pending = Some(PendingTarget {
                    rect: cursor_rect,
                    ready_at: now + self.trigger_delay,
                });
            }
        } else {
            self.pending = None;
        }

        if let Some(pending) = self.pending.filter(|pending| now >= pending.ready_at) {
            self.pending = None;
            if self.active || self.exceeds_start_threshold(pending.rect, sample) {
                let was_active = self.active;
                self.target = pending.rect;
                self.active = true;
                if !was_active {
                    // Stability filtering is not animation time. Start from the
                    // previous stable cursor so the first visible frame retains
                    // the complete trail geometry.
                    self.updated_at = now;
                }
            } else {
                self.target = pending.rect;
                self.snap_to_target();
                self.active = false;
            }
        }

        self.updated_at = now;
        if !self.active {
            return None;
        }

        let mut color = self.color.unwrap_or(sample.cursor_color);
        color[3] *= sample.pane_opacity;
        Some(CursorTrailVisual {
            pane_id: sample.pane_id,
            corners: self.corners,
            cursor_rect: self.target,
            pane_rect: sample.pane_rect,
            layer: sample.layer,
            color,
            cell_width: sample.cell_width,
            cell_height: sample.cell_height,
            segmented: self.segmented,
        })
    }

    fn advance(&mut self, now: Instant) {
        if !self.active || now <= self.updated_at {
            return;
        }
        let elapsed = now.duration_since(self.updated_at).as_secs_f32();
        let center_x = self.target.x + self.target.width * 0.5;
        let center_y = self.target.y + self.target.height * 0.5;
        let half_diagonal = (self.target.width.hypot(self.target.height) * 0.5).max(f32::EPSILON);
        let mut delta = [TrailPoint::default(); 4];
        let mut dots = [0.0; 4];
        let mut min_dot = f32::MAX;
        let mut max_dot = -f32::MAX;

        for index in 0..4 {
            let target = target_corner(self.target, index);
            let dx = target.x - self.corners[index].x;
            let dy = target.y - self.corners[index].y;
            delta[index] = TrailPoint { x: dx, y: dy };
            let distance = dx.hypot(dy);
            let dot = if distance <= 1e-6 {
                0.0
            } else {
                (dx * (target.x - center_x) + dy * (target.y - center_y))
                    / (half_diagonal * distance)
            };
            dots[index] = dot;
            min_dot = min_dot.min(dot);
            max_dot = max_dot.max(dot);
        }

        let fast = self.fast_decay.as_secs_f32();
        let slow = self.slow_decay.as_secs_f32();
        for index in 0..4 {
            let d = delta[index];
            if d.x == 0.0 && d.y == 0.0 {
                continue;
            }
            let decay = if (max_dot - min_dot).abs() <= f32::EPSILON {
                slow
            } else {
                slow + (fast - slow) * (dots[index] - min_dot) / (max_dot - min_dot)
            };
            let step = 1.0 - (-10.0 * elapsed / decay.max(f32::EPSILON)).exp2();
            self.corners[index].x += d.x * step;
            self.corners[index].y += d.y * step;
        }

        self.active = self.corners.iter().enumerate().any(|(index, corner)| {
            let target = target_corner(self.target, index);
            (target.x - corner.x).abs() >= CONVERGENCE_PX
                || (target.y - corner.y).abs() >= CONVERGENCE_PX
        });
        if !self.active {
            self.snap_to_target();
        }
    }

    fn exceeds_start_threshold(&self, rect: PaneRenderRect, sample: CursorTrailSample) -> bool {
        let dx = ((rect.x - self.target.x) / sample.cell_width.max(1.0)).round() as i32;
        let dy = ((rect.y - self.target.y) / sample.cell_height.max(1.0)).round() as i32;
        (dx != 0 && dx.unsigned_abs() >= self.minimum_distance_x)
            || (dy != 0 && dy.unsigned_abs() >= self.minimum_distance_y)
    }

    fn snap_to_target(&mut self) {
        for index in 0..4 {
            self.corners[index] = target_corner(self.target, index);
        }
    }
}

impl CursorTrailSample {
    fn context(self) -> CursorTrailContext {
        CursorTrailContext {
            pane_id: self.pane_id,
            pane_rect: self.pane_rect,
            layer: self.layer,
            origin_x: self.origin_x,
            origin_y: self.origin_y,
            cell_width: self.cell_width,
            cell_height: self.cell_height,
            cursor_style: self.cursor_style,
        }
    }

    fn cursor_rect(self) -> PaneRenderRect {
        let x = self.origin_x + self.cursor_col as f32 * self.cell_width;
        let y = self.origin_y + self.cursor_row as f32 * self.cell_height;
        match self.cursor_style {
            CursorStyle::Block => PaneRenderRect::new(x, y, self.cell_width, self.cell_height),
            CursorStyle::Beam => PaneRenderRect::new(x, y, 1.0, self.cell_height),
            CursorStyle::Underline => PaneRenderRect::new(
                x,
                y + (self.cell_height - 2.0).max(0.0),
                self.cell_width,
                2.0,
            ),
        }
    }
}

fn target_corner(rect: PaneRenderRect, index: usize) -> TrailPoint {
    let xs = [rect.x, rect.x + rect.width];
    let ys = [rect.y, rect.y + rect.height];
    TrailPoint {
        x: xs[CORNER_X[index]],
        y: ys[CORNER_Y[index]],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> CursorTrailConfig {
        CursorTrailConfig {
            enabled: true,
            trigger_delay_ms: 0,
            minimum_distance_x: 0,
            minimum_distance_y: 0,
            ..CursorTrailConfig::default()
        }
    }

    fn sample(col: usize, row: usize, style: CursorStyle) -> CursorTrailSample {
        CursorTrailSample {
            pane_id: PaneRenderId(1),
            pane_rect: PaneRenderRect::new(0.0, 0.0, 500.0, 400.0),
            layer: PaneRenderLayer::Normal,
            origin_x: 4.0,
            origin_y: 6.0,
            cell_width: 10.0,
            cell_height: 20.0,
            cursor_col: col,
            cursor_row: row,
            cursor_style: style,
            cursor_color: [1.0; 4],
            pane_opacity: 1.0,
        }
    }

    #[test]
    fn cursor_shapes_use_authoritative_renderer_dimensions() {
        assert_eq!(
            sample(2, 3, CursorStyle::Block).cursor_rect(),
            PaneRenderRect::new(24.0, 66.0, 10.0, 20.0)
        );
        assert_eq!(
            sample(2, 3, CursorStyle::Beam).cursor_rect(),
            PaneRenderRect::new(24.0, 66.0, 1.0, 20.0)
        );
        assert_eq!(
            sample(2, 3, CursorStyle::Underline).cursor_rect(),
            PaneRenderRect::new(24.0, 84.0, 10.0, 2.0)
        );
    }

    #[test]
    fn logical_target_changes_immediately_and_corners_decay_independently() {
        let start = Instant::now();
        let mut trail = CursorTrail::new(&config(), start).unwrap();
        assert!(trail
            .update(Some(sample(0, 0, CursorStyle::Block)), start)
            .is_none());

        let visual = trail
            .update(Some(sample(8, 2, CursorStyle::Block)), start)
            .expect("movement should start a trail");
        assert_eq!(
            visual.cursor_rect,
            PaneRenderRect::new(84.0, 46.0, 10.0, 20.0)
        );
        assert_ne!(visual.corners[0], target_corner(visual.cursor_rect, 0));

        let visual = trail
            .update(
                Some(sample(8, 2, CursorStyle::Block)),
                start + Duration::from_millis(40),
            )
            .unwrap();
        assert_ne!(
            visual.corners[0].x,
            visual.corners[1].x - visual.cursor_rect.width
        );
    }

    #[test]
    fn animation_converges_without_real_sleep() {
        let start = Instant::now();
        let mut trail = CursorTrail::new(&config(), start).unwrap();
        trail.update(Some(sample(0, 0, CursorStyle::Block)), start);
        trail.update(Some(sample(20, 10, CursorStyle::Block)), start);

        assert!(trail
            .update(
                Some(sample(20, 10, CursorStyle::Block)),
                start + Duration::from_secs(2)
            )
            .is_none());
        assert!(!trail.active);
    }

    #[test]
    fn pending_target_uses_exact_stability_deadline() {
        let start = Instant::now();
        let mut cfg = config();
        cfg.trigger_delay_ms = 50;
        let mut trail = CursorTrail::new(&cfg, start).unwrap();
        trail.update(Some(sample(0, 0, CursorStyle::Block)), start);
        assert!(trail
            .update(Some(sample(5, 0, CursorStyle::Block)), start)
            .is_none());
        assert_eq!(trail.next_wakeup(start), Some(Duration::from_millis(50)));
        assert!(!trail.wants_redraw(start + Duration::from_millis(49)));
        assert!(trail.wants_redraw(start + Duration::from_millis(50)));
    }

    #[test]
    fn movement_from_stable_source_starts_without_destination_delay() {
        let start = Instant::now();
        let mut cfg = config();
        cfg.trigger_delay_ms = 20;
        let mut trail = CursorTrail::new(&cfg, start).unwrap();
        let source = sample(0, 0, CursorStyle::Block);
        let destination = sample(8, 0, CursorStyle::Block);
        trail.update(Some(source), start);

        let visual = trail
            .update(Some(destination), start + Duration::from_millis(100))
            .expect("movement from an idle cursor should start immediately");

        let source_corners =
            std::array::from_fn(|index| target_corner(source.cursor_rect(), index));
        assert_eq!(visual.corners, source_corners);
        assert_eq!(visual.cursor_rect, destination.cursor_rect());
        assert!(trail.pending.is_none());
    }

    #[test]
    fn rapidly_changing_source_still_waits_for_destination_stability() {
        let start = Instant::now();
        let mut cfg = config();
        cfg.trigger_delay_ms = 20;
        let mut trail = CursorTrail::new(&cfg, start).unwrap();
        trail.update(Some(sample(0, 0, CursorStyle::Block)), start);

        assert!(trail
            .update(
                Some(sample(4, 0, CursorStyle::Block)),
                start + Duration::from_millis(5)
            )
            .is_none());
        assert!(trail
            .update(
                Some(sample(8, 0, CursorStyle::Block)),
                start + Duration::from_millis(10)
            )
            .is_none());
        assert_eq!(
            trail.next_wakeup(start + Duration::from_millis(10)),
            Some(Duration::from_millis(20))
        );
    }

    #[test]
    fn trigger_delay_does_not_consume_animation_progress() {
        let start = Instant::now();
        let mut cfg = config();
        cfg.trigger_delay_ms = 50;
        let mut trail = CursorTrail::new(&cfg, start).unwrap();
        let source = sample(0, 0, CursorStyle::Block);
        let destination = sample(8, 0, CursorStyle::Block);
        trail.update(Some(source), start);
        trail.update(Some(destination), start);

        let visual = trail
            .update(Some(destination), start + Duration::from_millis(50))
            .expect("the stable movement should produce a trail");

        let source_corners =
            std::array::from_fn(|index| target_corner(source.cursor_rect(), index));
        let target_corners =
            std::array::from_fn(|index| target_corner(destination.cursor_rect(), index));
        assert_eq!(visual.corners, source_corners);
        assert_ne!(visual.corners, target_corners);
    }

    #[test]
    fn redirected_movement_continues_from_current_visual_corners() {
        let start = Instant::now();
        let mut trail = CursorTrail::new(&config(), start).unwrap();
        trail.update(Some(sample(0, 0, CursorStyle::Block)), start);
        trail.update(Some(sample(10, 0, CursorStyle::Block)), start);
        let before_redirect = trail
            .update(
                Some(sample(10, 0, CursorStyle::Block)),
                start + Duration::from_millis(30),
            )
            .unwrap()
            .corners;

        let redirected = trail
            .update(
                Some(sample(4, 8, CursorStyle::Block)),
                start + Duration::from_millis(30),
            )
            .unwrap();

        assert_eq!(redirected.corners, before_redirect);
        assert_eq!(
            redirected.cursor_rect,
            sample(4, 8, CursorStyle::Block).cursor_rect()
        );
    }

    #[test]
    fn sub_threshold_motion_snaps_without_scheduling_frames() {
        let start = Instant::now();
        let mut cfg = config();
        cfg.minimum_distance_x = 2;
        cfg.minimum_distance_y = 2;
        let mut trail = CursorTrail::new(&cfg, start).unwrap();
        trail.update(Some(sample(0, 0, CursorStyle::Block)), start);

        assert!(trail
            .update(Some(sample(1, 1, CursorStyle::Block)), start)
            .is_none());
        assert!(!trail.wants_redraw(start));
    }

    #[test]
    fn movement_at_configured_threshold_starts_animation() {
        let start = Instant::now();
        let mut cfg = config();
        cfg.minimum_distance_x = 1;
        cfg.minimum_distance_y = 1;
        let mut trail = CursorTrail::new(&cfg, start).unwrap();
        trail.update(Some(sample(0, 0, CursorStyle::Block)), start);

        let visual = trail
            .update(Some(sample(1, 0, CursorStyle::Block)), start)
            .expect("one-cell movement should meet a one-cell threshold");
        assert_eq!(
            visual.cursor_rect,
            sample(1, 0, CursorStyle::Block).cursor_rect()
        );
        assert!(trail.wants_redraw(start));
    }

    #[test]
    fn context_change_cancels_stale_trail() {
        let start = Instant::now();
        let mut trail = CursorTrail::new(&config(), start).unwrap();
        trail.update(Some(sample(0, 0, CursorStyle::Block)), start);
        trail.update(Some(sample(10, 0, CursorStyle::Block)), start);
        let mut moved_pane = sample(10, 0, CursorStyle::Block);
        moved_pane.pane_rect.x = 20.0;
        moved_pane.origin_x = 24.0;

        assert!(trail
            .update(Some(moved_pane), start + Duration::from_millis(10))
            .is_none());
        assert!(!trail.active);
    }

    #[test]
    fn disabled_config_constructs_no_runtime_state() {
        assert!(CursorTrail::new(&CursorTrailConfig::default(), Instant::now()).is_none());
    }

    #[test]
    fn segmented_mode_changes_only_visual_metadata() {
        let start = Instant::now();
        let mut cfg = config();
        cfg.segmented = true;
        let mut trail = CursorTrail::new(&cfg, start).unwrap();
        trail.update(Some(sample(0, 0, CursorStyle::Block)), start);

        let visual = trail
            .update(Some(sample(8, 0, CursorStyle::Block)), start)
            .expect("movement should start a trail");

        assert!(visual.segmented);
        assert_eq!(visual.cell_width, 10.0);
        assert_eq!(visual.cell_height, 20.0);
        assert_eq!(visual.cursor_rect, sample(8, 0, CursorStyle::Block).cursor_rect());
    }
}
