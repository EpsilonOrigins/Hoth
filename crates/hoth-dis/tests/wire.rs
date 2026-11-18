use hoth_dis::{encode, Entity, EntityId, Registry, UdpPublisher};
use hoth_domain::state::SatelliteState;
use hoth_publisher::{PublishError, Publisher};
use std::{collections::BTreeMap, net::UdpSocket, time::Duration};

const STATE: &str = include_str!("../../hoth-domain/tests/fixtures/satellite-state.json");
// OpenDIS 1.1.0 dis7.EntityStatePdu serialization of STATE with exercise 37,
// entity (5,12,2), Platform/Space, fixed orientation/DRM FPW and ASCII marking.
const OPEN_DIS: &[u8] = include_bytes!("fixtures/entity-state.pdu");

fn entity() -> Entity {
    let registry =
        Registry::from_yaml(include_str!("../../../config/dis/entity_types.yaml")).unwrap();
    Entity {
        id: EntityId {
            site: 5,
            application: 12,
            entity: 2,
        },
        entity_type: registry.lookup(100_001),
    }
}

#[test]
fn dis_entity_state_matches_opendis_golden_packet() {
    let state: SatelliteState = serde_json::from_str(STATE).unwrap();
    assert_eq!(encode(&state, entity(), 37), OPEN_DIS);
}

#[test]
fn unicast_publishes_exact_pdus_and_rejects_use_after_shutdown() {
    let state: SatelliteState = serde_json::from_str(STATE).unwrap();
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut publisher = UdpPublisher::new(
        socket.local_addr().unwrap(),
        false,
        37,
        BTreeMap::from([(state.norad_id.get(), entity())]),
    );
    publisher.init().unwrap();
    let batch = std::slice::from_ref(&state);
    for _ in 0..10 {
        publisher.publish_batch(state.at_utc, batch).unwrap();
        let mut received = [0; 512];
        let length = socket.recv(&mut received).unwrap();
        assert_eq!(&received[..length], OPEN_DIS);
    }
    publisher.flush().unwrap();
    publisher.shutdown().unwrap();
    publisher.shutdown().unwrap();
    assert!(matches!(
        publisher.publish_batch(state.at_utc, batch),
        Err(PublishError::InvalidLifecycle)
    ));
}

#[test]
fn registry_validates_version_class_and_overrides() {
    let fallback = "fallback: {kind: 1, domain: 5, country: 0, category: 0, subcategory: 0, specific: 0, extra: 0}";
    let input = format!("schema_version: 1\n{fallback}\noverrides:\n  100001: {{kind: 1, domain: 5, country: 225, category: 1, subcategory: 0, specific: 0, extra: 0}}");
    let registry = Registry::from_yaml(&input).unwrap();
    assert_eq!(registry.lookup(100001).country, 225);
    assert_eq!(registry.lookup(100002).country, 0);
    for invalid in [
        input.replace("version: 1", "version: 9"),
        input.replace("domain: 5", "domain: 1"),
        input.replace("225", "65536"),
    ] {
        assert!(Registry::from_yaml(&invalid).is_err());
    }
}
