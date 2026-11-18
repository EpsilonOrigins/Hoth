//! Transport-neutral simulation clock and ordered clock events.

pub use hoth_domain::clock::ClockMode;
use hoth_domain::state::SimulationUtc;
use std::{error::Error, fmt};

/// The ratio of simulation time to elapsed time in standalone mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockRate(f64);

impl ClockRate {
    /// Constructs a finite, nonnegative rate. A zero rate holds the current time.
    pub fn new(multiplier: f64) -> Option<Self> {
        (multiplier.is_finite() && multiplier >= 0.0).then_some(Self(multiplier))
    }

    pub const fn multiplier(self) -> f64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClockEvent {
    /// Freeze automatic advancement at the current simulation instant.
    Pause,
    /// Resume automatic advancement from the current simulation instant.
    Resume,
    /// Set the exact UTC instant in either direction, including while paused.
    Seek(SimulationUtc),
    /// Change the standalone rate from the current simulation instant.
    SetRate(ClockRate),
    /// Supply authoritative absolute UTC to a running external clock.
    ExternalTick(SimulationUtc),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockError {
    ExternalTickInStandalone,
    RateInExternalMode,
    Paused,
    OutOfOrderTick {
        current: SimulationUtc,
        received: SimulationUtc,
    },
    TimeOutOfRange,
}

impl fmt::Display for ClockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExternalTickInStandalone => {
                formatter.write_str("external ticks require an external clock")
            }
            Self::RateInExternalMode => {
                formatter.write_str("a local rate requires a standalone clock")
            }
            Self::Paused => formatter.write_str("external tick rejected while the clock is paused"),
            Self::OutOfOrderTick { current, received } => write!(
                formatter,
                "external tick {} precedes current time {}; use an explicit seek to go backward",
                received.instant(),
                current.instant()
            ),
            Self::TimeOutOfRange => formatter.write_str("simulation time exceeds the UTC range"),
        }
    }
}

impl Error for ClockError {}

/// Supplies absolute UTC to simulation consumers. A clock's authority is fixed
/// for the lifetime of the instance.
pub trait SimulationClock {
    fn mode(&self) -> ClockMode;
    fn now_utc(&self) -> Result<SimulationUtc, ClockError>;
    fn is_paused(&self) -> bool;

    /// Returns the configured standalone rate, including while paused.
    /// External clocks return `None`.
    fn rate(&self) -> Option<ClockRate>;

    /// Applies one event in caller order. A rejected event leaves clock state
    /// intact. Pause and resume are idempotent. A running external clock accepts
    /// an equal tick without changing time and rejects older ticks until an
    /// explicit seek changes its current instant.
    fn apply(&mut self, event: ClockEvent) -> Result<(), ClockError>;
}

mod driver;
pub use driver::Clock;
