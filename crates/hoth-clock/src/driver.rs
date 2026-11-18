use super::{ClockError, ClockEvent, ClockMode, ClockRate, SimulationClock};
use chrono::TimeDelta;
use hoth_domain::state::SimulationUtc;
use std::time::{Duration, Instant};

/// Monotonic standalone clock or exact externally supplied UTC.
pub struct Clock {
    mode: ClockMode,
    base: SimulationUtc,
    anchored_at: Duration,
    elapsed: Box<dyn Fn() -> Duration>,
    rate: ClockRate,
    paused: bool,
}

impl Clock {
    pub fn new(mode: ClockMode, start: SimulationUtc, rate: ClockRate) -> Self {
        let origin = Instant::now();
        Self::with_elapsed(mode, start, rate, move || origin.elapsed())
    }

    pub fn with_elapsed(
        mode: ClockMode,
        start: SimulationUtc,
        rate: ClockRate,
        elapsed: impl Fn() -> Duration + 'static,
    ) -> Self {
        Self {
            mode,
            base: start,
            anchored_at: elapsed(),
            elapsed: Box::new(elapsed),
            rate,
            paused: false,
        }
    }

    fn at_elapsed(&self, now: Duration) -> Result<SimulationUtc, ClockError> {
        if self.paused || self.mode == ClockMode::External {
            return Ok(self.base);
        }
        let seconds = now.saturating_sub(self.anchored_at).as_secs_f64() * self.rate.multiplier();
        let duration =
            Duration::try_from_secs_f64(seconds).map_err(|_| ClockError::TimeOutOfRange)?;
        let delta = TimeDelta::from_std(duration).map_err(|_| ClockError::TimeOutOfRange)?;
        self.base
            .instant()
            .checked_add_signed(delta)
            .map(SimulationUtc::new)
            .ok_or(ClockError::TimeOutOfRange)
    }
}

impl SimulationClock for Clock {
    fn mode(&self) -> ClockMode {
        self.mode
    }

    fn now_utc(&self) -> Result<SimulationUtc, ClockError> {
        self.at_elapsed((self.elapsed)())
    }

    fn is_paused(&self) -> bool {
        self.paused
    }

    fn rate(&self) -> Option<ClockRate> {
        (self.mode == ClockMode::Standalone).then_some(self.rate)
    }

    fn apply(&mut self, event: ClockEvent) -> Result<(), ClockError> {
        let now = (self.elapsed)();
        match event {
            ClockEvent::Pause => {
                self.base = self.at_elapsed(now)?;
                self.paused = true;
            }
            ClockEvent::Resume if self.paused => {
                self.anchored_at = now;
                self.paused = false;
            }
            ClockEvent::Resume => {}
            ClockEvent::Seek(at) => {
                self.base = at;
                self.anchored_at = now;
            }
            ClockEvent::SetRate(_) if self.mode == ClockMode::External => {
                return Err(ClockError::RateInExternalMode)
            }
            ClockEvent::SetRate(rate) => {
                self.base = self.at_elapsed(now)?;
                self.anchored_at = now;
                self.rate = rate;
            }
            ClockEvent::ExternalTick(_) if self.mode == ClockMode::Standalone => {
                return Err(ClockError::ExternalTickInStandalone)
            }
            ClockEvent::ExternalTick(_) if self.paused => return Err(ClockError::Paused),
            ClockEvent::ExternalTick(received) if received < self.base => {
                return Err(ClockError::OutOfOrderTick {
                    current: self.base,
                    received,
                });
            }
            ClockEvent::ExternalTick(at) => self.base = at,
        }
        Ok(())
    }
}
