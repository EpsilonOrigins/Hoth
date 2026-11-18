//! Versioned Protobuf batches over a Zenoh TCP router.

use hoth_domain::state::{EpochInitialization, SatelliteState, SimulationUtc, UtcInstant};
use hoth_publisher::{PublishError, Publisher};
use prost::Message;
use std::net::SocketAddr;
use zenoh::{qos::CongestionControl, Config, Session, Wait};

/// Wire types mirror proto/hoth/telemetry/v1/satellite.proto.
#[derive(Clone, PartialEq, Message)]
pub struct Utc {
    #[prost(int64, tag = "1")]
    pub seconds: i64,
    #[prost(uint32, tag = "2")]
    pub nanos: u32,
}
impl From<UtcInstant> for Utc {
    fn from(instant: UtcInstant) -> Self {
        Self {
            seconds: instant.timestamp(),
            nanos: instant.timestamp_subsec_nanos(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Message)]
pub struct Vector3 {
    #[prost(double, tag = "1")]
    pub x: f64,
    #[prost(double, tag = "2")]
    pub y: f64,
    #[prost(double, tag = "3")]
    pub z: f64,
}
impl From<[f64; 3]> for Vector3 {
    fn from([x, y, z]: [f64; 3]) -> Self {
        Self { x, y, z }
    }
}

#[derive(Clone, PartialEq, Message)]
pub struct State {
    #[prost(uint32, tag = "1")]
    pub norad_id: u32,
    #[prost(message, optional, tag = "2")]
    pub position_ecef_m: Option<Vector3>,
    #[prost(message, optional, tag = "3")]
    pub velocity_ecef_m_per_s: Option<Vector3>,
    #[prost(string, tag = "4")]
    pub source: String,
    #[prost(string, tag = "5")]
    pub source_record_id: String,
    #[prost(message, optional, tag = "6")]
    pub source_epoch: Option<Utc>,
    #[prost(message, optional, tag = "7")]
    pub synthetic_epoch: Option<Utc>,
}

#[derive(Clone, PartialEq, Message)]
pub struct StateBatch {
    #[prost(uint32, tag = "1")]
    pub schema_version: u32,
    #[prost(string, tag = "2")]
    pub run_id: String,
    #[prost(message, optional, tag = "3")]
    pub simulation_utc: Option<Utc>,
    #[prost(message, repeated, tag = "4")]
    pub states: Vec<State>,
}

impl StateBatch {
    pub fn from_states(
        run_id: &str,
        at: SimulationUtc,
        states: &[SatelliteState],
    ) -> Result<Self, PublishError> {
        let mut encoded = Vec::with_capacity(states.len());
        for (index, state) in states.iter().enumerate() {
            if state.at_utc != at {
                return Err(PublishError::TimestampMismatch {
                    index,
                    expected: at,
                    actual: state.at_utc,
                });
            }
            let source = &state.provenance;
            encoded.push(State {
                norad_id: state.norad_id.get(),
                position_ecef_m: Some(state.ecef.position_m.xyz().into()),
                velocity_ecef_m_per_s: Some(state.ecef.velocity_m_per_s.xyz().into()),
                source: source.source().into(),
                source_record_id: source.source_record_id().into(),
                source_epoch: Some(source.source_epoch_utc.into()),
                synthetic_epoch: match source.initialization {
                    EpochInitialization::SourceEpoch => None,
                    EpochInitialization::Synthetic { epoch_utc } => Some(epoch_utc.into()),
                },
            });
        }
        Ok(Self {
            schema_version: 1,
            run_id: run_id.into(),
            simulation_utc: Some(at.instant().into()),
            states: encoded,
        })
    }
}

pub fn client_config(destination: SocketAddr) -> zenoh::Result<Config> {
    let mut config = Config::default();
    config.insert_json5("mode", "\"client\"")?;
    config.insert_json5("scouting/multicast/enabled", "false")?;
    config.insert_json5(
        "connect/endpoints",
        &serde_json::json!([format!("tcp/{destination}")]).to_string(),
    )?;
    config.insert_json5("connect/timeout_ms", "5000")?;
    Ok(config)
}

pub struct ZenohPublisher {
    destination: SocketAddr,
    run_id: String,
    key: String,
    session: Option<Session>,
    used: bool,
}

impl ZenohPublisher {
    pub fn new(destination: SocketAddr, run_id: String) -> Self {
        let key = format!("hoth/v1/{run_id}/states");
        Self {
            destination,
            run_id,
            key,
            session: None,
            used: false,
        }
    }
}

impl Publisher for ZenohPublisher {
    fn init(&mut self) -> Result<(), PublishError> {
        if std::mem::replace(&mut self.used, true) {
            return Err(PublishError::InvalidLifecycle);
        }
        zenoh::key_expr::KeyExpr::try_from(self.key.as_str()).map_err(PublishError::Sink)?;
        let config = client_config(self.destination).map_err(PublishError::Sink)?;
        self.session = Some(zenoh::open(config).wait().map_err(PublishError::Sink)?);
        Ok(())
    }

    fn publish_batch(
        &mut self,
        at: SimulationUtc,
        states: &[SatelliteState],
    ) -> Result<(), PublishError> {
        let session = self
            .session
            .as_ref()
            .ok_or(PublishError::InvalidLifecycle)?;
        if session.info().routers_zid().wait().next().is_none() {
            return Err(PublishError::Backpressure);
        }
        let payload = StateBatch::from_states(&self.run_id, at, states)?.encode_to_vec();
        session
            .put(&self.key, payload)
            .encoding(zenoh::bytes::Encoding::APPLICATION_PROTOBUF)
            .congestion_control(CongestionControl::Drop)
            .wait()
            .map_err(PublishError::Sink)
    }

    fn flush(&mut self) -> Result<(), PublishError> {
        self.session
            .as_ref()
            .map(|_| ())
            .ok_or(PublishError::InvalidLifecycle)
    }

    fn shutdown(&mut self) -> Result<(), PublishError> {
        self.used = true;
        if let Some(session) = self.session.take() {
            session.close().wait().map_err(PublishError::Sink)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_v1_decodes_canonical_protobuf_fixture() {
        // protoc --encode=hoth.telemetry.v1.StateBatch for
        // schema_version=1, run_id="r", simulation_utc=(1,2), states=[{norad_id:25544}].
        let wire = [8, 1, 18, 1, b'r', 26, 4, 8, 1, 16, 2, 34, 4, 8, 200, 199, 1];
        let batch = StateBatch::decode(wire.as_slice()).unwrap();
        assert_eq!(batch.schema_version, 1);
        assert_eq!(batch.run_id, "r");
        assert_eq!(
            batch.simulation_utc,
            Some(Utc {
                seconds: 1,
                nanos: 2
            })
        );
        assert_eq!(batch.states[0].norad_id, 25544);
        assert_eq!(batch.encode_to_vec(), wire);
    }
}
