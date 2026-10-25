//! Paint-only caret motion. Document positions and IME geometry never interpolate.
use gpui::{fill, point, px, Bounds, Pixels, Point, Rgba, Window};
use lst_editor::TabId;
use std::time::Instant;

#[derive(Default)]
pub(crate) struct CursorMotion {
    context: Option<MotionContext>,
    motion: Option<Motion>,
}

#[derive(Clone, PartialEq)]
pub(crate) struct MotionContext {
    pub(crate) tab: TabId,
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) scroll: Point<Pixels>,
    pub(crate) scale: f32,
    pub(crate) row_height: Pixels,
    pub(crate) font: gpui::Font,
    pub(crate) code_pad: Pixels,
    pub(crate) wrap_columns: Option<usize>,
}

// One spring moves the entire caret, preserving its shape in every direction.
struct Motion {
    spring: Spring,
    target: Bounds<Pixels>,
    updated: Instant,
    response: f32,
}

#[derive(Clone, Copy)]
struct Spring {
    position: Point<Pixels>,
    velocity: Point<Pixels>,
}

impl Spring {
    fn new(position: Point<Pixels>) -> Self {
        Self {
            position,
            velocity: point(px(0.0), px(0.0)),
        }
    }

    fn advance(&mut self, target: Point<Pixels>, dt: f32, response: f32) {
        // Exact critically damped solution: stable at any refresh rate, with
        // position and velocity preserved when input changes the destination.
        let omega = 6.0 / response;
        let error = self.position - target;
        let b = self.velocity + error * omega;
        let decay = (-omega * dt).exp();
        self.position = target + (error + b * dt) * decay;
        self.velocity = (self.velocity - b * (omega * dt)) * decay;
    }

    fn settled(&self, target: Point<Pixels>) -> bool {
        length(self.position - target) < 0.05 && length(self.velocity) < 1.0
    }
}

fn length(point: Point<Pixels>) -> f32 {
    f32::from(point.x).hypot(f32::from(point.y))
}

impl Motion {
    fn new(target: Bounds<Pixels>, now: Instant) -> Self {
        Self {
            spring: Spring::new(target.origin),
            target,
            updated: now,
            response: 0.110,
        }
    }

    fn advance(&mut self, now: Instant) {
        let dt = now.duration_since(self.updated).as_secs_f32();
        self.spring.advance(self.target.origin, dt, self.response * 0.85);
        self.updated = now;
        if !self.active() {
            self.spring = Spring::new(self.target.origin);
        }
    }

    fn retarget(&mut self, target: Bounds<Pixels>, now: Instant) {
        self.advance(now);
        // Width follows the current glyph immediately. Small shaping
        // differences must not discard an ongoing positional spring.
        self.target.size = target.size;
        if self.target.origin != target.origin {
            let rows = length(target.origin - self.target.origin) / f32::from(target.size.height).max(1.0);
            // Continuous distance response: a visible but brisk single-step
            // movement, and a little more tracking time for longer travel.
            self.response = 0.105 + 0.055 * (1.0 - (-rows / 3.0).exp());
            self.target = target;
        }
    }

    fn active(&self) -> bool {
        !self.spring.settled(self.target.origin)
    }

    fn translate(&mut self, delta: Point<Pixels>) {
        self.spring.position += delta;
        self.target.origin += delta;
    }

    fn frame(&self) -> Bounds<Pixels> {
        Bounds::new(self.spring.position, self.target.size)
    }
}

impl CursorMotion {
    pub(crate) fn reset(&mut self) {
        self.motion = None;
    }

    pub(crate) fn prepare(&mut self, context: MotionContext) {
        if let Some(previous) = self.context.as_ref() {
            if previous.tab == context.tab
                && previous.bounds == context.bounds
                && previous.scale == context.scale
                && previous.row_height == context.row_height
                && previous.font == context.font
                && previous.code_pad == context.code_pad
                && previous.wrap_columns == context.wrap_columns
            {
                if let Some(motion) = self.motion.as_mut() {
                    // Keep motion in document space when cursor reveal scrolls
                    // the viewport. Scrolling itself does not start an animation.
                    motion.translate(context.scroll - previous.scroll);
                }
            } else {
                self.reset();
            }
        }
        self.context = Some(context);
    }

