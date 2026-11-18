# ADR 0006 — Space-Track import checkpoints

## Status

Proposed for HOTH-008.

## Decision

Use bounded GP History queries over completed UTC months and explicit NORAD sets. Page in ascending provider GP ID order. Validate that the returned IDs advance and that each orbit falls within the requested object scope and epoch window. Retain the exact JSON object bytes for the shared orbital-element parser and trace each saved row to its acquisition page.

Add a SQLite checkpoint keyed by the canonical object set and month. Commit the cursor and coverage with the page's orbital rows in one transaction. A completed window is reused on subsequent runs with the same scope. Retain the first stored row for a provider record ID and report an error if that ID is later returned with different bytes.

## Compatibility

Migration 3 adds the checkpoint table and preserves the existing orbital and snapshot schema. Acquisition runs when the operator invokes the bootstrap command. Scenario preparation and the simulation use the local catalog. See the [Space-Track import guide](../architecture/space-track.md).
