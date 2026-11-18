# Execution Fabric integration

An execution controller starts and commands each HOTH process. Prepare the scenario outside the Execution Fabric, then provide the verified manifest on a read-only volume. The standalone runtime uses its monotonic system clock. The external runtime waits in `ready` and advances only on an accepted controller command. HOTH's canonical data path publishes directly to Zenoh and UDP listeners in the Execution Fabric. Enterprise ECS or RSA can consume run status through an adapter at their own cadence.

The local mock adapter accepts one UTF-8 JSON command followed by a newline per TCP connection. Use `--control 127.0.0.1:4123 --run-id local` for a loopback test. The Compose verifier sends commands to the private Compose subnet. This mock protocol assumes a trusted controller on an isolated network. For a future deployed ECS integration, replace or secure the adapter with the documented ECS protocol and authentication.

```json
{"schema_version":1,"run_id":"local","sequence":1,"command":"start"}
{"schema_version":1,"run_id":"local","sequence":2,"command":"tick","at_utc":"2026-11-17T00:00:00.040Z"}
```

Use `start`, `pause`, `resume`, `seek`, `tick`, and `stop`. Tick and seek require absolute `at_utc` inside the mission. Backward ticks are rejected. Advance the per-run sequence after an accepted reply. Replaying the most recently accepted request returns the same ACK without acting again. An out-of-order request returns an error without advancing the sequence. The reply includes run ID, sequence, acceptance, lifecycle state, UTC and error. A stop request closes the run after its reply. Standalone mode can use the same adapter for pause, resume, seek and stop. A new process starts a fresh controller sequence.

The [Protobuf v1 schema](../../proto/hoth/telemetry/v1/satellite.proto) publishes SI ECEF vectors, absolute UTC, NORAD, run identity, and both source and synthetic initialization provenance. Subscribe to `hoth/v1/<run-id>/states`. Each message contains one canonical batch with sorted NORAD IDs. For DIS, use the manifest mapping and the UDP receiver's packet timestamp and marking. The Compose verifier compares the ECEF doubles for position and the float32 conversion for velocity on both paths.

Production ECS/RSA API compatibility and production network transport are future validation. The mock controller verifies clock authority, replay, ACK and lifecycle semantics in the local environment.
