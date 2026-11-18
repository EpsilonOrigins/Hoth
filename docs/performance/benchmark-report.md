# Local throughput benchmark

## Reproduce

Run the release harness with an Eclipse Zenoh 1.7.2 router listening on local TCP. The harness builds a deterministic fixture population with varying orbital phases, propagates it, measures canonical state and serialization stages, sends one DIS PDU per UDP datagram, and validates receive counts on both transports.

```sh
docker run --rm --name hoth-bench-router -p 127.0.0.1:17447:7447 \
  eclipse/zenoh:1.7.2 -l tcp/0.0.0.0:7447
```

In a second terminal:

```sh
cargo build --release --locked --offline -p hoth --example pipeline_bench
export TIMEFORMAT='wall=%R user=%U sys=%S cpu=%P'
for objects in 1000 15000 50000; do
  for hz in 10 25; do
    time target/release/examples/pipeline_bench "$objects" "$hz" 60 127.0.0.1:17447
  done
done
```

Omit the final address to measure propagation and DIS without a Zenoh router. Set `HOTH_BENCH_UDP_RX_BYTES=4194304` to request a larger receive buffer on the benchmark's socket. The benchmark reports the effective OS buffer. This setting changes the test receiver, not the host or HOTH runtime configuration.

## Environment

Measured on November 18, 2026. Native optimized Rust 1.100.0 binary on Debian 13 x86-64, Linux 6.18.57, AMD EPYC 9V74, 3 visible logical CPUs, cgroup quota of 2 CPUs and 8 GiB memory. The local router ran in Docker, reached through a published loopback port. Default UDP receive buffer was 212,992 bytes and the kernel cap was 4,194,304 bytes. No external network or orbital services took part in the benchmark.

Tick processing includes SGP4, canonical batch construction, enqueue to a bounded Zenoh worker and inline DIS sending with a simultaneous UDP receiver. Output counts were measured after draining the workers and receiving queues. HOTH normally runs the DIS sender in its own worker. The inline stage lets the benchmark measure the UDP bottleneck directly. Achieved state rate is generated states divided by the larger of nominal test duration or elapsed processing time. CPU is total user plus system time as a percentage of elapsed wall time, with 100% representing one core. Peak RSS includes the benchmark's publisher and subscriber clients and excludes the router process.

## Results

All rows used 60 ticks and the default receive buffer. The sender reported zero DIS send errors on all rows. Loss describes observations at the receiver.

| Objects | Requested Hz | Generated states/s | Tick p50 / p95 / p99 ms | Start delay p99 ms | UDP received / sent | Zenoh received / sent | CPU / peak RSS |
| ---: | ---: | ---: | --- | ---: | --- | --- | --- |
| 1,000 | 10 | 10,000 | 1.91 / 22.47 / 33.38 | 5.71 | 53,356 / 60,000 | 60,000 / 60,000 | 10.0% / 10.6 MiB |
| 1,000 | 25 | 25,000 | 2.67 / 5.51 / 22.10 | 14.12 | 57,823 / 60,000 | 60,000 / 60,000 | 10.7% / 10.6 MiB |
| 15,000 | 10 | 150,000 | 34.44 / 50.43 / 53.85 | 0.32 | 707,716 / 900,000 | 900,000 / 900,000 | 51.8% / 47.0 MiB |
| 15,000 | 25 | 373,486 | 33.69 / 59.19 / 77.96 | 70.48 | 668,284 / 900,000 | 900,000 / 900,000 | 101.6% / 47.9 MiB |
| 50,000 | 10 | 372,634 | 125.99 / 199.68 / 255.93 | 2,063.48 | 2,503,665 / 3,000,000 | 2,950,000 / 3,000,000 | 144.0% / 119.2 MiB |
| 50,000 | 25 | 413,869 | 105.91 / 221.23 / 346.53 | 4,798.66 | 2,525,733 / 3,000,000 | 2,900,000 / 3,000,000 | 137.9% / 119.3 MiB |

For 15,000 objects at 25 Hz, isolated median SGP4 was 5.17 ms and TEME-to-ECEF was 0.41 ms. DIS encoding including entity lookup was 1.20 ms for one batch. The serialization sample produced a 1.79 MB Protobuf batch in 4.63 ms. During concurrent reception, Linux UDP send cost was 26.91 / 42.85 / 61.32 ms at p50 / p95 / p99. System CPU time was 2.02 seconds compared with 1.31 seconds of application CPU time over the entire benchmark. At this rate, the DIS receiver lost 25.7% of submitted packets.

A preliminary 15,000-object run with one send syscall per datagram measured 49.55 ms median DIS send and 246,595 generated states/s. It overlapped with integration startup and serves as a profiling signal. The optimized implementation passes up to 64 separate datagrams per Linux send syscall. Reused canonical vectors now allocate zero per batch in the isolated heap probe, down from 30,000 allocations and 450 kB requested per 15,000-object batch. Protobuf batch construction still allocates twice per object for source strings. Preallocating the message vector cut total requested heap bytes from 7.74 MB to 4.76 MB for the same Protobuf payload.

The 15,000-object run approached the requested average state rate. It exceeded the 40 ms tick budget at p95 and lost UDP packets, so reliable dual delivery at 15,000 objects and 25 Hz remains unverified. The 50,000-object configuration exceeded its time budget at both rates. The SGP4 and frame stages fit comfortably in the 15,000-object budget on this workstation. Packet processing, receiver buffering and concurrent transport scheduling consumed most of the headroom. These measurements apply to this CPU quota, the default receiver, and local networking.

The receiver-buffer follow-up requested 4 MiB and Linux reported an effective 8 MiB. Both receivers observed all 900,000 submitted states at 15,000 objects. The run generated 359,266 states/s with tick p50 / p95 / p99 of 37.42 / 54.89 / 97.16 ms and peak RSS of 46.5 MiB. Receiver buffering eliminated packet loss for this run. The processing deadline still exceeded the target, so buffer tuning alone does not establish sustained 25 Hz.
