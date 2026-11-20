//! Export or serve HOTH canonical states for the globe.

mod radar;
mod server;
mod wire;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use hoth_domain::state::{SimulationUtc, StateProvenance};
use hoth_runtime::{Engine, RunResult};
use hoth_scenario::{Manifest, Member};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
};

const MAX_POINTS: u64 = 500_000;
const DEFAULT_STEP_SECONDS: u64 = 15;
const USAGE: &str = "Usage: orbit_globe --manifest PATH (--output PATH | --serve IP:PORT) [--step-seconds SECONDS] [--norad ID ...]
       With --serve, use --step-ms MILLISECONDS for subsecond replay or --zenoh IP:PORT --run-id ID for live telemetry.
       Add --dis-listen IP:PORT to inspect DIS 7 Entity State UDP datagrams.";

type Vectors = [f64; 6];

#[derive(Serialize)]
struct Replay {
    name: String,
    snapshot: String,
    digest: String,
    population: usize,
    offsets: Vec<u64>,
    times: Vec<String>,
    tracks: Vec<Track>,
    radars: Vec<radar::Overlay>,
    wire_monitor: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeline: Option<Timeline>,
}

#[derive(Serialize)]
struct Track {
    norad: u32,
    provenance: Option<StateProvenance>,
    // ECEF [x, y, z, vx, vy, vz] in meters and meters per second.
    states: Vec<Option<Vectors>>,
}

#[derive(Serialize)]
struct Timeline {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    duration_ms: u64,
    step_ms: u64,
    last: u64,
    live: bool,
}

impl Timeline {
    fn new(config: &hoth_config::ScenarioConfig, step_ms: u64, live: bool) -> RunResult<Self> {
        if step_ms == 0 {
            return Err("sample interval must be positive".into());
        }
        let duration_ms = config
            .time
            .duration_seconds
            .get()
            .checked_mul(1000)
            .filter(|&duration| duration < (1_u64 << 53))
            .ok_or("mission duration exceeds the browser's millisecond timeline range")?;
        Ok(Self {
            start: config.time.start_utc.instant(),
            end: config.end_utc().instant(),
            duration_ms,
            step_ms: step_ms.min(duration_ms),
            last: duration_ms.div_ceil(step_ms),
            live,
        })
    }

    fn time(&self, frame: u64) -> DateTime<Utc> {
        let offset = frame.saturating_mul(self.step_ms).min(self.duration_ms);
        self.start + TimeDelta::milliseconds(offset as i64)
    }
}

fn sampling_plan(
    duration: u64,
    requested_step: Option<u64>,
    satellite_count: u64,
) -> RunResult<(u64, u64)> {
    // Reserve slots for both endpoints. Increase the default interval before
    // generating any states so large missions can preserve their membership.
    let max_intervals = (MAX_POINTS / satellite_count).saturating_sub(1).max(1);
    let step = requested_step.unwrap_or(DEFAULT_STEP_SECONDS.max(duration.div_ceil(max_intervals)));
    if step == 0 {
        return Err("--step-seconds must be positive".into());
    }
    let frame_count = duration.div_ceil(step) + 1;
    let point_count = frame_count.saturating_mul(satellite_count);
    if point_count > MAX_POINTS {
        return Err(format!(
            "export requires {point_count} points (limit {MAX_POINTS}); omit --step-seconds for automatic sampling, increase it, or select satellites with --norad"
        ).into());
    }
    Ok((step, frame_count))
}

fn selected_members<'a>(manifest: &'a Manifest, ids: &BTreeSet<u32>) -> RunResult<Vec<&'a Member>> {
    let members = manifest.members();
    let selected: Vec<_> = members
        .iter()
        .filter(|member| ids.is_empty() || ids.contains(&member.norad_id()))
        .collect();
    if !ids.is_empty() && selected.len() != ids.len() {
        let missing: Vec<_> = ids
            .iter()
            .filter(|&&id| !members.iter().any(|member| member.norad_id() == id))
            .copied()
            .collect();
        return Err(format!("NORAD IDs absent from manifest: {missing:?}").into());
    }
    Ok(selected)
}

