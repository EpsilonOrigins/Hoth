use hoth_clock::ClockMode;
use hoth_domain::state::{SatelliteState, SimulationUtc};
use hoth_publisher::{PublishError, Publisher};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};

const QUEUE_DEPTH: usize = 2;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(15);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(3);

type Batch = (SimulationUtc, Arc<Vec<SatelliteState>>);

#[derive(Default)]
pub struct OutputStats {
    pub sent_batches: AtomicU64,
    pub sent_states: AtomicU64,
    pub dropped_batches: AtomicU64,
    pub errors: AtomicU64,
}

pub struct Output {
    name: String,
    period: Duration,
    next_wall: Option<Instant>,
    next_utc: Option<SimulationUtc>,
    sender: Option<mpsc::SyncSender<Batch>>,
    done: mpsc::Receiver<()>,
    failed: Arc<AtomicBool>,
    stats: Arc<OutputStats>,
}

impl Output {
    pub fn start(
        name: String,
        rate_hz: f64,
        mut publisher: impl Publisher + Send + 'static,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let period = Duration::try_from_secs_f64(rate_hz.recip())?;
        if period.is_zero() {
            return Err("publisher period is zero".into());
        }
        let (sender, queue) = mpsc::sync_channel::<Batch>(QUEUE_DEPTH);
        let (startup, active) = mpsc::sync_channel(1);
        let (completed, done) = mpsc::sync_channel(1);
        let failed = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(OutputStats::default());
        let worker_name = name.clone();
        let worker_failed = Arc::clone(&failed);
        let worker_stats = Arc::clone(&stats);
        thread::Builder::new().name(name.clone()).spawn(move || {
            let initialized = publisher.init();
            let ready = initialized.is_ok();
            if startup.send(initialized).is_ok() && ready {
                for (at, states) in queue {
                    match publisher.publish_batch(at, &states) {
                        Ok(()) => {
                            worker_stats.sent_batches.fetch_add(1, Relaxed);
                            worker_stats
                                .sent_states
                                .fetch_add(states.len() as u64, Relaxed);
                        }
                        Err(PublishError::Backpressure) => {
                            worker_stats.dropped_batches.fetch_add(1, Relaxed);
                        }
                        Err(error) => {
                            report(&worker_name, &worker_failed, &worker_stats, error);
                            break;
                        }
                    }
                }
                if !worker_failed.load(Relaxed) {
                    if let Err(error) = publisher.flush() {
                        report(&worker_name, &worker_failed, &worker_stats, error);
                    }
                }
            }
            if let Err(error) = publisher.shutdown() {
                report(&worker_name, &worker_failed, &worker_stats, error);
            }
            let _ = completed.send(());
        })?;
        active
            .recv_timeout(STARTUP_TIMEOUT)
            .map_err(|error| format!("{name} initialization: {error}"))??;
        Ok(Self {
            name,
            period,
            next_wall: None,
            next_utc: None,
            sender: Some(sender),
            done,
            failed,
            stats,
        })
    }

    pub fn submit_batch(&mut self, at: SimulationUtc, states: Arc<Vec<SatelliteState>>) {
        self.submit(at, Instant::now(), states);
    }

    pub fn drain(&mut self) -> bool {
        self.finish()
    }

    pub fn failed(&self) -> bool {
        self.failed.load(Relaxed)
    }

    pub fn stats(&self) -> &OutputStats {
        &self.stats
    }

    pub(crate) fn due(&self, mode: ClockMode, at: SimulationUtc, wall: Instant) -> bool {
        !self.failed()
            && match mode {
                ClockMode::Standalone => self.next_wall.is_none_or(|next| wall >= next),
                ClockMode::External => self.next_utc.is_none_or(|next| at >= next),
            }
    }

    pub(crate) fn wait(&self, now: Instant) -> Option<Duration> {
        if self.failed() {
            return None;
        }
        self.next_wall
            .map(|next| next.saturating_duration_since(now))
    }

    pub(crate) fn reset_schedule(&mut self) {
        self.next_utc = None;
        self.next_wall = None;
    }

