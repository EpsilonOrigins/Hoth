//! Compose verifier with a protocol-independent DIS decoder and mock ECS client.

use chrono::{DateTime, TimeDelta, Utc};
use hoth_zenoh::StateBatch;
use prost::Message;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    error::Error,
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, TcpStream, UdpSocket},
    thread,
    time::{Duration, Instant},
};
use zenoh::Wait;

const COUNT: u32 = 100;

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [udp, control, router]: [String; 3] = args
        .try_into()
        .map_err(|_| "usage: hoth-verify UDP_BIND CONTROL_IP:PORT ZENOH_IP:PORT")?;
    let control: SocketAddr = control.parse()?;
    let udp = UdpSocket::bind(udp)?;
    udp.set_read_timeout(Some(Duration::from_secs(3)))?;
    let session = zenoh::open(hoth_zenoh::client_config(router.parse()?)?).wait()?;
    let subscriber = session
        .declare_subscriber("hoth/v1/compose/states")
        .wait()?;
    // Router receipt of the subscriber declaration is asynchronous. The first
    // control request is sent only after both adapters have initialized.
    thread::sleep(Duration::from_millis(300));
    let start: DateTime<Utc> = "2026-11-17T00:00:00Z".parse()?;
    let read = |at| {
        let batch = subscriber
            .recv_timeout(Duration::from_secs(3))?
            .ok_or("missing Protobuf batch")?;
        let batch = StateBatch::decode(batch.payload().to_bytes().as_ref())?;
        let pdus = read_batch(&udp, &batch, at)?;
        Ok::<_, Box<dyn Error + Send + Sync>>((batch, pdus))
    };

    control_request(control, 2, "start", None, false, "ready")?;
    let ack = control_request(control, 1, "start", None, true, "running")?;
    assert_eq!(
        control_request(control, 1, "start", None, true, "running")?,
        ack
    );
    let first = read(start)?;
    let tick_start = Instant::now();
    for step in 1..=12 {
        let millis = step * 40;
        thread::sleep(
            (tick_start + Duration::from_millis(millis as u64))
                .saturating_duration_since(Instant::now()),
        );
        let tick = start + TimeDelta::milliseconds(millis);
        control_request(
            control,
            (step + 1) as u64,
            "tick",
            Some(tick),
            true,
            "running",
        )?;
        let next = read(tick)?;
        assert_ne!(next.1[&70_000], first.1[&70_000]);
    }
    control_request(control, 14, "tick", Some(start), false, "running")?;
    control_request(control, 14, "pause", None, true, "paused")?;
    control_request(control, 15, "tick", Some(start), false, "paused")?;
    control_request(control, 15, "seek", Some(start), true, "paused")?;
    udp.set_read_timeout(Some(Duration::from_millis(200)))?;
    let mut bytes = [0; 512];
    assert!(
        matches!(udp.recv(&mut bytes), Err(error) if matches!(error.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock))
    );
    assert!(subscriber
        .recv_timeout(Duration::from_millis(200))?
        .is_none());
    udp.set_read_timeout(Some(Duration::from_secs(3)))?;
    control_request(control, 16, "resume", None, true, "running")?;
    assert_eq!(
        read(start)?,
        first,
        "backward seek must reproduce both transports"
    );
    control_request(control, 17, "stop", None, true, "stopped")?;
    println!("Verified 100 identities at 25 Hz timestamps, DIS/Protobuf parity, synthetic provenance, ordered control and replay.");
    Ok(())
}

fn control_request(
    target: SocketAddr,
    sequence: u64,
    command: &str,
    at: Option<DateTime<Utc>>,
    accepted: bool,
    state: &str,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let mut request =
        json!({"schema_version":1,"run_id":"compose","sequence":sequence,"command":command});
    if let Some(at) = at {
        request["at_utc"] = json!(at);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut stream = loop {
        match TcpStream::connect_timeout(&target, Duration::from_millis(250)) {
            Ok(stream) => break stream,
            Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(error.into()),
        }
    };
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    writeln!(stream, "{request}")?;
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply)?;
    let reply: Value = serde_json::from_str(&reply)?;
    assert_eq!(
        reply["accepted"], accepted,
        "control {command} reply: {reply}"
    );
    assert_eq!(reply["state"], state, "control {command} reply: {reply}");
    assert_eq!(reply["sequence"], sequence);
    assert_eq!(reply["run_id"], "compose");
    Ok(reply)
}

type Packets = BTreeMap<u32, Vec<u8>>;

fn read_batch(
    udp: &UdpSocket,
    batch: &StateBatch,
    expected: DateTime<Utc>,
) -> Result<Packets, Box<dyn Error + Send + Sync>> {
    assert_eq!(batch.schema_version, 1);
    assert_eq!(batch.run_id, "compose");
    let time = batch.simulation_utc.as_ref().ok_or("missing UTC")?;
    assert_eq!(
        (time.seconds, time.nanos),
        (expected.timestamp(), expected.timestamp_subsec_nanos())
    );
    assert_eq!(batch.states.len(), COUNT as usize);
    let mut packets = BTreeMap::new();
    for _ in 0..COUNT {
        let mut packet = [0_u8; 512];
        let length = udp.recv(&mut packet)?;
        let u16_at = |start| u16::from_be_bytes(packet[start..start + 2].try_into().unwrap());
        assert_eq!(length, 144);
        assert_eq!(&packet[..4], &[7, 37, 1, 1]);
        assert_eq!(usize::from(u16_at(8)), length);
        let stamp = u32::from_be_bytes(packet[4..8].try_into()?);
        assert_eq!(stamp & 1, 1);
        let seconds = f64::from(stamp >> 1) * 3600.0 / 2147483648.0;
        assert!(
            (seconds
                - (expected.timestamp().rem_euclid(3600) as f64 + f64::from(time.nanos) / 1e9))
                .abs()
                < 2e-6
        );
        assert_eq!((u16_at(12), u16_at(14)), (5, 12));
        assert_eq!(&packet[20..22], &[1, 5]);
        assert_eq!(packet[88], 2);
        assert_eq!(packet[128], 1);
        let marking = std::str::from_utf8(&packet[129..140])?.trim_end_matches('\0');
        let id: u32 = marking.parse()?;
        assert!((70_000..70_000 + COUNT).contains(&id));
        assert_eq!(u32::from(u16_at(16)), id - 70_000 + 1);
        let protobuf = &batch.states[(id - 70_000) as usize];
        assert_eq!(protobuf.norad_id, id);
        assert_eq!(protobuf.source, "local-fixture");
        let synthetic = protobuf
            .synthetic_epoch
            .as_ref()
            .ok_or("missing synthetic provenance")?;
        assert_ne!(protobuf.source_epoch.as_ref(), Some(synthetic));
        let p = protobuf.position_ecef_m.ok_or("missing position")?;
        let v = protobuf.velocity_ecef_m_per_s.ok_or("missing velocity")?;
        for (axis, (p, v)) in [p.x, p.y, p.z].into_iter().zip([v.x, v.y, v.z]).enumerate() {
            let pos = 48 + axis * 8;
            let vel = 36 + axis * 4;
            assert_eq!(p, f64::from_be_bytes(packet[pos..pos + 8].try_into()?));
            assert_eq!(
                v as f32,
                f32::from_be_bytes(packet[vel..vel + 4].try_into()?)
            );
        }
        assert!(
            packets.insert(id, packet[..length].to_vec()).is_none(),
            "duplicate NORAD {id}"
        );
    }
    Ok(packets)
}