fn sample_mission(
    manifest: &Manifest,
    requested_step: Option<u64>,
    ids: &BTreeSet<u32>,
) -> RunResult<Replay> {
    let config = manifest.config()?;
    let duration = config.time.duration_seconds.get();
    let selected = selected_members(manifest, ids)?;
    let (step, frame_count) = sampling_plan(duration, requested_step, selected.len() as u64)?;
    let engine = Engine::new(
        selected
            .iter()
            .map(|member| member.propagator())
            .collect::<Result<_, _>>()?,
    );
    let mut tracks: Vec<_> = selected
        .iter()
        .map(|member| Track {
            norad: member.norad_id(),
            provenance: None,
            states: vec![None; frame_count as usize],
        })
        .collect();
    let track_indices: BTreeMap<_, _> = tracks
        .iter()
        .enumerate()
        .map(|(index, track)| (track.norad, index))
        .collect();

    let mut offsets = Vec::with_capacity(frame_count as usize);
    let mut times = Vec::with_capacity(frame_count as usize);
    let mut states = Vec::with_capacity(tracks.len());
    for frame in 0..frame_count {
        let offset = step.saturating_mul(frame).min(duration);
        let utc = config.time.start_utc.instant() + TimeDelta::seconds(offset as i64);
        offsets.push(offset);
        times.push(utc.to_rfc3339_opts(SecondsFormat::AutoSi, true));
        engine.batch(SimulationUtc::new(utc), &mut states);
        for state in &states {
            let track = &mut tracks[track_indices[&state.norad_id.get()]];
            track.states[frame as usize] = Some(vectors(state));
            track
                .provenance
                .get_or_insert_with(|| state.provenance.clone());
        }
    }
    Ok(Replay {
        name: config.name,
        snapshot: config.catalog.snapshot,
        digest: manifest.digest().to_owned(),
        population: manifest.members().len(),
        offsets,
        times,
        tracks,
        radars: config.radars.iter().map(radar::Overlay::new).collect(),
        wire_monitor: false,
        timeline: None,
    })
}

fn vectors(state: &hoth_domain::state::SatelliteState) -> Vectors {
    let [x, y, z] = state.ecef.position_m.xyz();
    let [vx, vy, vz] = state.ecef.velocity_m_per_s.xyz();
    [x, y, z, vx, vy, vz]
}

fn write_replay(output: &Path, replay: &Replay) -> RunResult<()> {
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    write_page(&mut file, replay)?;
    file.persist_noclobber(output)
        .map_err(|error| format!("write {}: {}", output.display(), error.error))?;
    Ok(())
}

fn write_page(mut file: impl Write, replay: &Replay) -> RunResult<()> {
    // Prevent a scenario name or provenance field from closing the JSON script.
    let data = serde_json::to_string(replay)?.replace('<', "\\u003c");
    let earth = STANDARD.encode(include_bytes!("earth.jpg"));
    let html = include_str!("viewer.html");
    let (head, body) = html
        .split_once("<!-- ASSETS -->")
        .expect("viewer template asset marker");
    file.write_all(head.as_bytes())?;
    writeln!(file, "<style>{}</style>", include_str!("viewer.css"))?;
    file.write_all(body.as_bytes())?;
    writeln!(file, "<!--\n{}-->", include_str!("earth.LICENSE"))?;
    writeln!(
        file,
        "<script id=\"mission-data\" type=\"application/json\">{data}</script>"
    )?;
    writeln!(
        file,
        "<img id=\"earth-texture\" alt=\"\" hidden src=\"data:image/jpeg;base64,{earth}\">"
    )?;
    writeln!(
        file,
        "<script>{}\n{}\n{}</script>\n</body>\n</html>",
        include_str!("reference.js"),
        include_str!("viewer.js"),
        include_str!("wire.js"),
    )?;
    Ok(())
}

