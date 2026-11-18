//! Transport-neutral publishing of ordered batches of canonical satellite states.

use hoth_domain::state::{SatelliteState, SimulationUtc};
use std::{error::Error, fmt};

#[derive(Debug)]
pub enum PublishError {
    /// The requested operation is invalid in the publisher's current lifecycle.
    InvalidLifecycle,
    /// One state's timestamp differs from the batch timestamp.
    TimestampMismatch {
        index: usize,
        expected: SimulationUtc,
        actual: SimulationUtc,
    },
    /// Capacity is temporarily exhausted. No part of this batch was accepted.
    Backpressure,
    /// A sink failed. Delivery may be partial or unknown.
    Sink(Box<dyn Error + Send + Sync>),
}

impl fmt::Display for PublishError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLifecycle => {
                formatter.write_str("publisher operation is invalid in this lifecycle state")
            }
            Self::TimestampMismatch {
                index,
                expected,
                actual,
            } => write!(
                formatter,
                "state {index} has timestamp {}; expected batch timestamp {}",
                actual.instant(),
                expected.instant()
            ),
            Self::Backpressure => {
                formatter.write_str("publisher is full and did not accept the batch")
            }
            Self::Sink(source) => write!(formatter, "publisher sink failed: {source}"),
        }
    }
}

impl Error for PublishError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Sink(source) => Some(source.as_ref()),
            _ => None,
        }
    }
}

/// A synchronous port for a single publisher instance. Initialize before
/// submitting or flushing batches. A sink failure ends submissions to this
/// instance, and the caller should shut it down.
pub trait Publisher {
    /// Initializes this instance once. A failed initialization permits shutdown
    /// for cleanup, and further initialization requires a new instance.
    fn init(&mut self) -> Result<(), PublishError>;

    /// Accepts an entire batch in caller order. All states must use `at_utc`;
    /// a mismatch rejects the entire batch. Timestamps may decrease after a seek.
    /// Success acknowledges local acceptance, including optional bounded queuing.
    /// Return backpressure on a full queue. The caller may retry this batch
    /// before submitting newer work or drop it in full.
    fn publish_batch(
        &mut self,
        at_utc: SimulationUtc,
        states: &[SatelliteState],
    ) -> Result<(), PublishError>;

    /// Drains accepted work to the sink or reports a sink failure. A successful
    /// flush gives no remote-delivery guarantee.
    fn flush(&mut self) -> Result<(), PublishError>;

    /// Releases resources and discards any pending work. Call flush first to
    /// drain accepted work. Shutdown is terminal and safe to call again.
    fn shutdown(&mut self) -> Result<(), PublishError>;
}
