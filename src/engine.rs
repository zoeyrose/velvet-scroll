//! Wheel acceleration in Linux v120 units (120 units = one detent).
//!
//! Tuning: an 80 ms exponential distance history estimates speed independently of
//! packet count. Gain rises smoothly from 1 at 2,200 units/s to the configured
//! ceiling at 7,200 units/s. Integrating that curve across each packet makes a
//! coalesced packet equivalent to same-time fragments. Slow detents remain exact.
//! A direction change or 180 ms input gap starts a fresh gesture.
//!
//! Optional coast requires at least four detents and 40 ms of fast movement. It
//! waits 65 ms for another physical packet, then adds a short exponentially
//! decaying tail, capped at two detents and 450 ms. All state belongs to one axis
//! of one device; callers must cancel on device loss, disable, or button activity.

use std::time::Duration;

const HISTORY_SECONDS: f64 = 0.080;
const PRECISION_SPEED: f64 = 2_200.0;
const FULL_SPEED: f64 = 7_200.0;
const MAX_SPEED: f64 = 12_000.0;
const RESET_GAP: Duration = Duration::from_millis(180);
const MAX_INPUT: i32 = 14_400;
const COAST_DELAY: Duration = Duration::from_millis(65);
const COAST_DURATION: Duration = Duration::from_millis(450);
const COAST_DECAY: f64 = 0.110;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Maximum gain, clamped to 1..=8. Nonfinite values use the default of 3.
    pub acceleration: f64,
    pub coast: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            acceleration: 3.0,
            coast: false,
        }
    }
}

#[derive(Debug)]
struct Coast {
    start: Duration,
    last_tick: Duration,
    velocity: f64,
    remaining: f64,
}

#[derive(Debug)]
pub struct Engine {
    settings: Settings,
    last_input: Option<Duration>,
    gesture_start: Duration,
    direction: i32,
    distance_history: f64,
    gesture_distance: f64,
    fraction: f64,
    coast: Option<Coast>,
}

impl Engine {
    pub fn new(settings: Settings) -> Self {
        let mut engine = Self {
            settings: Settings::default(),
            last_input: None,
            gesture_start: Duration::ZERO,
            direction: 0,
            distance_history: 0.0,
            gesture_distance: 0.0,
            fraction: 0.0,
            coast: None,
        };
        engine.set_settings(settings);
        engine
    }

    /// Apply settings atomically and discard any old gesture or pending tail.
    pub fn set_settings(&mut self, mut settings: Settings) {
        settings.acceleration = if settings.acceleration.is_finite() {
            settings.acceleration.clamp(1.0, 8.0)
        } else {
            Settings::default().acceleration
        };
        self.settings = settings;
        self.cancel();
    }

    pub fn cancel(&mut self) {
        self.last_input = None;
        self.direction = 0;
        self.distance_history = 0.0;
        self.gesture_distance = 0.0;
        self.fraction = 0.0;
        self.coast = None;
    }

    /// Includes the pending delay; only schedule timer wakeups while this is true.
    pub fn is_coasting(&self) -> bool {
        self.coast.is_some()
    }

    /// Transform signed high-resolution wheel input. `now` is monotonic time.
    /// Badly formed extreme packets are bounded before arithmetic or conversion.
    pub fn scroll(&mut self, delta: i32, now: Duration) -> i32 {
        if delta == 0 {
            return 0;
        }
        let delta = delta.clamp(-MAX_INPUT, MAX_INPUT);
        let direction = delta.signum();
        let resumed_after_tail = self.coast.as_ref().is_some_and(|c| now >= c.start);
        let stale = self
            .last_input
            .is_some_and(|last| now < last || now.saturating_sub(last) >= RESET_GAP);
        if direction != self.direction || stale || resumed_after_tail {
            self.cancel();
        }
        self.coast = None;
        if let Some(last) = self.last_input {
            self.distance_history *=
                (-now.saturating_sub(last).as_secs_f64() / HISTORY_SECONDS).exp();
        } else {
            self.gesture_start = now;
        }
        self.direction = direction;
        self.last_input = Some(now);

        let distance = f64::from(delta.abs());
        let before = self.distance_history;
        let after = before + distance;
        let output =
            distance + (self.settings.acceleration - 1.0) * (gain_area(after) - gain_area(before));
        self.distance_history = after.min(MAX_SPEED * HISTORY_SECONDS);
        self.gesture_distance = (self.gesture_distance + distance).min(14_400.0);

        let speed = self.distance_history / HISTORY_SECONDS;
        if self.settings.coast
            && speed >= 4_000.0
            && self.gesture_distance >= 480.0
            && now.saturating_sub(self.gesture_start) >= Duration::from_millis(40)
        {
            // Limit the handoff velocity independently of the acceleration knob.
            let velocity = (speed * 0.20).min(1_800.0);
            if let Some(start) = now.checked_add(COAST_DELAY) {
                self.coast = Some(Coast {
                    start,
                    last_tick: start,
                    velocity,
                    remaining: 240.0,
                });
            }
        }
        self.quantize(output)
    }

