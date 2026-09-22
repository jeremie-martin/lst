//! Paint-only caret motion. Document positions and IME geometry never interpolate.
use gpui::{fill, point, px, Bounds, Pixels, Point, Rgba, Window};
use lst_editor::TabId;
use std::time::Instant;

#[derive(Default)]
pub(crate) struct CursorMotion {
    context: Option<MotionContext>,
    motion: Option<Motion>,
}

#[derive(PartialEq)]
struct MotionContext {
    tab: TabId,
    bounds: Bounds<Pixels>,
    scroll: Point<Pixels>,
    scale: f32,
    row_height: Pixels,
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
        if self.target.size != target.size {
            *self = Self::new(target, now);
            return;
        }
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

    pub(crate) fn prepare(
        &mut self,
        tab: TabId,
        bounds: Bounds<Pixels>,
        scroll: Point<Pixels>,
        scale: f32,
        row_height: Pixels,
    ) {
        let context = MotionContext {
            tab,
            bounds,
            scroll,
            scale,
            row_height,
        };
        if let Some(previous) = self.context.as_ref() {
            if previous.tab == tab
                && previous.bounds == bounds
                && previous.scale == scale
                && previous.row_height == row_height
            {
                if let Some(motion) = self.motion.as_mut() {
                    // Keep motion in document space when cursor reveal scrolls
                    // the viewport. Scrolling itself does not start an animation.
                    motion.translate(scroll - previous.scroll);
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
