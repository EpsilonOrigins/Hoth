use hoth_domain::state::{NoradId, SatelliteState, SimulationUtc, UtcInstant};
use hoth_publisher::{PublishError, Publisher};

const STATE: &str = include_str!("../../hoth-domain/tests/fixtures/satellite-state.json");

type Batch = (SimulationUtc, Vec<SatelliteState>);

#[derive(Default, PartialEq)]
enum Lifecycle {
    #[default]
    New,
    Ready,
    Closed,
}

#[derive(Default)]
struct MockPublisher {
    lifecycle: Lifecycle,
    pending: Option<Batch>,
    delivered: Vec<Batch>,
}

impl Publisher for MockPublisher {
    fn init(&mut self) -> Result<(), PublishError> {
        if self.lifecycle != Lifecycle::New {
            return Err(PublishError::InvalidLifecycle);
        }
        self.lifecycle = Lifecycle::Ready;
        Ok(())
    }

    fn publish_batch(
        &mut self,
        at_utc: SimulationUtc,
        states: &[SatelliteState],
    ) -> Result<(), PublishError> {
        if self.lifecycle != Lifecycle::Ready {
            return Err(PublishError::InvalidLifecycle);
        }
        if let Some((index, state)) = states
            .iter()
            .enumerate()
            .find(|(_, state)| state.at_utc != at_utc)
        {
            return Err(PublishError::TimestampMismatch {
                index,
                expected: at_utc,
                actual: state.at_utc,
            });
        }
        if self.pending.is_some() {
            return Err(PublishError::Backpressure);
        }
        self.pending = Some((at_utc, states.to_vec()));
        Ok(())
    }

    fn flush(&mut self) -> Result<(), PublishError> {
        if self.lifecycle != Lifecycle::Ready {
            return Err(PublishError::InvalidLifecycle);
        }
        self.delivered.extend(self.pending.take());
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), PublishError> {
        self.lifecycle = Lifecycle::Closed;
        self.pending = None;
        Ok(())
    }
}

#[test]
fn both_publishers_capture_identical_ordered_batches_at_25_hz() {
    let fixture = fixture();
    let expected = [instant(0), instant(40)].map(|at_utc| (at_utc, states_at(&fixture, at_utc)));
    let mut first = MockPublisher::default();
    let mut second = MockPublisher::default();

    for publisher in [&mut first as &mut dyn Publisher, &mut second] {
        publisher.init().unwrap();
        for (at_utc, states) in &expected {
            publisher.publish_batch(*at_utc, states).unwrap();
            publisher.flush().unwrap();
        }
        // A seek can replay the first timestamp and the same propagation inputs.
        publisher
            .publish_batch(expected[0].0, &expected[0].1)
            .unwrap();
        publisher.flush().unwrap();
        publisher.shutdown().unwrap();
    }

    assert_eq!(first.delivered, second.delivered);
    assert_eq!(&first.delivered[..2], &expected);
    assert_eq!(first.delivered[2], expected[0]);
    for (_, states) in &first.delivered {
        let ids: Vec<_> = states.iter().map(|state| state.norad_id.get()).collect();
        assert_eq!(ids, [43013, 25544]);
    }
}

#[test]
fn backpressure_leaves_the_rejected_batch_available_for_an_ordered_retry() {
    let fixture = fixture();
    let mut publisher = MockPublisher::default();
    let first = instant(0);
    let second = instant(40);
    let first_states = states_at(&fixture, first);
    let second_states = states_at(&fixture, second);

    publisher.init().unwrap();
    publisher.publish_batch(first, &first_states).unwrap();
    assert!(matches!(
        publisher.publish_batch(second, &second_states),
        Err(PublishError::Backpressure)
    ));
    publisher.flush().unwrap();
    assert_eq!(publisher.delivered.len(), 1);
    assert_eq!(publisher.delivered[0], (first, first_states.clone()));
    publisher.publish_batch(second, &second_states).unwrap();
    publisher.flush().unwrap();
    assert_eq!(
        publisher.delivered,
        [(first, first_states), (second, second_states)]
    );
}

#[test]
fn rejects_invalid_lifecycle_and_mixed_timestamps_without_accepting_work() {
    let mut publisher = MockPublisher::default();
    let at = instant(0);
    let mut states = states_at(&fixture(), at);
    assert!(matches!(
        publisher.publish_batch(at, &states),
        Err(PublishError::InvalidLifecycle)
    ));
    assert!(matches!(
        publisher.flush(),
        Err(PublishError::InvalidLifecycle)
    ));
    publisher.init().unwrap();
    assert!(matches!(
        publisher.init(),
        Err(PublishError::InvalidLifecycle)
    ));

    states[1].at_utc = instant(40);
    assert!(matches!(
        publisher.publish_batch(at, &states),
        Err(PublishError::TimestampMismatch { index: 1, expected, actual })
            if expected == at && actual == states[1].at_utc
    ));
    publisher.flush().unwrap();
    assert!(publisher.delivered.is_empty());

    states[1].at_utc = at;
    publisher.publish_batch(at, &states).unwrap();
    publisher.shutdown().unwrap();
    publisher.shutdown().unwrap();
    assert!(publisher.pending.is_none());
    assert!(publisher.delivered.is_empty());
    assert!(matches!(
        publisher.init(),
        Err(PublishError::InvalidLifecycle)
    ));
}

fn fixture() -> SatelliteState {
    serde_json::from_str(STATE).unwrap()
}

fn states_at(fixture: &SatelliteState, at_utc: SimulationUtc) -> Vec<SatelliteState> {
    [43013, 25544]
        .into_iter()
        .map(|id| SatelliteState {
            norad_id: NoradId::new(id).unwrap(),
            at_utc,
            ..fixture.clone()
        })
        .collect()
}

fn instant(milliseconds: u32) -> SimulationUtc {
    SimulationUtc::new(UtcInstant::from_timestamp(1_794_873_600, milliseconds * 1_000_000).unwrap())
}
