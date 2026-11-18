use hoth_clock::{ClockError, ClockEvent, ClockMode, ClockRate, SimulationClock};
use hoth_domain::state::{SimulationUtc, UtcInstant};

struct MockClock {
    mode: ClockMode,
    now: SimulationUtc,
    is_paused: bool,
    rate: Option<ClockRate>,
}

impl MockClock {
    fn new(mode: ClockMode) -> Self {
        Self {
            mode,
            now: instant(0),
            is_paused: false,
            rate: match mode {
                ClockMode::Standalone => ClockRate::new(1.0),
                ClockMode::External => None,
            },
        }
    }
}

impl SimulationClock for MockClock {
    fn mode(&self) -> ClockMode {
        self.mode
    }

    fn now_utc(&self) -> Result<SimulationUtc, ClockError> {
        Ok(self.now)
    }

    fn is_paused(&self) -> bool {
        self.is_paused
    }

    fn rate(&self) -> Option<ClockRate> {
        self.rate
    }

    fn apply(&mut self, event: ClockEvent) -> Result<(), ClockError> {
        match event {
            ClockEvent::Pause => self.is_paused = true,
            ClockEvent::Resume => self.is_paused = false,
            ClockEvent::Seek(at_utc) => self.now = at_utc,
            ClockEvent::SetRate(_) if self.mode == ClockMode::External => {
                return Err(ClockError::RateInExternalMode);
            }
            ClockEvent::SetRate(rate) => self.rate = Some(rate),
            ClockEvent::ExternalTick(_) if self.mode == ClockMode::Standalone => {
                return Err(ClockError::ExternalTickInStandalone);
            }
            ClockEvent::ExternalTick(_) if self.is_paused => return Err(ClockError::Paused),
            ClockEvent::ExternalTick(received) if received < self.now => {
                return Err(ClockError::OutOfOrderTick {
                    current: self.now,
                    received,
                });
            }
            ClockEvent::ExternalTick(at_utc) => self.now = at_utc,
        }
        Ok(())
    }
}

#[test]
fn an_external_clock_replays_the_same_absolute_ticks_after_a_backward_seek() {
    let clock: &mut dyn SimulationClock = &mut MockClock::new(ClockMode::External);
    let start = instant(0);
    let next = instant(40);
    let later = instant(80);
    assert_eq!(clock.mode(), ClockMode::External);
    assert_eq!(clock.rate(), None);

    // The fixed propagation inputs can be read again at the same absolute UTC after a seek.
    let read_times = |clock: &mut dyn SimulationClock| {
        [next, later].map(|at_utc| {
            clock.apply(ClockEvent::ExternalTick(at_utc)).unwrap();
            clock.now_utc().unwrap()
        })
    };
    let first_pass = read_times(clock);
    assert_eq!(first_pass, [next, later]);
    clock.apply(ClockEvent::ExternalTick(later)).unwrap();
    assert_eq!(clock.now_utc().unwrap(), later);
    assert_eq!(
        clock.apply(ClockEvent::ExternalTick(next)),
        Err(ClockError::OutOfOrderTick {
            current: later,
            received: next,
        })
    );
    assert_eq!(clock.now_utc().unwrap(), later);

    clock.apply(ClockEvent::Pause).unwrap();
    clock.apply(ClockEvent::Pause).unwrap();
    assert_eq!(
        clock.apply(ClockEvent::ExternalTick(instant(120))),
        Err(ClockError::Paused)
    );
    assert_eq!(clock.now_utc().unwrap(), later);
    clock.apply(ClockEvent::Seek(start)).unwrap();
    assert!(clock.is_paused());
    assert_eq!(clock.now_utc().unwrap(), start);
    clock.apply(ClockEvent::Resume).unwrap();
    clock.apply(ClockEvent::Resume).unwrap();
    assert!(!clock.is_paused());
    assert_eq!(read_times(clock), first_pass);
}

#[test]
fn clock_modes_reject_events_for_the_other_authority() {
    let mut standalone = MockClock::new(ClockMode::Standalone);
    let mut external = MockClock::new(ClockMode::External);
    let faster = ClockRate::new(10.0).unwrap();

    assert_eq!(
        standalone.apply(ClockEvent::ExternalTick(instant(40))),
        Err(ClockError::ExternalTickInStandalone)
    );
    assert_eq!(standalone.now_utc().unwrap(), instant(0));
    assert_eq!(
        external.apply(ClockEvent::SetRate(faster)),
        Err(ClockError::RateInExternalMode)
    );
    assert_eq!(external.rate(), None);

    standalone.apply(ClockEvent::Pause).unwrap();
    standalone.apply(ClockEvent::SetRate(faster)).unwrap();
    assert!(standalone.is_paused());
    assert_eq!(standalone.rate(), Some(faster));
    assert_eq!(standalone.now_utc().unwrap(), instant(0));
}

#[test]
fn clock_rates_accept_zero_and_reject_negative_or_nonfinite_values() {
    for rate in [0.0, 1.0, 10.0] {
        assert_eq!(ClockRate::new(rate).unwrap().multiplier(), rate);
    }
    for rate in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(ClockRate::new(rate), None);
    }
}

fn instant(milliseconds: u32) -> SimulationUtc {
    SimulationUtc::new(UtcInstant::from_timestamp(1_794_873_600, milliseconds * 1_000_000).unwrap())
}
