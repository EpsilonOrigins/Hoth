use crate::{EntityId, EntityType, PDU_BYTES};
use serde::Serialize;

/// Fields from a DIS 7 Entity State PDU without variable parameters.
#[derive(Debug, Serialize)]
pub struct EntityStatePdu {
    pub version: u8,
    pub exercise_id: u8,
    pub pdu_type: u8,
    pub protocol_family: u8,
    pub timestamp_raw: u32,
    pub timestamp_absolute: bool,
    pub timestamp_seconds_into_hour: f64,
    pub length_bytes: u16,
    pub pdu_status: u8,
    pub entity_id: EntityId,
    pub force_id: u8,
    pub entity_type: EntityType,
    pub alternate_entity_type: EntityType,
    pub velocity_ecef_m_per_s: [f32; 3],
    pub position_ecef_m: [f64; 3],
    pub orientation_rad: [f32; 3],
    pub appearance: u32,
    pub dead_reckoning_algorithm: u8,
    pub marking_charset: u8,
    pub marking: String,
    pub capabilities: u32,
}

impl EntityStatePdu {
    pub fn entity_id(packet: &[u8; PDU_BYTES]) -> EntityId {
        EntityId {
            site: unsigned16(packet, 12),
            application: unsigned16(packet, 14),
            entity: unsigned16(packet, 16),
        }
    }

    pub fn decode(packet: &[u8]) -> Result<Self, &'static str> {
        let packet = <&[u8; PDU_BYTES]>::try_from(packet)
            .map_err(|_| "expected a 144-byte fixed Entity State PDU")?;
        let length_bytes = unsigned16(packet, 8);
        if (packet[0], packet[2], packet[3]) != (7, 1, 1)
            || usize::from(length_bytes) != PDU_BYTES
            || packet[19] != 0
        {
            return Err("expected DIS 7 Entity State with no variable parameters");
        }
        let single = |offset| f32::from_bits(unsigned32(packet, offset));
        let velocity = [36, 40, 44].map(single);
        let position = [48, 56, 64]
            .map(|offset| f64::from_be_bytes(packet[offset..offset + 8].try_into().unwrap()));
        let orientation = [72, 76, 80].map(single);
        if !position.iter().all(|v| v.is_finite())
            || !velocity.iter().chain(&orientation).all(|v| v.is_finite())
        {
            return Err("nonfinite location, velocity or orientation");
        }
        let timestamp_raw = unsigned32(packet, 4);
        let timestamp_absolute = timestamp_raw & 1 != 0;
        let timestamp_seconds_into_hour = f64::from(timestamp_raw >> 1) * (3600.0 / 2147483648.0);
        let marking = String::from_utf8_lossy(&packet[129..140])
            .trim_end_matches('\0')
            .to_owned();
        Ok(Self {
            version: packet[0],
            exercise_id: packet[1],
            pdu_type: packet[2],
            protocol_family: packet[3],
            timestamp_raw,
            timestamp_absolute,
            timestamp_seconds_into_hour,
            length_bytes,
            pdu_status: packet[10],
            entity_id: Self::entity_id(packet),
            force_id: packet[18],
            entity_type: entity_type(packet, 20),
            alternate_entity_type: entity_type(packet, 28),
            velocity_ecef_m_per_s: velocity,
            position_ecef_m: position,
            orientation_rad: orientation,
            appearance: unsigned32(packet, 84),
            dead_reckoning_algorithm: packet[88],
            marking_charset: packet[128],
            marking,
            capabilities: unsigned32(packet, 140),
        })
    }
}

fn entity_type(packet: &[u8], at: usize) -> EntityType {
    EntityType {
        kind: packet[at],
        domain: packet[at + 1],
        country: unsigned16(packet, at + 2),
        category: packet[at + 4],
        subcategory: packet[at + 5],
        specific: packet[at + 6],
        extra: packet[at + 7],
    }
}

fn unsigned16(packet: &[u8], at: usize) -> u16 {
    u16::from_be_bytes(packet[at..at + 2].try_into().unwrap())
}

fn unsigned32(packet: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(packet[at..at + 4].try_into().unwrap())
}
