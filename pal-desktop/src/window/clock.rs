//! Fixed-step desktop frame clock.

use std::time::{Duration, Instant};

pub(super) struct FrameClock {
    ui_epoch: Instant,
    last_update: Instant,
    accumulator: Duration,
}

impl FrameClock {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            ui_epoch: now,
            last_update: now,
            accumulator: Duration::ZERO,
        }
    }

    pub(super) fn begin_frame(&mut self, now: Instant) -> Duration {
        let elapsed = now
            .duration_since(self.last_update)
            .min(Duration::from_millis(250));
        self.accumulator += elapsed;
        self.last_update = now;
        elapsed
    }

    pub(super) fn accumulator(&self) -> Duration {
        self.accumulator
    }

    pub(super) fn consume(&mut self, interval: Duration) {
        self.accumulator = self.accumulator.saturating_sub(interval);
    }

    pub(super) fn reset_accumulator(&mut self) {
        self.accumulator = Duration::ZERO;
    }

    pub(super) fn ui_elapsed(&self, now: Instant) -> Duration {
        now.duration_since(self.ui_epoch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_elapsed_is_capped_and_consumed_safely() {
        let start = Instant::now();
        let mut clock = FrameClock::new(start);
        assert_eq!(
            clock.begin_frame(start + Duration::from_secs(1)),
            Duration::from_millis(250)
        );
        clock.consume(Duration::from_millis(40));
        assert_eq!(clock.accumulator(), Duration::from_millis(210));
        clock.consume(Duration::from_secs(1));
        assert_eq!(clock.accumulator(), Duration::ZERO);
    }
}