    /// Advance an optional tail, normally every 8 ms. Never replay a backlog
    /// following a scheduler stall, suspend, or a backwards clock.
    pub fn tick(&mut self, now: Duration) -> i32 {
        let Some(coast) = self.coast.as_mut() else {
            return 0;
        };
        if self.last_input.is_some_and(|last| now < last) {
            self.cancel();
            return 0;
        }
        if now < coast.start {
            return 0;
        }
        if now < coast.last_tick
            || now.saturating_sub(coast.start) >= COAST_DURATION
            || now.saturating_sub(coast.last_tick) > Duration::from_millis(100)
        {
            self.cancel();
            return 0;
        }
        let from = coast.last_tick.saturating_sub(coast.start).as_secs_f64();
        let to = now.saturating_sub(coast.start).as_secs_f64();
        let distance = (coast.velocity
            * COAST_DECAY
            * ((-from / COAST_DECAY).exp() - (-to / COAST_DECAY).exp()))
        .max(0.0)
        .min(coast.remaining);
        coast.last_tick = now;
        coast.remaining -= distance;
        let finished = coast.remaining <= 0.0 || coast.velocity * (-to / COAST_DECAY).exp() < 25.0;
        let output = self.quantize(distance);
        if finished {
            self.cancel();
        }
        output
    }

    fn quantize(&mut self, distance: f64) -> i32 {
        // Magnitudes keep a sub-unit remainder from ever reversing direction.
        let total = distance + self.fraction;
        let whole = total.floor();
        self.fraction = total - whole;
        (whole as i32) * self.direction
    }
}

