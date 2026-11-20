use chrono::{DateTime, Utc};
use hoth_dis::{EntityId, EntityStatePdu, PDU_BYTES};
use hoth_scenario::Manifest;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    io,
    net::{SocketAddr, UdpSocket},
    sync::{Arc, Mutex},
    thread,
    time::{Instant, SystemTime},
};

#[derive(Clone, Copy)]
struct Received {
    bytes: [u8; PDU_BYTES],
    source: SocketAddr,
    at: SystemTime,
    received: Instant,
}

#[derive(Clone, Copy, Default, Serialize)]
struct Counters {
    datagrams: u64,
    accepted: u64,
    ignored: u64,
    entities_seen: usize,
}

#[derive(Default)]
struct Capture {
    counters: Counters,
    packets: BTreeMap<u32, Received>,
}

#[derive(Serialize)]
pub struct Snapshot {
    counters: Counters,
    packet: Option<Packet>,
}

#[derive(Serialize)]
struct Packet {
    norad_id: u32,
    source: SocketAddr,
    received_at_utc: DateTime<Utc>,
    age_ms: u64,
    pdu: EntityStatePdu,
    hex: String,
}

pub struct WireTap {
    capture: Arc<Mutex<Capture>>,
}

impl WireTap {
    pub fn listen(address: SocketAddr, manifest: &Manifest, exercise: u8) -> io::Result<Self> {
        let socket = UdpSocket::bind(address)?;
        let entities: Vec<_> = manifest
            .members()
            .iter()
            .map(|member| (member.entity().id, member.norad_id()))
            .collect();
        let capture = Arc::new(Mutex::new(Capture::default()));
        let input = Arc::clone(&capture);
        thread::Builder::new()
            .name("dis-inspector".into())
            .spawn(move || {
                use io::ErrorKind::Interrupted;
                // Large enough to detect oversize UDP datagrams without truncation.
                let mut buffer = [0u8; 65536];
                loop {
                    let (count, source) = match socket.recv_from(&mut buffer) {
                        Ok(pair) => pair,
                        Err(error) if error.kind() == Interrupted => continue,
                        Err(error) => {
                            eprintln!("DIS inspector receive failed: {error}");
                            break;
                        }
                    };
                    let packet = match_packet(&buffer[..count], exercise, &entities);
                    let mut capture = input.lock().expect("DIS capture");
                    capture.counters.datagrams += 1;
                    if let Some((norad, bytes)) = packet {
                        capture.counters.accepted += 1;
                        capture.packets.insert(
                            norad,
                            Received {
                                bytes,
                                source,
                                at: SystemTime::now(),
                                received: Instant::now(),
                            },
                        );
                        capture.counters.entities_seen = capture.packets.len();
                    } else {
                        capture.counters.ignored += 1;
                    }
                }
            })?;
        Ok(Self { capture })
    }

    pub fn snapshot(&self, norad_id: u32) -> Snapshot {
        let (counters, packet) = {
            let capture = self.capture.lock().expect("DIS capture");
            (capture.counters, capture.packets.get(&norad_id).copied())
        };
        Snapshot {
            counters,
            packet: packet.map(|wire| Packet {
                norad_id,
                source: wire.source,
                received_at_utc: wire.at.into(),
                age_ms: wire.received.elapsed().as_millis() as u64,
                pdu: EntityStatePdu::decode(&wire.bytes).expect("accepted PDU was validated"),
                hex: hex(&wire.bytes),
            }),
        }
    }
}

fn match_packet(
    packet: &[u8],
    exercise: u8,
    entities: &[(EntityId, u32)],
) -> Option<(u32, [u8; PDU_BYTES])> {
    let pdu = EntityStatePdu::decode(packet).ok()?;
    let index = usize::from(pdu.entity_id.entity).checked_sub(1)?;
    let &(id, norad) = entities.get(index)?;
    (pdu.exercise_id == exercise && pdu.entity_id == id && pdu.marking.parse().ok() == Some(norad))
        .then(|| (norad, packet.try_into().expect("fixed DIS PDU size")))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut dump = String::new();
    for (index, row) in bytes.chunks(16).enumerate() {
        write!(dump, "{:04x}  ", index * 16).expect("string write");
        for byte in row {
            write!(dump, "{byte:02x} ").expect("string write");
        }
        dump.push('\n');
    }
    dump
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_requires_matching_exercise_and_identity() {
        let packet = include_bytes!("../../../../crates/hoth-dis/tests/fixtures/entity-state.pdu");
        let id = EntityId {
            site: 5,
            application: 12,
            entity: 2,
        };
        let entities = [(EntityId { entity: 1, ..id }, 10), (id, 100_001)];
        let (norad, bytes) = match_packet(packet, 37, &entities).unwrap();
        assert_eq!(norad, 100_001);
        assert_eq!(&bytes, packet);
        assert!(match_packet(packet, 0, &entities).is_none());
        assert!(match_packet(packet, 37, &[(id, 100_001)]).is_none());
        assert!(hex(packet).starts_with("0000  07 25 01 01"));
        assert_eq!(hex(packet).lines().count(), 9);
    }
}
