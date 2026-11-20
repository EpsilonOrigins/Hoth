use super::{
    radar, selected_members, vectors, wire::WireTap, write_page, Replay, Timeline, Track, Vectors,
};
use chrono::{DateTime, SecondsFormat, Utc};
use hoth_domain::state::SimulationUtc;
use hoth_orbits::{EarthRotation, Propagator};
use hoth_runtime::{Engine, RunResult};
use hoth_scenario::Manifest;
use hoth_zenoh::{client_config, StateBatch};
use prost::Message;
use serde::Serialize;
use std::{
    collections::BTreeSet,
    io::{Cursor, Read},
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Instant,
};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};
use zenoh::{sample::Sample, Session, Wait};

const CSV_HEADER: &str =
    "utc,norad_id,x_ecef_m,y_ecef_m,z_ecef_m,vx_ecef_m_s,vy_ecef_m_s,vz_ecef_m_s,status\n";

#[derive(Serialize)]
struct Frame {
    utc: DateTime<Utc>,
    states: Vec<Option<Vectors>>,
}

struct Feed {
    _subscriber: zenoh::pubsub::Subscriber<()>,
    _session: Session,
    pending: Arc<Mutex<Option<(Instant, Sample)>>>,
    run_id: String,
    last: Option<(Instant, Frame)>,
}

impl Feed {
    fn connect(router: SocketAddr, run_id: String) -> RunResult<Self> {
        if run_id.is_empty()
            || run_id.len() > 64
            || !run_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(
                "--run-id must have 1 to 64 ASCII letters, digits, hyphens or underscores".into(),
            );
        }
        let config = client_config(router).map_err(|error| error.to_string())?;
        let session = zenoh::open(config)
            .wait()
            .map_err(|error| error.to_string())?;
        let pending = Arc::new(Mutex::new(None));
        let mailbox = Arc::clone(&pending);
        let subscriber = session
            .declare_subscriber(format!("hoth/v1/{run_id}/states"))
            .callback(move |sample| {
                *mailbox.lock().expect("live feed mailbox") = Some((Instant::now(), sample));
            })
            .wait()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            _subscriber: subscriber,
            _session: session,
            pending,
            run_id,
            last: None,
        })
    }

    fn snapshot(&mut self, tracks: &[Track], timeline: &Timeline) -> RunResult<Vec<u8>> {
        let received = self.pending.lock().expect("live feed mailbox").take();
        if let Some((received_at, sample)) = received {
            let payload = sample.payload().to_bytes();
            match StateBatch::decode(payload.as_ref())
                .map_err(|error| error.to_string())
                .and_then(|batch| live_frame(batch, &self.run_id, tracks, timeline))
            {
                Ok(frame) => self.last = Some((received_at, frame)),
                Err(error) => eprintln!("ignored invalid live batch: {error}"),
            }
        }
        #[derive(Serialize)]
        struct Snapshot<'a> {
            age_ms: Option<u64>,
            frame: Option<&'a Frame>,
        }
        let (age_ms, frame) = self
            .last
            .as_ref()
            .map_or((None, None), |(received, frame)| {
                (Some(received.elapsed().as_millis() as u64), Some(frame))
            });
        Ok(serde_json::to_vec(&Snapshot { age_ms, frame })?)
    }
}

fn live_frame(
    batch: StateBatch,
    run_id: &str,
    tracks: &[Track],
    timeline: &Timeline,
) -> Result<Frame, String> {
    if batch.schema_version != 1 || batch.run_id != run_id {
        return Err("unexpected telemetry schema or run ID".into());
    }
    let utc = batch
        .simulation_utc
        .filter(|time| time.nanos < 1_000_000_000)
        .and_then(|time| DateTime::from_timestamp(time.seconds, time.nanos))
        .filter(|time| (timeline.start..=timeline.end).contains(time))
        .ok_or("simulation UTC absent or outside manifest mission")?;
    let mut states = vec![None; tracks.len()];
    for state in batch.states {
        let Ok(index) = tracks.binary_search_by_key(&state.norad_id, |track| track.norad) else {
            continue;
        };
        let p = state.position_ecef_m.ok_or("position absent")?;
        let v = state.velocity_ecef_m_per_s.ok_or("velocity absent")?;
        let value = [p.x, p.y, p.z, v.x, v.y, v.z];
        if !value.iter().all(|axis| axis.is_finite()) || states[index].replace(value).is_some() {
            return Err(format!(
                "invalid or duplicate state for NORAD {}",
                state.norad_id
            ));
        }
    }
    Ok(Frame { utc, states })
}