/// Antiderivative of smoothstep gain in distance-history units. Below the
/// precision threshold the extra gain is zero; above full speed it is one.
fn gain_area(distance: f64) -> f64 {
    let low = PRECISION_SPEED * HISTORY_SECONDS;
    let span = (FULL_SPEED - PRECISION_SPEED) * HISTORY_SECONDS;
    let shifted = distance - low;
    if shifted <= 0.0 {
        0.0
    } else if shifted >= span {
        shifted - span * 0.5
    } else {
        let x = shifted / span;
        span * (x * x * x - 0.5 * x * x * x * x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    fn with_coast() -> Engine {
        Engine::new(Settings {
            acceleration: 3.0,
            coast: true,
        })
    }

    fn flick(engine: &mut Engine) {
        for time in (0..=80).step_by(10) {
            engine.scroll(120, ms(time));
        }
        assert!(engine.is_coasting());
    }

    #[test]
    fn slow_detents_and_slow_high_resolution_stay_exact() {
        let mut engine = Engine::new(Settings::default());
        for time in (0..2_000).step_by(160) {
            assert_eq!(engine.scroll(120, ms(time)), 120);
        }
        engine.cancel();
        for time in (0..2_000).step_by(10) {
            assert_eq!(engine.scroll(6, ms(time)), 6);
        }
    }

    #[test]
    fn fast_gain_is_bounded_and_does_not_touch_first_notch() {
        let mut engine = Engine::new(Settings::default());
        assert_eq!(engine.scroll(120, ms(0)), 120);
        let outputs: Vec<_> = (10..300)
            .step_by(10)
            .map(|t| engine.scroll(120, ms(t)))
            .collect();
        assert!(outputs.iter().all(|&v| (120..=360).contains(&v)));
        assert_eq!(*outputs.last().unwrap(), 360);
    }

    #[test]
    fn reversal_idle_and_backwards_time_reset_gain() {
        let mut engine = Engine::new(Settings::default());
        for t in (0..100).step_by(10) {
            engine.scroll(120, ms(t));
        }
        assert_eq!(engine.scroll(-120, ms(100)), -120);
        assert_eq!(engine.scroll(-120, ms(400)), -120);
        assert_eq!(engine.scroll(-120, ms(300)), -120);
    }

    #[test]
    fn packet_fragmentation_preserves_fractional_distance() {
        let mut batch = Engine::new(Settings::default());
        let mut fragments = Engine::new(Settings::default());
        for t in (0..200).step_by(20) {
            let expected = batch.scroll(120, ms(t));
            let actual: i32 = (0..120).map(|_| fragments.scroll(1, ms(t))).sum();
            // Floating-point summation may place an exact integer on either side
            // of the floor, but the carried difference never exceeds one unit.
            assert!((expected - actual).abs() <= 1);
            assert!((batch.fraction - fragments.fraction).abs() < 1.0);
        }
        assert!(batch.fraction > 0.0);
    }

    #[test]
    fn unity_gain_is_exact_even_at_extreme_speed() {
        let mut engine = Engine::new(Settings {
            acceleration: 1.0,
            coast: false,
        });
        for t in 0..100 {
            assert_eq!(engine.scroll(120, ms(t)), 120);
        }
    }

    #[test]
    fn coast_is_delayed_finite_and_conservative() {
        let mut engine = with_coast();
        flick(&mut engine);
        assert_eq!(engine.tick(ms(120)), 0);
        let mut total = 0;
        for t in (145..1_000).step_by(8) {
            let amount = engine.tick(ms(t));
            assert!(amount >= 0);
            total += amount;
        }
        assert!((1..=240).contains(&total));
        assert!(!engine.is_coasting());
        assert_eq!(engine.tick(ms(2_000)), 0);
    }

    #[test]
    fn slow_scroll_and_single_bursts_do_not_coast() {
        let mut engine = with_coast();
        for t in (0..1_000).step_by(160) {
            engine.scroll(120, ms(t));
            assert!(!engine.is_coasting());
        }
        engine.cancel();
        engine.scroll(1_200, ms(0));
        assert!(!engine.is_coasting());
    }

    #[test]
    fn cancellation_and_settings_discard_pending_coast() {
        let mut engine = with_coast();
        flick(&mut engine);
        engine.cancel();
        assert!(!engine.is_coasting());
        assert_eq!(engine.tick(ms(200)), 0);
        flick(&mut engine);
        engine.set_settings(Settings::default());
        assert!(!engine.is_coasting());
        assert_eq!(engine.tick(ms(200)), 0);
        assert_eq!(engine.scroll(120, ms(201)), 120);
    }

    #[test]
    fn physical_input_interrupts_tail_immediately() {
        let mut engine = with_coast();
        flick(&mut engine);
        assert!(engine.tick(ms(153)) > 0);
        assert_eq!(engine.scroll(-120, ms(160)), -120);
        assert!(!engine.is_coasting());
        assert_eq!(engine.tick(ms(168)), 0);
    }

    #[test]
    fn stalled_timer_discards_tail_instead_of_replaying_it() {
        let mut engine = with_coast();
        flick(&mut engine);
        assert_eq!(engine.tick(ms(300)), 0);
        assert!(!engine.is_coasting());
    }

    #[test]
    fn malformed_input_and_settings_are_bounded() {
        for gain in [f64::NAN, f64::INFINITY, -5.0, 99.0] {
            let mut engine = Engine::new(Settings {
                acceleration: gain,
                coast: true,
            });
            for (value, time) in [(i32::MIN, 0), (i32::MAX, 1), (i32::MAX, 2)] {
                let output = engine.scroll(value, ms(time));
                assert_eq!(output.signum(), value.signum());
                assert!(output.abs() <= MAX_INPUT * 8);
            }
        }
    }
}