fn run() -> RunResult<()> {
    let mut manifest = None;
    let mut output = None;
    let mut address: Option<SocketAddr> = None;
    let mut router: Option<SocketAddr> = None;
    let mut dis_listen: Option<SocketAddr> = None;
    let mut run_id = None;
    let mut step: Option<u64> = None;
    let mut step_ms: Option<u64> = None;
    let mut norad = BTreeSet::new();
    let mut args = std::env::args_os().skip(1);
    while let Some(flag) = args.next() {
        if flag == "--help" || flag == "-h" {
            println!("{USAGE}");
            return Ok(());
        }
        let value = args.next().ok_or("each option needs a value")?;
        match flag.to_str() {
            Some("--manifest") if manifest.is_none() => manifest = Some(PathBuf::from(value)),
            Some("--output") if output.is_none() => output = Some(PathBuf::from(value)),
            Some("--serve") if address.is_none() => {
                address = Some(value.to_str().ok_or("invalid --serve address")?.parse()?);
            }
            Some("--zenoh") if router.is_none() => {
                router = Some(value.to_str().ok_or("invalid --zenoh address")?.parse()?);
            }
            Some("--dis-listen") if dis_listen.is_none() => {
                dis_listen = Some(
                    value
                        .to_str()
                        .ok_or("invalid --dis-listen address")?
                        .parse()?,
                );
            }
            Some("--run-id") if run_id.is_none() => {
                run_id = Some(value.into_string().map_err(|_| "invalid --run-id")?);
            }
            Some("--step-seconds") if step.is_none() => {
                step = Some(value.to_str().ok_or("invalid --step-seconds")?.parse()?);
            }
            Some("--step-ms") if step_ms.is_none() => {
                step_ms = Some(value.to_str().ok_or("invalid --step-ms")?.parse()?);
            }
            Some("--norad") => {
                let id = value.to_str().ok_or("invalid NORAD ID")?.parse()?;
                if id == 0 || !norad.insert(id) {
                    return Err("--norad requires distinct positive IDs".into());
                }
            }
            _ => {
                return Err(format!("unknown or repeated option {}", flag.to_string_lossy()).into())
            }
        }
    }
    let manifest = manifest.ok_or("provide --manifest PATH")?;
    if output.is_some() == address.is_some() {
        return Err("provide exactly one of --output PATH or --serve IP:PORT".into());
    }
    if step_ms.is_some() && (step.is_some() || address.is_none()) {
        return Err("--step-ms requires --serve and replaces --step-seconds".into());
    }
    if dis_listen.is_some() && address.is_none() {
        return Err("--dis-listen requires --serve".into());
    }
    let live = match (router, run_id) {
        (Some(router), Some(run_id)) if address.is_some() => Some((router, run_id)),
        (None, None) => None,
        _ => return Err("--zenoh and --run-id must be provided together with --serve".into()),
    };
    let manifest = Manifest::decode(&fs::read(manifest)?)?;
    if let Some(address) = address {
        let interval = step_ms.unwrap_or_else(|| step.unwrap_or(1).saturating_mul(1000));
        return server::serve(address, &manifest, &norad, interval, live, dis_listen);
    }
    let output = output.expect("output was validated");
    let replay = sample_mission(&manifest, step, &norad)?;
    write_replay(&output, &replay)?;
    println!(
        "Wrote {} ({} satellites, {} samples each)",
        output.display(),
        replay.tracks.len(),
        replay.times.len()
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}\n{USAGE}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hoth_catalog::{offline, schema};
    use hoth_dis::Registry;

    pub(super) fn fixture() -> Manifest {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        schema::migrate(&mut db).unwrap();
        offline::import_elements(
            &mut db,
            "omm-json",
            include_bytes!("../../../../crates/hoth-catalog/tests/fixtures/orbits/iss.omm.json"),
            "fixture",
            "2026-11-17T00:00:00Z",
        )
        .unwrap();
        offline::materialize(&mut db, "2008-09", 17).unwrap();
        let config = include_str!("../../../../tests/fixtures/scenarios/standalone.yaml")
            .replace("2021-11", "2008-09")
            .replace("duration_seconds: 3", "duration_seconds: 41");
        let registry =
            Registry::from_yaml(include_str!("../../../../config/dis/entity_types.yaml")).unwrap();
        Manifest::prepare(&db, config, &registry).unwrap()
    }

    #[test]
    fn samples_match_runtime_and_include_exact_end() {
        let manifest = fixture();
        let replay = sample_mission(&manifest, None, &BTreeSet::new()).unwrap();
        assert_eq!(replay.offsets, [0, 15, 30, 41]);
        let engine = Engine::load(&manifest).unwrap();
        assert_eq!(replay.tracks[0].norad, 25544);
        let mut states = Vec::new();
        for (index, time) in replay.times.iter().enumerate() {
            let at = SimulationUtc::new(time.parse().unwrap());
            assert_eq!(engine.batch(at, &mut states), 0);
            let values = replay.tracks[0].states[index].unwrap();
            assert_eq!(values[..3], states[0].ecef.position_m.xyz());
            assert_eq!(values[3..], states[0].ecef.velocity_m_per_s.xyz());
            assert_eq!(
                replay.tracks[0].provenance.as_ref(),
                Some(&states[0].provenance)
            );
        }
        assert!(sample_mission(&manifest, Some(15), &BTreeSet::from([9])).is_err());
        assert!(sample_mission(&manifest, Some(0), &BTreeSet::new()).is_err());
    }

    #[test]
    fn automatic_sampling_keeps_large_populations_within_budget() {
        assert_eq!(sampling_plan(5700, None, 100).unwrap(), (15, 381));
        assert_eq!(sampling_plan(5700, None, 15_000).unwrap(), (179, 33));
        assert_eq!(sampling_plan(5700, None, 50_000).unwrap(), (634, 10));
        assert_eq!(sampling_plan(5700, Some(10), 100).unwrap(), (10, 571));
        let error = sampling_plan(5700, Some(15), 15_000).unwrap_err();
        assert!(error.to_string().contains("limit 500000"));
    }

    #[test]
    fn served_timeline_keeps_subsecond_steps_and_exact_endpoint() {
        let config = fixture().config().unwrap();
        let timeline = Timeline::new(&config, 60, false).unwrap();
        assert_eq!(timeline.last, 684);
        assert_eq!(
            timeline.time(1) - timeline.start,
            TimeDelta::milliseconds(60)
        );
        assert_eq!((timeline.end - timeline.time(683)).num_milliseconds(), 20);
        assert_eq!(timeline.time(684), config.end_utc().instant());
        assert!(Timeline::new(&config, 0, false).is_err());
    }
}