pub fn serve(
    address: SocketAddr,
    manifest: &Manifest,
    ids: &BTreeSet<u32>,
    step_ms: u64,
    live: Option<(SocketAddr, String)>,
    dis_listen: Option<SocketAddr>,
) -> RunResult<()> {
    let config = manifest.config()?;
    let timeline = Timeline::new(&config, step_ms, live.is_some())?;
    let members = selected_members(manifest, ids)?;
    let propagators = members
        .iter()
        .map(|member| member.propagator())
        .collect::<Result<Vec<_>, _>>()?;
    let rotation = EarthRotation::at(SimulationUtc::new(timeline.start));
    let replay = Replay {
        name: config.name,
        snapshot: config.catalog.snapshot,
        digest: manifest.digest().to_owned(),
        population: manifest.members().len(),
        offsets: Vec::new(),
        times: Vec::new(),
        tracks: propagators
            .iter()
            .map(|propagator| Track {
                norad: propagator.norad_id().get(),
                provenance: propagator
                    .state(rotation)
                    .ok()
                    .map(|state| state.provenance),
                states: Vec::new(),
            })
            .collect(),
        radars: config.radars.iter().map(radar::Overlay::new).collect(),
        wire_monitor: dis_listen.is_some(),
        timeline: Some(timeline),
    };
    let mut html = Vec::new();
    write_page(&mut html, &replay)?;
    let engine = Engine::new(
        members
            .iter()
            .map(|member| member.propagator())
            .collect::<Result<_, _>>()?,
    );
    let mut simulator = Simulator {
        engine,
        buffer: Vec::new(),
    };
    let mut feed = live
        .map(|(address, run)| Feed::connect(address, run))
        .transpose()?;
    let wire = dis_listen
        .map(|ip| WireTap::listen(ip, manifest, config.dis.exercise_id))
        .transpose()?;
    let listener = Server::http(address).map_err(|error| error.to_string())?;
    println!("Globe listening on http://{}/", listener.server_addr());
    let timeline = replay.timeline.as_ref().expect("served replay timeline");
    for request in listener.incoming_requests() {
        if let Err(error) = respond(
            request,
            &html,
            &replay.tracks,
            timeline,
            &propagators,
            &mut simulator,
            (feed.as_mut(), wire.as_ref()),
        ) {
            eprintln!("globe request failed: {error}");
        }
    }
    Ok(())
}

struct Simulator {
    engine: Engine,
    buffer: Vec<hoth_domain::state::SatelliteState>,
}

impl Simulator {
    fn frame(&mut self, utc: DateTime<Utc>, tracks: &[Track]) -> Frame {
        self.engine.batch(SimulationUtc::new(utc), &mut self.buffer);
        let mut states = self.buffer.iter().peekable();
        let states = tracks
            .iter()
            .map(|track| {
                states
                    .next_if(|state| state.norad_id.get() == track.norad)
                    .map(vectors)
            })
            .collect();
        Frame { utc, states }
    }
}