    pub(crate) fn submit(
        &mut self,
        at: SimulationUtc,
        wall: Instant,
        states: Arc<Vec<SatelliteState>>,
    ) {
        if self.failed() {
            return;
        }
        let next = self.next_wall.unwrap_or(wall);
        // Preserve phase under normal jitter. Skip missed deadlines under overload.
        let period = self.period;
        self.next_wall = Some(if next + period > wall {
            next + period
        } else {
            wall + period
        });
        self.next_utc = chrono::TimeDelta::from_std(period)
            .ok()
            .and_then(|delta| at.instant().checked_add_signed(delta))
            .map(SimulationUtc::new);
        match self
            .sender
            .as_ref()
            .expect("active publisher")
            .try_send((at, states))
        {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => {
                self.stats.dropped_batches.fetch_add(1, Relaxed);
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                self.stats.errors.fetch_add(1, Relaxed);
                self.failed.store(true, Relaxed);
            }
        }
    }

    pub(crate) fn finish(&mut self) -> bool {
        self.sender.take();
        let drained = self.done.recv_timeout(DRAIN_TIMEOUT).is_ok();
        eprintln!(
            "publisher={} batches={} states={} dropped_batches={} errors={} drained={}",
            self.name,
            self.stats.sent_batches.load(Relaxed),
            self.stats.sent_states.load(Relaxed),
            self.stats.dropped_batches.load(Relaxed),
            self.stats.errors.load(Relaxed),
            drained,
        );
        drained && !self.failed()
    }
}

fn report(name: &str, failed: &AtomicBool, stats: &OutputStats, error: PublishError) {
    eprintln!("publisher {name}: {error}");
    stats.errors.fetch_add(1, Relaxed);
    failed.store(true, Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mock {
        entered: mpsc::Sender<()>,
        release: Option<mpsc::Receiver<()>>,
        fail: bool,
    }

    impl Publisher for Mock {
        fn init(&mut self) -> Result<(), PublishError> {
            Ok(())
        }
        fn publish_batch(
            &mut self,
            _: SimulationUtc,
            _: &[SatelliteState],
        ) -> Result<(), PublishError> {
            let _ = self.entered.send(());
            if let Some(release) = self.release.take() {
                release.recv_timeout(Duration::from_secs(2)).unwrap();
            }
            if self.fail {
                return Err(PublishError::Sink("test sink failure".into()));
            }
            Ok(())
        }
        fn flush(&mut self) -> Result<(), PublishError> {
            Ok(())
        }
        fn shutdown(&mut self) -> Result<(), PublishError> {
            Ok(())
        }
    }

    #[test]
    fn slow_and_failed_publishers_do_not_block_healthy_output() {
        let (entered, slow) = mpsc::channel();
        let (unblock, release) = mpsc::channel();
        let mut blocked = Output::start(
            "slow".into(),
            25.0,
            Mock {
                entered,
                release: Some(release),
                fail: false,
            },
        )
        .unwrap();
        let (entered, broken) = mpsc::channel();
        let mut failed = Output::start(
            "failed".into(),
            25.0,
            Mock {
                entered,
                release: None,
                fail: true,
            },
        )
        .unwrap();
        let (entered, received) = mpsc::channel();
        let mut fast = Output::start(
            "healthy".into(),
            25.0,
            Mock {
                entered,
                release: None,
                fail: false,
            },
        )
        .unwrap();

        let at = SimulationUtc::new("2026-11-17T00:00:00Z".parse().unwrap());
        let wall = Instant::now();
        let states = Arc::new(Vec::new());
        blocked.submit(at, wall, Arc::clone(&states));
        slow.recv_timeout(Duration::from_secs(1)).unwrap();
        failed.submit(at, wall, Arc::clone(&states));
        broken.recv_timeout(Duration::from_secs(1)).unwrap();
        for _ in 0..4 {
            blocked.submit(at, wall, Arc::clone(&states));
            fast.submit(at, wall, Arc::clone(&states));
            received.recv_timeout(Duration::from_secs(1)).unwrap();
        }
        assert_eq!(blocked.stats().dropped_batches.load(Relaxed), 2);
        assert!(!failed.finish());
        assert_eq!(failed.stats().errors.load(Relaxed), 1);
        unblock.send(()).unwrap();
        assert!(blocked.finish());
        assert!(fast.finish());
        assert_eq!(fast.stats().sent_batches.load(Relaxed), 4);
    }
}
