#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

if (( $# )); then
    exec cargo run --locked --quiet -p hoth --example orbit_globe -- "$@"
fi

fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
hoth=(cargo run --locked --quiet -p hoth --bin hoth --)
"${hoth[@]}" catalog import --database "$fixture/catalog.sqlite" \
    --format omm-json --file tests/fixtures/catalog/population.json \
    --source local-fixture --acquired-at 2026-11-17T00:00:00Z
"${hoth[@]}" catalog snapshot --database "$fixture/catalog.sqlite" --month 2021-11
"${hoth[@]}" scenario prepare --database "$fixture/catalog.sqlite" \
    --config tests/fixtures/scenarios/globe.yaml --output "$fixture/mission.json"

mkdir -p data
output=data/orbit-globe.html
if [[ -e $output || -L $output ]]; then
    output="$(mktemp -d data/orbit-globe.XXXXXX)/index.html"
fi
cargo run --locked --quiet -p hoth --example orbit_globe -- \
    --manifest "$fixture/mission.json" --output "$output" \
    --norad 70000 --norad 70001 --norad 70002 --norad 70003