fn respond(
    request: Request,
    html: &[u8],
    tracks: &[Track],
    timeline: &Timeline,
    propagators: &[Propagator],
    simulator: &mut Simulator,
    (feed, wire): (Option<&mut Feed>, Option<&WireTap>),
) -> RunResult<()> {
    let index = |prefix| {
        let norad = argument::<u32>(&request, prefix)?;
        tracks
            .binary_search_by_key(&norad, |track| track.norad)
            .ok()
    };
    if request.method() != &Method::Get {
        request.respond(Response::empty(405))?;
    } else if request.url() == "/" {
        request.respond(
            Response::from_data(html).with_header(content_type("text/html; charset=utf-8")),
        )?;
    } else if request.url() == "/healthz" {
        request.respond(Response::empty(200))?;
    } else if let Some((wire, norad)) = wire.zip(argument(&request, "/api/dis/")) {
        json(request, serde_json::to_vec(&wire.snapshot(norad))?)?;
    } else if let Some(feed) = feed.filter(|_| request.url() == "/api/live") {
        let data = feed.snapshot(tracks, timeline);
        match data {
            Ok(data) => json(request, data)?,
            Err(error) => {
                request.respond(Response::empty(503))?;
                return Err(error);
            }
        }
    } else if let Some(index) = argument::<u64>(&request, "/api/frame/")
        .filter(|&index| !timeline.live && index <= timeline.last)
    {
        let frame = simulator.frame(timeline.time(index), tracks);
        json(request, serde_json::to_vec(&frame)?)?;
    } else if let Some(i) = index("/api/track/") {
        let stride = timeline.last.div_ceil(2399).max(1);
        let points: Vec<_> = (0..=timeline.last)
            .step_by(stride as usize)
            .take_while(|&index| index < timeline.last)
            .chain(std::iter::once(timeline.last))
            .map(|index| state(&propagators[i], timeline.time(index)).map(|v| [v[0], v[1], v[2]]))
            .collect();
        json(request, serde_json::to_vec(&points)?)?;
    } else if let Some(i) = index("/api/csv/").filter(|_| !timeline.live) {
        let norad = tracks[i].norad;
        let rows = Csv {
            propagator: &propagators[i],
            timeline,
            index: 0,
            row: Cursor::new(CSV_HEADER.as_bytes().to_vec()),
        };
        let disposition = Header::from_bytes(
            "Content-Disposition",
            format!("attachment; filename=hoth-{norad}-ecef.csv"),
        )
        .expect("numeric attachment filename");
        request.respond(Response::new(
            StatusCode(200),
            vec![content_type("text/csv; charset=utf-8"), disposition],
            rows,
            None,
            None,
        ))?;
    } else {
        request.respond(Response::empty(404))?;
    }
    Ok(())
}

fn argument<T: std::str::FromStr>(request: &Request, prefix: &str) -> Option<T> {
    request.url().strip_prefix(prefix)?.parse().ok()
}

fn content_type(value: &str) -> Header {
    Header::from_bytes("Content-Type", value).expect("fixed content type")
}

fn json(request: Request, data: Vec<u8>) -> RunResult<()> {
    request.respond(
        Response::from_data(data)
            .with_header(content_type("application/json"))
            .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap()),
    )?;
    Ok(())
}

fn state(propagator: &Propagator, utc: DateTime<Utc>) -> Option<Vectors> {
    let rotation = EarthRotation::at(SimulationUtc::new(utc));
    propagator.state(rotation).ok().as_ref().map(vectors)
}

struct Csv<'a> {
    propagator: &'a Propagator,
    timeline: &'a Timeline,
    index: u64,
    row: Cursor<Vec<u8>>,
}

impl Read for Csv<'_> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if self.row.position() == self.row.get_ref().len() as u64
            && self.index <= self.timeline.last
        {
            let utc = self.timeline.time(self.index);
            self.index += 1;
            let id = self.propagator.norad_id().get();
            let timestamp = utc.to_rfc3339_opts(SecondsFormat::AutoSi, true);
            let row = match state(self.propagator, utc) {
                Some([x, y, z, vx, vy, vz]) => {
                    format!("{timestamp},{id},{x},{y},{z},{vx},{vy},{vz},ok\n")
                }
                None => format!("{timestamp},{id},,,,,,,unavailable\n"),
            };
            self.row = Cursor::new(row.into_bytes());
        }
        self.row.read(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::fixture;

    #[test]
    fn live_frames_preserve_exact_utc_and_allow_backward_seeks() {
        let manifest = fixture();
        let config = manifest.config().unwrap();
        let timeline = Timeline::new(&config, 40, true).unwrap();
        let tracks = [Track {
            norad: 25544,
            provenance: None,
            states: Vec::new(),
        }];
        let engine = Engine::load(&manifest).unwrap();
        let mut states = Vec::new();
        for index in [1, 2, 0] {
            let utc = timeline.time(index);
            let instant = SimulationUtc::new(utc);
            assert_eq!(engine.batch(instant, &mut states), 0);
            let batch = StateBatch::from_states("fixture", instant, &states).unwrap();
            assert!(live_frame(batch.clone(), "other", &tracks, &timeline).is_err());
            let frame = live_frame(batch.clone(), "fixture", &tracks, &timeline).unwrap();
            assert_eq!(frame.utc, utc);
            assert_eq!(frame.states, [Some(vectors(&states[0]))]);
            let mut duplicate = batch;
            duplicate.states.push(duplicate.states[0].clone());
            assert!(live_frame(duplicate, "fixture", &tracks, &timeline).is_err());
        }
    }
}
