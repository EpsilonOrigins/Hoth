//! Manifest-only propagation, lifecycle and publisher dispatch.

mod output;

use hoth_clock::{Clock, ClockEvent, ClockMode, ClockRate, SimulationClock};
use hoth_domain::state::{SatelliteState, SimulationUtc};
use hoth_orbits::{EarthRotation, Propagator};
use hoth_scenario::Manifest;
pub use output::{Output, OutputStats};
use std::{
    error::Error,
    sync::Arc,
    time::{Duration, Instant},
};

pub type RunResult<T> = Result<T, Box<dyn Error>>;

pub struct Engine {
    propagators: Vec<Propagator>,
}

impl Engine {
    pub fn load(manifest: &Manifest) -> RunResult<Self> {
        Ok(Self {
            propagators: manifest
                .members()
                .iter()
                .map(|member| member.propagator())
                .collect::<Result<_, _>>()?,
        })
    }

    pub fn new(propagators: Vec<Propagator>) -> Self {
        Self { propagators }
    }

    pub fn batch(&self, at: SimulationUtc, states: &mut Vec<SatelliteState>) -> usize {
        states.clear();
        let mut failures = 0;
        let rotation = EarthRotation::at(at);
        for propagator in &self.propagators {
            match propagator.state(rotation) {
                Ok(state) => states.push(state),
                Err(error) => {
                    if failures == 0 {
                        eprintln!(
                            "propagation at {} failed for NORAD {}: {error}",
                            at.instant(),
                            propagator.norad_id().get()
                        );
                    }
                    failures += 1;
                }
            }
        }
        failures
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    Ready,
    Running,
    Paused,
    Stopped,
}

impl Lifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Stopped => "stopped",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Start,
    Pause,
    Resume,
    Seek(SimulationUtc),
    Tick(SimulationUtc),
    Stop,
}

pub struct Runner {
    clock: Clock,
    start: SimulationUtc,
    end: SimulationUtc,
    lifecycle: Lifecycle,
    engine: Engine,
    outputs: Vec<Output>,
    reusable: Option<Arc<Vec<SatelliteState>>>,
    pending: bool,
    last_time: Option<SimulationUtc>,
    ticks: u64,
    states: u64,
    failed_states: u64,
}

impl Runner {
    pub fn new(manifest: &Manifest, outputs: Vec<Output>) -> RunResult<Self> {
        let config = manifest.config()?;
        let time = config.time.clone();
        let clock = Clock::new(
            time.clock,
            time.start_utc,
            ClockRate::new(time.rate).ok_or("invalid clock rate")?,
        );
        Ok(Self {
            clock,
            start: time.start_utc,
            end: config.end_utc(),
            lifecycle: Lifecycle::Ready,
            engine: Engine::load(manifest)?,
            outputs,
            reusable: None,
            pending: false,
            last_time: None,
            ticks: 0,
            states: 0,
            failed_states: 0,
        })
    }

    pub fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }
    pub fn at(&self) -> RunResult<SimulationUtc> {
        Ok(self.clock.now_utc()?.min(self.end))
    }

    pub fn command(&mut self, command: Command) -> RunResult<()> {
        if self.lifecycle == Lifecycle::Stopped && command != Command::Stop {
            return Err("run has stopped".into());
        }
        match command {
            Command::Start if self.lifecycle == Lifecycle::Ready => {
                self.clock.apply(ClockEvent::Seek(self.start))?;
                self.lifecycle = Lifecycle::Running;
                self.pending = true;
            }
            Command::Start if self.lifecycle == Lifecycle::Running => {}
            Command::Pause if matches!(self.lifecycle, Lifecycle::Running | Lifecycle::Paused) => {
                self.clock.apply(ClockEvent::Pause)?;
                self.lifecycle = Lifecycle::Paused;
            }
            Command::Resume if self.lifecycle == Lifecycle::Paused => {
                self.clock.apply(ClockEvent::Resume)?;
                self.lifecycle = Lifecycle::Running;
                self.pending = true;
            }
            Command::Resume if self.lifecycle == Lifecycle::Running => {}
            Command::Seek(at)
                if matches!(self.lifecycle, Lifecycle::Running | Lifecycle::Paused) =>
            {
                self.check_range(at)?;
                self.clock.apply(ClockEvent::Seek(at))?;
                self.pending = true;
                self.last_time = None;
                for output in &mut self.outputs {
                    output.reset_schedule();
                }
            }
            Command::Tick(at) if self.lifecycle == Lifecycle::Running => {
                self.check_range(at)?;
                self.clock.apply(ClockEvent::ExternalTick(at))?;
            }
            Command::Stop => self.lifecycle = Lifecycle::Stopped,
            _ => {
                return Err(
                    format!("{command:?} is invalid while {}", self.lifecycle.as_str()).into(),
                )
            }
        }
        Ok(())
    }

    fn check_range(&self, at: SimulationUtc) -> RunResult<()> {
        if at < self.start || at > self.end {
            return Err(format!(
                "UTC must be between {} and {}",
                self.start.instant(),
                self.end.instant()
            )
            .into());
        }
        Ok(())
    }

    /// Poll at idle or after accepted control. Both sinks can be due for one batch.
    pub fn step(&mut self) -> RunResult<Duration> {
        let idle = Duration::from_millis(10);
        if self.lifecycle != Lifecycle::Running {
            return Ok(idle);
        }
        let at = if self.pending && self.last_time.is_none() && self.ticks == 0 {
            // The first standalone update is anchored exactly to the configured start.
            self.clock.now_utc().map(|at| {
                if self.clock.mode() == ClockMode::Standalone {
                    self.start
                } else {
                    at
                }
            })?
        } else {
            self.at()?
        };
        let mode = self.clock.mode();
        let wall = Instant::now();
        let finished = at == self.end;
        let timestamp_changed = self.last_time != Some(at);
        let should_run = self.pending
            || (timestamp_changed
                && (finished || self.outputs.iter().any(|output| output.due(mode, at, wall))));
        if should_run {
            let mut states = self
                .reusable
                .take()
                .and_then(Arc::into_inner)
                .unwrap_or_default();
            self.failed_states += self.engine.batch(at, &mut states) as u64;
            if states.is_empty() {
                return Err("all selected objects failed propagation".into());
            }
            self.ticks += 1;
            self.states += states.len() as u64;
            let states = Arc::new(states);
            for output in &mut self.outputs {
                if self.pending || finished || output.due(mode, at, wall) {
                    output.submit(at, wall, Arc::clone(&states));
                }
            }
            self.reusable = Some(states);
            self.last_time = Some(at);
            self.pending = false;
        }
        if finished {
            self.lifecycle = Lifecycle::Stopped;
        } else if self.outputs.iter().all(Output::failed) {
            return Err("all publishers have failed".into());
        }
        Ok(if mode == ClockMode::Standalone {
            self.outputs
                .iter()
                .filter_map(|o| o.wait(wall))
                .min()
                .unwrap_or(idle)
                .min(idle)
        } else {
            idle
        })
    }

    pub fn finish(mut self) -> RunResult<()> {
        eprintln!(
            "runtime ticks={} states={} propagation_errors={}",
            self.ticks, self.states, self.failed_states
        );
        let mut successful = false;
        for output in &mut self.outputs {
            successful |= output.finish();
        }
        if !successful {
            return Err("no publisher completed cleanly".into());
        }
        Ok(())
    }
}
