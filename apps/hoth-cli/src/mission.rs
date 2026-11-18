use hoth_catalog::offline;
use hoth_config::{ClockMode, PublisherKind};
use hoth_dis::{Registry, UdpPublisher};
use hoth_fabric_adapter::Controller;
use hoth_runtime::{Command, Lifecycle, Output, RunResult, Runner};
use hoth_scenario::Manifest;
use hoth_zenoh::ZenohPublisher;
use std::{
    collections::{BTreeMap, HashMap},
    ffi::OsString,
    fs,
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering::Relaxed},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

pub fn prepare(args: &[OsString]) -> RunResult<()> {
    let allowed = ["--config", "--database", "--output", "--entity-types"];
    let options = Options::parse(args, &allowed)?;
    let database = options
        .optional_path("--database")
        .unwrap_or(Path::new("data/hoth.db"));
    let db = offline::open_read_only(database)?;
    let registry_file = options
        .optional_path("--entity-types")
        .map(fs::read_to_string)
        .transpose()?;
    let registry = Registry::from_yaml(
        registry_file
            .as_deref()
            .unwrap_or(include_str!("../../../config/dis/entity_types.yaml")),
    )?;
    let yaml = fs::read_to_string(options.path("--config")?)?;
    let manifest = Manifest::prepare(&db, yaml, &registry)?;
    let output = options.path("--output")?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(file.as_file_mut(), &manifest)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    file.persist_noclobber(output)?;
    println!(
        "Prepared {} members, {} rejected, SHA-256 {}",
        manifest.members().len(),
        manifest.rejected().len(),
        manifest.digest()
    );
    for (id, reason) in manifest.rejected() {
        eprintln!("Excluded NORAD {id}: {reason}");
    }
    Ok(())
}

pub fn run(args: &[OsString]) -> RunResult<()> {
    let allowed = ["--manifest", "--control", "--run-id", "--ready-file"];
    let options = Options::parse(args, &allowed)?;
    let manifest = Manifest::decode(&fs::read(options.path("--manifest")?)?)?;
    let config = manifest.config()?;
    let run_id = options
        .text("--run-id")?
        .unwrap_or(&manifest.digest()[..16]);
    if run_id.is_empty()
        || run_id.len() > 64
        || !run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(
            "--run-id must have 1 to 64 ASCII letters, digits, hyphens or underscores".into(),
        );
    }
    let mut control = options
        .text("--control")?
        .map(|value| {
            let address: SocketAddr = value.parse()?;
            Controller::bind(address, run_id.into()).map_err(Box::<dyn std::error::Error>::from)
        })
        .transpose()?;
    if config.time.clock == ClockMode::External && control.is_none() {
        return Err("external mode requires --control HOST:PORT".into());
    }

    let stopped = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&stopped);
    ctrlc::set_handler(move || signal.store(true, Relaxed))?;
    let entities: BTreeMap<_, _> = manifest
        .members()
        .iter()
        .map(|m| (m.norad_id(), m.entity()))
        .collect();
    let mut outputs = Vec::new();
    for publisher in config.publishers {
        let rate = publisher.rate_hz;
        let destination = publisher.destination;
        let output = match publisher.kind {
            PublisherKind::Dis => Output::start(
                format!("dis/{destination}"),
                rate,
                UdpPublisher::new(
                    destination,
                    publisher.multicast,
                    config.dis.exercise_id,
                    entities.clone(),
                ),
            )?,
            PublisherKind::Zenoh => Output::start(
                format!("zenoh/{destination}"),
                rate,
                ZenohPublisher::new(destination, run_id.into()),
            )?,
        };
        outputs.push(output);
    }
    let mut runner = Runner::new(&manifest, outputs)?;
    let mut ready = options
        .optional_path("--ready-file")
        .map(ReadyFile::create)
        .transpose()?;
    if let Some(control) = &control {
        println!("READY run_id={run_id} control={}", control.local_addr()?);
    } else {
        println!("READY run_id={run_id}");
    }
    std::io::stdout().flush()?;

    let execution = (|| -> RunResult<()> {
        if config.time.clock == ClockMode::Standalone {
            runner.command(Command::Start)?;
        }
        while runner.lifecycle() != Lifecycle::Stopped && !stopped.load(Relaxed) {
            if let Some(control) = &mut control {
                control.poll(&mut runner)?;
            }
            if let Some(ready) = &mut ready {
                ready.refresh()?;
            }
            let delay = runner.step()?;
            if runner.lifecycle() != Lifecycle::Stopped {
                thread::sleep(delay);
            }
        }
        Ok(())
    })();
    drop(ready);
    let ended = runner.finish();
    execution.and(ended)
}

pub fn health(args: &[OsString]) -> RunResult<()> {
    let options = Options::parse(args, &["--ready-file"])?;
    let file = options.path("--ready-file")?;
    if fs::metadata(file)?.modified()?.elapsed()? > Duration::from_secs(5) {
        return Err("runtime heartbeat is stale".into());
    }
    Ok(())
}

struct ReadyFile {
    path: PathBuf,
    refreshed: Instant,
}

impl ReadyFile {
    fn create(path: &Path) -> std::io::Result<Self> {
        fs::write(path, b"ready\n")?;
        Ok(Self {
            path: path.into(),
            refreshed: Instant::now(),
        })
    }

    fn refresh(&mut self) -> std::io::Result<()> {
        if self.refreshed.elapsed() >= Duration::from_secs(1) {
            fs::write(&self.path, b"ready\n")?;
            self.refreshed = Instant::now();
        }
        Ok(())
    }
}

impl Drop for ReadyFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

struct Options<'a>(HashMap<&'a str, &'a OsString>);

impl<'a> Options<'a> {
    fn parse(args: &'a [OsString], allowed: &[&str]) -> RunResult<Self> {
        let mut options = HashMap::new();
        let (pairs, remainder) = args.as_chunks::<2>();
        for pair in pairs {
            let option = pair[0].to_str().ok_or("invalid option")?;
            if !allowed.contains(&option) || options.insert(option, &pair[1]).is_some() {
                return Err(format!("unknown or repeated option {option}").into());
            }
        }
        if !remainder.is_empty() {
            return Err("option requires a value. Run 'hoth --help' for usage".into());
        }
        Ok(Self(options))
    }

    fn optional_path(&self, key: &str) -> Option<&Path> {
        self.0.get(key).map(Path::new)
    }

    fn path(&self, key: &str) -> RunResult<&Path> {
        self.optional_path(key)
            .ok_or_else(|| format!("provide {key}. Run 'hoth --help' for usage").into())
    }

    fn text(&self, key: &str) -> RunResult<Option<&str>> {
        self.0
            .get(key)
            .map(|arg| {
                arg.to_str()
                    .ok_or_else(|| format!("{key} requires UTF-8").into())
            })
            .transpose()
    }
}
