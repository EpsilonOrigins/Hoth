use hoth_domain::state::SatelliteState;
use hoth_publisher::{PublishError, Publisher};
use hoth_zenoh::{client_config, StateBatch, ZenohPublisher};
use prost::Message;
use std::{
    net::{SocketAddr, TcpListener},
    thread,
    time::{Duration, Instant},
};
use zenoh::{Config, Session, Wait};

fn router(at: SocketAddr) -> Session {
    let mut config = Config::default();
    config.insert_json5("mode", "\"router\"").unwrap();
    config
        .insert_json5("scouting/multicast/enabled", "false")
        .unwrap();
    config
        .insert_json5(
            "listen/endpoints",
            &serde_json::json!([format!("tcp/{at}")]).to_string(),
        )
        .unwrap();
    zenoh::open(config).wait().unwrap()
}

#[test]
fn reconnects_and_resumes_batches_after_router_outage() {
    let temporary = TcpListener::bind("127.0.0.1:0").unwrap();
    let at = temporary.local_addr().unwrap();
    drop(temporary);
    let first = router(at);
    let state: SatelliteState = serde_json::from_str(include_str!(
        "../../hoth-domain/tests/fixtures/satellite-state.json"
    ))
    .unwrap();
    let states = std::slice::from_ref(&state);
    let mut publisher = ZenohPublisher::new(at, "reconnect".into());
    publisher.init().unwrap();
    publisher.publish_batch(state.at_utc, states).unwrap();
    first.close().wait().unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    while !matches!(
        publisher.publish_batch(state.at_utc, states),
        Err(PublishError::Backpressure)
    ) {
        assert!(
            Instant::now() < deadline,
            "router disconnect was not observed"
        );
        thread::sleep(Duration::from_millis(50));
    }
    let replacement = router(at);
    let client = zenoh::open(client_config(at).unwrap()).wait().unwrap();
    let subscriber = client
        .declare_subscriber("hoth/v1/reconnect/states")
        .wait()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let result = publisher.publish_batch(state.at_utc, states);
        match result {
            Ok(()) | Err(PublishError::Backpressure) => {}
            Err(error) => panic!("unexpected publisher failure: {error}"),
        }
        if let Some(sample) = subscriber.recv_timeout(Duration::from_millis(100)).unwrap() {
            let batch = StateBatch::decode(sample.payload().to_bytes().as_ref()).unwrap();
            assert_eq!(batch.states.len(), 1);
            assert_eq!(batch.states[0].norad_id, state.norad_id.get());
            break;
        }
        assert!(Instant::now() < deadline, "publisher did not recover");
    }
    publisher.shutdown().unwrap();
    drop(subscriber);
    client.close().wait().unwrap();
    replacement.close().wait().unwrap();
}
