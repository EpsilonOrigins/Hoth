#!/bin/sh
set -eu
data=$(mktemp -d)
trap 'rm -rf "$data"' EXIT
hoth catalog import --format omm-json --file /fixtures/catalog/population.json \
    --source local-fixture --acquired-at 2026-11-17T00:00:00Z --database "$data/catalog.sqlite"
hoth catalog snapshot --month 2021-11 --database "$data/catalog.sqlite"
hoth catalog validate --database "$data/catalog.sqlite"
hoth scenario prepare --database "$data/catalog.sqlite" --config /fixtures/scenarios/compose.yaml \
    --output "$data/mission.json"
exec hoth run --manifest "$data/mission.json" --run-id compose --control 0.0.0.0:4123 \
    --ready-file "$data/ready"
