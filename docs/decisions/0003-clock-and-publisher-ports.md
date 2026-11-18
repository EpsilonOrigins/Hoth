# ADR 0003 — Clock authority and publisher ports

## Status

Proposed for HOTH-005.

## Clock authority

Each clock has one authority for its lifetime. A standalone clock advances simulation UTC from elapsed monotonic time and a finite, nonnegative rate multiplier. A rate change takes effect from the current simulation instant. A zero rate holds that instant, independently of the explicit pause state. Scenario YAML v1 still requires a positive initial rate.

An external clock takes the exact absolute UTC of each accepted tick. Its adapter delivers events in authority order through `SimulationClock::apply`. Between external ticks the last accepted instant holds. Local rate changes are rejected in external mode, and an external tick is rejected in standalone mode. Duplicate timestamps leave the current time intact. An older tick is rejected. Explicit seek establishes a new current instant in either direction. Subsequent ticks are compared with that instant. The adapter is responsible for keeping stale events from a previous playback sequence out of the ordered stream.

Pause and resume are idempotent. Pause holds automatic advancement, and an external tick received while paused is rejected without buffering it. An explicit seek can change UTC while paused and preserves the pause state. Standalone resume starts elapsed-time advancement from the held or sought instant. External resume waits for the next valid tick. Rejected events leave the clock state intact.

The runner samples absolute UTC and computes orbital state from the fixed inputs at that instant. Replaying the same ordered events yields the same external timestamps, so a backward seek requires no mutation of the input orbital records. The clock port imposes no wire protocol. HOTH-015 implements the clock drivers.

## Publisher lifecycle and pressure

Use a synchronous, object-safe port for each publisher instance. A submission accepts an ordered slice of canonical states sharing the supplied timestamp. The port permits a timestamp to repeat or decrease on replay.

Initialization precedes submissions and flush. Failed initialization still permits cleanup. Successful submission acknowledges acceptance into the local publisher. Any internal queue is bounded. Capacity pressure rejects the full batch before acceptance and returns `Backpressure`. The caller can retry the rejected batch before newer submissions or drop it. A sink error may follow a partial delivery. The caller shuts down that publisher and avoids automatically retrying an uncertain batch.

Flush attempts to drain accepted work before shutdown and can fail. Local acceptance and a successful flush give no remote-delivery guarantee. Shutdown is terminal, releases resources, and discards work left pending. Repeated shutdown is safe. Transport adapters define their encoding and I/O behavior behind the port.

## Compatibility

`SimulationUtc` and `SatelliteState` retain their existing representations. The scenario YAML values `standalone` and `external` retain their spellings, and `hoth_config::ClockMode` re-exports the shared authority type. The ports add no transport dependencies.
