//! Export sampled HOTH canonical states as a self-contained globe replay.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{SecondsFormat, TimeDelta};
use hoth_domain::state::{SimulationUtc, StateProvenance};
use hoth_runtime::{Engine, RunResult};
use hoth_scenario::Manifest;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const MAX_POINTS: u64 = 500_000;
const DEFAULT_STEP_SECONDS: u64 = 15;
const USAGE: &str =
    "Usage: orbit_globe --manifest PATH --output PATH [--step-seconds 15] [--norad ID ...]";

#[derive(Serialize)]
struct Replay {
    name: String,
    snapshot: String,
    digest: String,
    population: usize,
    offsets: Vec<u64>,
    times: Vec<String>,
    tracks: Vec<Track>,
}

#[derive(Serialize)]
struct Track {
    norad: u32,
    provenance: Option<StateProvenance>,
    // ECEF [x, y, z, vx, vy, vz] in meters and meters per second.
    states: Vec<Option<[f64; 6]>>,
}

fn sample_mission(manifest: &Manifest, step: u64, ids: &BTreeSet<u32>) -> RunResult<Replay> {
    let config = manifest.config()?;
    let duration = config.time.duration_seconds.get();
    if step == 0 {
        return Err("--step-seconds must be positive".into());
    }
    let frame_count = duration.div_ceil(step) + 1;
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
    let point_count = frame_count.saturating_mul(selected.len() as u64);
    if point_count > MAX_POINTS {
        return Err(format!(
            "export requires {point_count} points (limit {MAX_POINTS}); increase --step-seconds or select satellites with --norad"
        ).into());
    }
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
            let [x, y, z] = state.ecef.position_m.xyz();
            let [vx, vy, vz] = state.ecef.velocity_m_per_s.xyz();
            track.states[frame as usize] = Some([x, y, z, vx, vy, vz]);
            track
                .provenance
                .get_or_insert_with(|| state.provenance.clone());
        }
    }
    Ok(Replay {
        name: config.name,
        snapshot: config.catalog.snapshot,
        digest: manifest.digest().to_owned(),
        population: members.len(),
        offsets,
        times,
        tracks,
    })
}

fn write_replay(output: &Path, replay: &Replay) -> RunResult<()> {
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
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
        "<script>{}</script>\n</body>\n</html>",
        include_str!("viewer.js")
    )?;
    file.persist_noclobber(output)
        .map_err(|error| format!("write {}: {}", output.display(), error.error))?;
    Ok(())
}

fn run() -> RunResult<()> {
    let mut manifest = None;
    let mut output = None;
    let mut step = None;
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
            Some("--step-seconds") if step.is_none() => {
                step = Some(value.to_str().ok_or("invalid --step-seconds")?.parse()?);
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
    let output = output.ok_or("provide --output PATH")?;
    let manifest = Manifest::decode(&fs::read(manifest)?)?;
    let replay = sample_mission(&manifest, step.unwrap_or(DEFAULT_STEP_SECONDS), &norad)?;
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

    fn fixture() -> Manifest {
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
        let replay = sample_mission(&manifest, 15, &BTreeSet::new()).unwrap();
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
        assert!(sample_mission(&manifest, 15, &BTreeSet::from([9])).is_err());
        assert!(sample_mission(&manifest, 0, &BTreeSet::new()).is_err());
    }
}
