use hoth_clock::{Clock, ClockError, ClockEvent, ClockMode, ClockRate, SimulationClock};
use hoth_domain::state::SimulationUtc;
use std::{cell::Cell, rc::Rc, time::Duration};

fn utc(instant: &str) -> SimulationUtc {
    SimulationUtc::new(instant.parse().unwrap())
}

#[test]
fn standalone_rate_pause_resume_and_seek_across_midnight() {
    let start = utc("2026-11-17T23:59:59Z");
    let elapsed = Rc::new(Cell::new(Duration::ZERO));
    let fake = Rc::clone(&elapsed);
    let mut clock = Clock::with_elapsed(
        ClockMode::Standalone,
        start,
        ClockRate::new(10.0).unwrap(),
        move || fake.get(),
    );
    elapsed.set(Duration::from_millis(100));
    let midnight = utc("2026-11-18T00:00:00Z");
    assert_eq!(clock.now_utc().unwrap(), midnight);
    clock.apply(ClockEvent::Pause).unwrap();
    elapsed.set(Duration::from_secs(5));
    assert_eq!(clock.now_utc().unwrap(), midnight);
    clock.apply(ClockEvent::Seek(start)).unwrap();
    clock
        .apply(ClockEvent::SetRate(ClockRate::new(1.0).unwrap()))
        .unwrap();
    clock.apply(ClockEvent::Resume).unwrap();
    elapsed.set(Duration::from_secs(6));
    assert_eq!(clock.now_utc().unwrap(), midnight);
    clock
        .apply(ClockEvent::SetRate(ClockRate::new(0.0).unwrap()))
        .unwrap();
    elapsed.set(Duration::from_secs(8));
    assert_eq!(clock.now_utc().unwrap(), midnight);
}

#[test]
fn external_rejects_backward_ticks_and_accepts_exact_backward_seek() {
    let start = utc("2026-11-17T00:00:00Z");
    let later = utc("2026-11-17T00:00:02Z");
    let mut clock = Clock::new(ClockMode::External, start, ClockRate::new(1.0).unwrap());
    clock.apply(ClockEvent::ExternalTick(later)).unwrap();
    assert!(matches!(
        clock.apply(ClockEvent::ExternalTick(start)),
        Err(ClockError::OutOfOrderTick { .. })
    ));
    clock.apply(ClockEvent::Pause).unwrap();
    assert_eq!(
        clock.apply(ClockEvent::ExternalTick(later)),
        Err(ClockError::Paused)
    );
    clock.apply(ClockEvent::Seek(start)).unwrap();
    assert_eq!(clock.now_utc().unwrap(), start);
    clock.apply(ClockEvent::Resume).unwrap();
    clock.apply(ClockEvent::ExternalTick(later)).unwrap();
    assert_eq!(clock.now_utc().unwrap(), later);
}
