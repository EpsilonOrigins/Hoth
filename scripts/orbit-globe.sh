#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

scenario=tests/fixtures/scenarios/globe.yaml
format=omm-json
population=tests/fixtures/catalog/population.json
month=2021-11
source=local-fixture
output=data/orbit-globe.html
options=()
if [[ ${1:-} == --reference-demo && $# == 1 ]]; then
    scenario=tests/fixtures/scenarios/globe-vallado.yaml
    format=tle
    population=crates/hoth-orbits/tests/fixtures/vallado/verification.tle
    month=2000-06
    source=vallado-verification
    output=data/orbit-globe-reference.html
    options=(--norad 5 --step-seconds 60)
elif (( $# )); then
    exec cargo run --locked --quiet -p hoth --example orbit_globe -- "$@"
fi

fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
hoth=(cargo run --locked --quiet -p hoth --bin hoth --)
"${hoth[@]}" catalog import --database "$fixture/catalog.sqlite" \
    --format "$format" --file "$population" \
    --source "$source" --acquired-at 2026-11-17T00:00:00Z
"${hoth[@]}" catalog snapshot --database "$fixture/catalog.sqlite" --month "$month"
"${hoth[@]}" scenario prepare --database "$fixture/catalog.sqlite" \
    --config "$scenario" --output "$fixture/mission.json"

mkdir -p data
if [[ -e $output || -L $output ]]; then
    output="$(mktemp -d "${output%.html}.XXXXXX")/index.html"
fi
cargo run --locked --quiet -p hoth --example orbit_globe -- \
    --manifest "$fixture/mission.json" --output "$output" "${options[@]}"
