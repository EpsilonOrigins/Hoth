//! DIS 7 Entity State encoding and UDP transport.

mod datagrams;
mod decode;

pub use decode::EntityStatePdu;

use hoth_domain::state::{SatelliteState, SimulationUtc};
use hoth_publisher::{PublishError, Publisher};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    error::Error,
    io,
    net::{IpAddr, SocketAddr, UdpSocket},
};

pub const PDU_BYTES: usize = 144;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityId {
    pub site: u16,
    pub application: u16,
    pub entity: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityType {
    pub kind: u8,
    pub domain: u8,
    pub country: u16,
    pub category: u8,
    pub subcategory: u8,
    pub specific: u8,
    pub extra: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    schema_version: u8,
    fallback: EntityType,
    #[serde(default)]
    overrides: BTreeMap<u32, EntityType>,
}

impl Registry {
    pub fn from_yaml(yaml: &str) -> Result<Self, Box<dyn Error>> {
        let registry: Self = yaml_serde::from_str(yaml)?;
        if registry.schema_version != 1 {
            return Err("expected entity type registry schema_version 1".into());
        }
        if registry.overrides.contains_key(&0) {
            return Err("entity type overrides require positive NORAD IDs".into());
        }
        for entry in registry
            .overrides
            .values()
            .chain(std::iter::once(&registry.fallback))
        {
            entry.validate()?;
        }
        Ok(registry)
    }

    pub fn lookup(&self, norad: u32) -> EntityType {
        self.overrides.get(&norad).copied().unwrap_or(self.fallback)
    }
}

impl EntityType {
    pub fn validate(self) -> Result<(), &'static str> {
        if self.kind != 1 || self.domain != 5 {
            return Err("SISO entity type must be platform (1), space (5)");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entity {
    pub id: EntityId,
    pub entity_type: EntityType,
}

/// DIS absolute timestamp uses the low bit as the absolute flag, and the upper
/// 31 bits as an unsigned fraction of the simulation UTC hour.
pub fn timestamp(at: SimulationUtc) -> u32 {
    let utc = at.instant();
    let nanos = utc.timestamp().rem_euclid(3600) as u64 * 1_000_000_000
        + u64::from(utc.timestamp_subsec_nanos()).min(999_999_999);
    let units = (u128::from(nanos) << 31) / 3_600_000_000_000;
    ((units as u32) << 1) | 1
}

pub fn encode(state: &SatelliteState, entity: Entity, exercise_id: u8) -> [u8; PDU_BYTES] {
    let mut pdu = [0_u8; PDU_BYTES];
    pdu[..4].copy_from_slice(&[7, exercise_id, 1, 1]);
    pdu[4..8].copy_from_slice(&timestamp(state.at_utc).to_be_bytes());
    pdu[8..10].copy_from_slice(&(PDU_BYTES as u16).to_be_bytes());
    pdu[12..14].copy_from_slice(&entity.id.site.to_be_bytes());
    pdu[14..16].copy_from_slice(&entity.id.application.to_be_bytes());
    pdu[16..18].copy_from_slice(&entity.id.entity.to_be_bytes());
    let kind = entity.entity_type;
    let [country_hi, country_lo] = kind.country.to_be_bytes();
    pdu[20..28].copy_from_slice(&[
        kind.kind,
        kind.domain,
        country_hi,
        country_lo,
        kind.category,
        kind.subcategory,
        kind.specific,
        kind.extra,
    ]);
    for (target, value) in pdu[36..48]
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(state.ecef.velocity_m_per_s.xyz())
    {
        target.copy_from_slice(&(value as f32).to_be_bytes());
    }
    for (target, value) in pdu[48..72]
        .as_chunks_mut::<8>()
        .0
        .iter_mut()
        .zip(state.ecef.position_m.xyz())
    {
        target.copy_from_slice(&value.to_be_bytes());
    }
    // World-frame constant-velocity dead reckoning, fixed orientation (DRM FPW).
    pdu[88] = 2;
    pdu[128] = 1;
    write_norad_marking(&mut pdu[129..140], state.norad_id.get());
    pdu
}

fn write_norad_marking(marking: &mut [u8], mut number: u32) {
    let mut digits = [0u8; 10];
    let mut start = digits.len();
    while number > 0 {
        start -= 1;
        digits[start] = b'0' + (number % 10) as u8;
        number /= 10;
    }
    let text = &digits[start..];
    marking[..text.len()].copy_from_slice(text);
}

pub struct UdpPublisher {
    destination: SocketAddr,
    multicast: bool,
    socket: Option<UdpSocket>,
    used: bool,
    exercise_id: u8,
    entities: BTreeMap<u32, Entity>,
}

impl UdpPublisher {
    pub fn new(
        destination: SocketAddr,
        multicast: bool,
        exercise_id: u8,
        entities: BTreeMap<u32, Entity>,
    ) -> Self {
        Self {
            destination,
            multicast,
            socket: None,
            used: false,
            exercise_id,
            entities,
        }
    }
}

impl Publisher for UdpPublisher {
    fn init(&mut self) -> Result<(), PublishError> {
        if std::mem::replace(&mut self.used, true) {
            return Err(PublishError::InvalidLifecycle);
        }
        if self.destination.ip().is_multicast() != self.multicast || self.destination.port() == 0 {
            return Err(PublishError::Sink(
                "invalid UDP destination or multicast opt-in".into(),
            ));
        }
        let any = match self.destination.ip() {
            IpAddr::V4(_) => IpAddr::from([0, 0, 0, 0]),
            IpAddr::V6(_) => IpAddr::from([0; 16]),
        };
        let socket = UdpSocket::bind((any, 0)).map_err(sink)?;
        socket.set_nonblocking(true).map_err(sink)?;
        if self.multicast && self.destination.is_ipv4() {
            socket.set_multicast_ttl_v4(1).map_err(sink)?;
        }
        self.socket = Some(socket);
        Ok(())
    }

    fn publish_batch(
        &mut self,
        at: SimulationUtc,
        states: &[SatelliteState],
    ) -> Result<(), PublishError> {
        let socket = self.socket.as_ref().ok_or(PublishError::InvalidLifecycle)?;
        for (index, state) in states.iter().enumerate() {
            if state.at_utc != at {
                return Err(PublishError::TimestampMismatch {
                    index,
                    actual: state.at_utc,
                    expected: at,
                });
            }
            if !self.entities.contains_key(&state.norad_id.get())
                || state
                    .ecef
                    .velocity_m_per_s
                    .xyz()
                    .iter()
                    .any(|value| !(*value as f32).is_finite())
            {
                return Err(PublishError::Sink(
                    format!("invalid DIS state for NORAD {}", state.norad_id.get()).into(),
                ));
            }
        }
        let mut packets = [[0; PDU_BYTES]; datagrams::PER_CALL];
        for (chunk, states) in states.chunks(datagrams::PER_CALL).enumerate() {
            for (packet, state) in packets.iter_mut().zip(states) {
                let entity = self.entities[&state.norad_id.get()];
                *packet = encode(state, entity, self.exercise_id);
            }
            match datagrams::send(socket, self.destination, &packets[..states.len()]) {
                Ok(()) => {}
                Err((0, error)) if chunk == 0 && error.kind() == io::ErrorKind::WouldBlock => {
                    return Err(PublishError::Backpressure);
                }
                Err((_, error)) => return Err(sink(error)),
            }
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), PublishError> {
        self.socket
            .as_ref()
            .map(|_| ())
            .ok_or(PublishError::InvalidLifecycle)
    }

    fn shutdown(&mut self) -> Result<(), PublishError> {
        self.used = true;
        self.socket.take();
        Ok(())
    }
}

fn sink(error: io::Error) -> PublishError {
    PublishError::Sink(Box::new(error))
}
