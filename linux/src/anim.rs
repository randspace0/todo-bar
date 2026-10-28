use std::time::{Duration, Instant};

use gpui::{point, size, Bounds, Pixels};

pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        2.0 * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
    }
}

fn lerp(a: Pixels, b: Pixels, t: f32) -> Pixels {
    a + (b - a) * t
}

#[derive(Clone, Copy)]
pub struct Anim {
    pub from: Bounds<Pixels>,
    pub to: Bounds<Pixels>,
    pub start: Instant,
    pub dur: Duration,
}

impl Anim {
    pub fn new(from: Bounds<Pixels>, to: Bounds<Pixels>, dur: Duration) -> Self {
        Self { from, to, start: Instant::now(), dur }
    }

    pub fn progress(&self) -> f32 {
        (self.start.elapsed().as_secs_f32() / self.dur.as_secs_f32()).min(1.0)
    }

    pub fn done(&self) -> bool {
        self.progress() >= 1.0
    }

    pub fn current(&self) -> Bounds<Pixels> {
        let t = ease_in_out(self.progress());
        Bounds {
            origin: point(
                lerp(self.from.origin.x, self.to.origin.x, t),
                lerp(self.from.origin.y, self.to.origin.y, t),
            ),
            size: size(
                lerp(self.from.size.width, self.to.size.width, t),
                lerp(self.from.size.height, self.to.size.height, t),
            ),
        }
    }
}

/// A scalar that eases toward a target.
#[derive(Clone, Copy)]
pub struct Tween {
    from: f32,
    to: f32,
    start: Instant,
    dur: Duration,
}

impl Tween {
    pub fn new(value: f32) -> Self {
        Self { from: value, to: value, start: Instant::now(), dur: Duration::from_millis(1) }
    }

    pub fn target(&self) -> f32 {
        self.to
    }

    pub fn value(&self) -> f32 {
        let t = (self.start.elapsed().as_secs_f32() / self.dur.as_secs_f32()).min(1.0);
        self.from + (self.to - self.from) * ease_in_out(t)
    }

    pub fn active(&self) -> bool {
        self.start.elapsed() < self.dur
    }

    pub fn set_target(&mut self, to: f32, dur: Duration) {
        if (self.to - to).abs() > f32::EPSILON {
            self.from = self.value();
            self.to = to;
            self.start = Instant::now();
            self.dur = dur;
        }
    }
}

/// 0→1 over `dur` since `since`, eased.
pub fn since_progress(since: Instant, dur: Duration) -> f32 {
    ease_in_out((since.elapsed().as_secs_f32() / dur.as_secs_f32()).min(1.0))
}