    pub(crate) fn paint(
        &mut self,
        target: Bounds<Pixels>,
        color: Rgba,
        enabled: bool,
        visible: bool,
        window: &mut Window,
    ) {
        if !enabled {
            self.reset();
            if visible {
                window.paint_quad(fill(target, color));
            }
            return;
        }
        let now = Instant::now();
        let motion = self.motion.get_or_insert_with(|| Motion::new(target, now));
        motion.retarget(target, now);
        let active = motion.active();
        // Blinking hides a settled caret without forgetting its position. Input
        // can start motion even if the preceding frame was in the hidden phase.
        if !visible && !active {
            return;
        }
        if !active {
            window.paint_quad(fill(target, color));
            return;
        }
        window.paint_quad(fill(motion.frame(), color));
        window.request_animation_frame();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;
    use std::time::Duration;

    fn rect(x: f32, y: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(2.0), px(18.0)))
    }

    #[test]
    fn layout_changes_reset_motion_but_scrolling_translates_it() {
        let initial = MotionContext {
            tab: TabId::from_raw(1),
            bounds: rect(0.0, 0.0),
            scroll: point(px(0.0), px(0.0)),
            scale: 1.0,
            row_height: px(18.0),
            font: gpui::font("monospace"),
            code_pad: px(40.0),
            wrap_columns: Some(80),
        };
        for change in 0..7 {
            let mut motion = CursorMotion::default();
            motion.prepare(initial.clone());
            motion.motion = Some(Motion::new(rect(100.0, 100.0), Instant::now()));
            let mut next = initial.clone();
            match change {
                0 => next.tab = TabId::from_raw(2),
                1 => next.bounds.size.width += px(10.0),
                2 => next.scale = 1.5,
                3 => next.row_height = px(20.0),
                4 => next.font = gpui::font("other"),
                5 => next.code_pad = px(48.0),
                _ => next.wrap_columns = None,
            }
            motion.prepare(next);
            assert!(motion.motion.is_none(), "layout change {change}");
        }
        let mut motion = CursorMotion::default();
        motion.prepare(initial.clone());
        motion.motion = Some(Motion::new(rect(100.0, 100.0), Instant::now()));
        motion.prepare(MotionContext {
            scroll: point(px(0.0), px(-18.0)),
            ..initial
        });
        assert_eq!(motion.motion.unwrap().frame(), rect(100.0, 82.0));
    }

    #[test]
    fn glyph_width_changes_preserve_the_positional_spring() {
        let now = Instant::now();
        let mut motion = Motion::new(rect(100.0, 100.0), now);
        motion.retarget(rect(180.0, 100.0), now);
        let later = now + Duration::from_millis(24);
        motion.advance(later);
        let position = motion.spring.position;
        let velocity = motion.spring.velocity;
        let mut target = rect(190.0, 100.0);
        target.size.width += px(0.0001);
        motion.retarget(target, later);
        assert_eq!(motion.spring.position, position);
        assert_eq!(motion.spring.velocity, velocity);
        assert!(motion.active());
        assert_eq!(motion.frame().size, target.size);
    }

    #[test]
    fn all_eight_directions_slide_without_stretching_and_settle() {
        for (dx, dy) in [
            (10.0, 0.0),
            (-10.0, 0.0),
            (0.0, 18.0),
            (0.0, -18.0),
            (240.0, 180.0),
            (-240.0, 180.0),
            (240.0, -180.0),
            (-240.0, -180.0),
        ] {
            let now = Instant::now();
            let mut motion = Motion::new(rect(300.0, 300.0), now);
            let target = rect(300.0 + dx, 300.0 + dy);
            motion.retarget(target, now);
            assert!(motion.active());
            for ms in (8..400).step_by(8) {
                motion.advance(now + Duration::from_millis(ms));
                let frame = motion.frame();
                assert_eq!(frame.size, target.size);
                let origin = rect(300.0, 300.0).origin;
                assert!(frame.left() >= origin.x.min(target.left()) && frame.left() <= origin.x.max(target.left()));
                assert!(frame.top() >= origin.y.min(target.top()) && frame.top() <= origin.y.max(target.top()));
            }
            assert!(!motion.active());
            assert_eq!(motion.frame(), target);
        }
    }

    #[test]
    fn rapid_retarget_preserves_position_and_velocity() {
        let now = Instant::now();
        let mut motion = Motion::new(rect(100.0, 100.0), now);
        motion.retarget(rect(180.0, 118.0), now);
        let later = now + Duration::from_millis(24);
        motion.advance(later);
        let frame = motion.frame();
        let velocity = motion.spring.velocity;
        motion.retarget(rect(80.0, 82.0), later);
        assert_eq!(motion.frame(), frame);
        assert_eq!(motion.spring.velocity, velocity);
        motion.advance(later + Duration::from_secs(1));
        assert_eq!(motion.frame(), rect(80.0, 82.0));
        assert!(!motion.active());
    }

    #[test]
    fn spring_is_independent_of_frame_rate_and_scroll_translation() {
        let now = Instant::now();
        let mut fast = Motion::new(rect(100.0, 100.0), now);
        let mut slow = Motion::new(rect(100.0, 100.0), now);
        fast.retarget(rect(130.0, 118.0), now);
        slow.retarget(rect(130.0, 118.0), now);
        for ms in (4..=80).step_by(4) {
            fast.advance(now + Duration::from_millis(ms));
        }
        slow.advance(now + Duration::from_millis(80));
        assert!(length(fast.spring.position - slow.spring.position) < 0.001);
        let frame = fast.frame();
        let delta = point(px(-7.0), px(-18.0));
        fast.translate(delta);
        assert_eq!(fast.frame().size, frame.size);
        assert!(length(frame.origin + delta - fast.frame().origin) < 0.001);
    }
}
