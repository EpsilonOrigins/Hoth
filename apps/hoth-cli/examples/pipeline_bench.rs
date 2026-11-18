//! Manual release benchmark for complete batches on one workstation.

use chrono::TimeDelta;
use hoth_catalog::parse::parse_omm_json;
use hoth_dis::{encode, Entity, EntityId, EntityType, UdpPublisher};
use hoth_domain::{
    orbit::RecordProvenance,
    state::{EpochInitialization, NoradId, SimulationUtc},
};
use hoth_orbits::{EarthRotation, Propagator};
use hoth_publisher::Publisher;
use hoth_runtime::{Engine, Output};
use hoth_zenoh::{StateBatch, ZenohPublisher};
use prost::Message;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    collections::BTreeMap,
    error::Error,
    hint::black_box,
    net::{SocketAddr, UdpSocket},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use zenoh::Wait;

struct CountingAllocator;

static MEASURING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

// The allocator forwards the original layout and pointer to System. The
// counters collect an isolated sample before the transport threads start.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if MEASURING.load(Relaxed) {
            ALLOCATIONS.fetch_add(1, Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size(), Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

fn heap_sample<T>(operation: impl FnOnce() -> T) -> (T, usize, usize) {
    ALLOCATIONS.store(0, Relaxed);
    ALLOCATED_BYTES.store(0, Relaxed);
    MEASURING.store(true, Relaxed);
    let value = operation();
    MEASURING.store(false, Relaxed);
    (
        value,
        ALLOCATIONS.load(Relaxed),
        ALLOCATED_BYTES.load(Relaxed),
    )
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let count: usize = args.first().ok_or("provide object count")?.parse()?;
    let hz: u32 = args.get(1).ok_or("provide tick rate")?.parse()?;
    let ticks: u32 = args.get(2).ok_or("provide tick count")?.parse()?;
    let router: Option<SocketAddr> = args.get(3).map(|arg| arg.parse()).transpose()?;
    if count == 0 || count > 65_535 || hz == 0 || ticks == 0 {
        return Err("usage: pipeline_bench OBJECTS HZ TICKS [ROUTER_IP:PORT]".into());
    }
    let time = SimulationUtc::new("2026-11-17T00:00:00Z".parse()?);
    let mut element = parse_omm_json(
        include_bytes!("../../../crates/hoth-catalog/tests/fixtures/orbits/iss.omm.json"),
        RecordProvenance {
            source: "benchmark-fixture".into(),
            record_id: "fixture-iss".into(),
        },
    )?
    .element;
    let entity_type = EntityType {
        kind: 1,
        domain: 5,
        country: 0,
        category: 0,
        subcategory: 0,
        specific: 0,
        extra: 0,
    };
    let mut propagators = Vec::with_capacity(count);
    let mut entities = BTreeMap::new();
    for index in 0..count {
        let norad_id = 70_000 + index as u32;
        element.norad_id = NoradId::new(norad_id).expect("positive NORAD");
        element.mean_elements.mean_anomaly_deg = (index as f64 * 17.25).rem_euclid(360.0);
        element.mean_elements.right_ascension_deg = (index as f64 * 7.63).rem_euclid(360.0);
        element.raw_source.provenance.record_id = format!("fixture-{norad_id:05}");
        propagators.push(Propagator::new(
            &element,
            EpochInitialization::Synthetic {
                epoch_utc: time.instant(),
            },
        )?);
        entities.insert(
            norad_id,
            Entity {
                id: EntityId {
                    site: 1,
                    application: 1,
                    entity: (index + 1) as u16,
                },
                entity_type,
            },
        );
    }
    // Isolated propagation and frame costs exclude initialization and I/O.
    let rotation = EarthRotation::at(time);
    let mut teme = Vec::with_capacity(count);
    let mut propagate_ms = Vec::new();
    let mut frame_ms = Vec::new();
    for _ in 0..16 {
        teme.clear();
        let start = Instant::now();
        for orbit in &propagators {
            teme.push(orbit.teme(time)?);
        }
        propagate_ms.push(ms(start.elapsed()));
        let start = Instant::now();
        for state in &teme {
            black_box(rotation.convert(*state)?);
        }
        frame_ms.push(ms(start.elapsed()));
    }
    let engine = Engine::new(propagators);
    let mut states = Vec::with_capacity(count);
    let (failures, canonical_allocations, canonical_bytes) =
        heap_sample(|| engine.batch(time, &mut states));
    assert_eq!(failures, 0);
    let start = Instant::now();
    for state in &states {
        black_box(encode(state, entities[&state.norad_id.get()], 1));
    }
    let dis_ms = ms(start.elapsed());
    let start = Instant::now();
    let payload_bytes =
        black_box(StateBatch::from_states("bench", time, &states)?.encode_to_vec()).len();
    let proto_ms = ms(start.elapsed());

    let (_, proto_allocations, proto_heap_bytes) = heap_sample(|| {
        StateBatch::from_states("bench", time, &states).map(|batch| batch.encode_to_vec())
    });
    println!("canonical_allocations={canonical_allocations} canonical_heap_bytes={canonical_bytes} proto_allocations={proto_allocations} proto_heap_bytes={proto_heap_bytes}");

    // Direct isolated UDP and routed Zenoh adapters with competing receiver
    // threads. The serial DIS path shows the cost of one datagram per entity.
    let udp = UdpSocket::bind("127.0.0.1:0")?;
    udp.set_read_timeout(Some(Duration::from_millis(50)))?;
    let address = udp.local_addr()?;
    let running = Arc::new(AtomicBool::new(true));
    let receiver_running = Arc::clone(&running);
    let udp_receiver = thread::spawn(move || {
        let mut received = 0usize;
        let mut bytes = [0_u8; 144];
        while receiver_running.load(Relaxed) {
            match udp.recv(&mut bytes) {
                Ok(n) => {
                    assert_eq!(n, 144);
                    received += 1;
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    continue
                }
                Err(error) => panic!("UDP reception failed: {error}"),
            }
        }
        while udp.recv(&mut bytes).is_ok() {
            received += 1;
        }
        received
    });

    let zenoh_count = Arc::new(AtomicUsize::new(0));
    let mut zenoh_session = None;
    let mut zenoh_subscriber = None;
    let mut zenoh = None;
    if let Some(router) = router {
        let session = zenoh::open(hoth_zenoh::client_config(router).map_err(|e| e.to_string())?)
            .wait()
            .map_err(|e| e.to_string())?;
        let counter = Arc::clone(&zenoh_count);
        let subscriber = session
            .declare_subscriber("hoth/v1/bench/states")
            .callback(move |sample| {
                let payload = sample.payload().to_bytes();
                if let Ok(batch) = StateBatch::decode(payload.as_ref()) {
                    counter.fetch_add(batch.states.len(), Relaxed);
                }
            })
            .wait()
            .map_err(|e| e.to_string())?;
        let publisher = ZenohPublisher::new(router, "bench".into());
        zenoh = Some(Output::start(
            "bench-zenoh".into(),
            f64::from(hz),
            publisher,
        )?);
        zenoh_subscriber = Some(subscriber);
        zenoh_session = Some(session);
        thread::sleep(Duration::from_millis(250));
    }

    let mut udp_output = UdpPublisher::new(address, false, 1, entities);
    udp_output.init()?;
    let period = Duration::from_secs_f64(1.0 / f64::from(hz));
    let mut processing = Vec::with_capacity(ticks as usize);
    let mut lateness = Vec::with_capacity(ticks as usize);
    let mut udp_send = Vec::with_capacity(ticks as usize);
    let mut successful_udp = 0;
    let mut udp_errors = 0;
    let mut previous = Some(Arc::new(states));
    let origin = Instant::now();
    for tick in 0..ticks {
        let deadline = origin + period * tick;
        thread::sleep(deadline.saturating_duration_since(Instant::now()));
        let start = Instant::now();
        lateness.push(ms(start.saturating_duration_since(deadline)));
        let utc = SimulationUtc::new(time.instant() + TimeDelta::from_std(period * tick)?);
        let mut states = previous
            .take()
            .and_then(Arc::into_inner)
            .unwrap_or_default();
        assert_eq!(engine.batch(utc, &mut states), 0);
        let states = Arc::new(states);
        if let Some(output) = &mut zenoh {
            output.submit_batch(utc, Arc::clone(&states));
        }
        let before_udp = Instant::now();
        match udp_output.publish_batch(utc, &states) {
            Ok(()) => successful_udp += count,
            Err(error) => {
                eprintln!("{error}");
                udp_errors += 1;
            }
        }
        udp_send.push(ms(before_udp.elapsed()));
        processing.push(ms(start.elapsed()));
        previous = Some(states);
    }
    let elapsed = origin.elapsed().max(period * ticks).as_secs_f64();
    running.store(false, Relaxed);
    udp_output.shutdown()?;
    let udp_received = udp_receiver.join().expect("UDP receiver");
    let zenoh_sent = zenoh
        .take()
        .map(|mut output| {
            output.drain();
            output.stats().sent_states.load(Relaxed)
        })
        .unwrap_or_default();
    if router.is_some() {
        thread::sleep(Duration::from_millis(400));
    }
    drop(zenoh_subscriber);
    drop(zenoh_session);
    let rss = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let peak = rss
        .lines()
        .find(|line| line.starts_with("VmHWM:"))
        .unwrap_or("VmHWM unavailable");
    let expected = count * ticks as usize;
    println!(
        "objects={count} hz={hz} ticks={ticks} states/s={:.0} budget_ms={:.1} {peak}",
        expected as f64 / elapsed,
        ms(period)
    );
    println!("propagate_ms={:.3} frame_ms={:.3} dis_encode_ms={dis_ms:.3} proto_encode_ms={proto_ms:.3} proto_bytes={payload_bytes}",
        percentile(&mut propagate_ms, 50), percentile(&mut frame_ms, 50));
    stats("tick_ms", &mut processing);
    stats("start_late_ms", &mut lateness);
    stats("udp_send_ms", &mut udp_send);
    println!("expected={expected} udp_sent={successful_udp} udp_rx={udp_received} udp_errors={udp_errors} zenoh_sent={zenoh_sent} zenoh_rx={}",
        zenoh_count.load(Relaxed));
    Ok(())
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn percentile(values: &mut [f64], p: usize) -> f64 {
    values.sort_unstable_by(f64::total_cmp);
    values[(p * values.len()).div_ceil(100) - 1]
}

fn stats(label: &str, samples: &mut [f64]) {
    println!(
        "{label} p50={:.3} p95={:.3} p99={:.3}",
        percentile(samples, 50),
        percentile(samples, 95),
        percentile(samples, 99)
    );
}
