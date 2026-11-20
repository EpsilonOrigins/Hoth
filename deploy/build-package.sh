#!/bin/sh
set -eu

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
release="$CARGO_TARGET_DIR/release"
bin=/package/usr/local/bin
demo=/package/opt/hoth/demo
globe=/package/opt/hoth/globe

cargo build --release --locked --offline -p hoth --bin hoth --examples
cargo test --release --locked --offline --workspace --test reference \
    --no-run --message-format=json > "$scratch/artifacts.json"
reference=$(sed -n '/"name":"reference"/s/.*"executable":"\([^"]*\)".*/\1/p' "$scratch/artifacts.json")
if [ ! -x "$reference" ]; then
    printf 'Expected one compiled propagation reference executable\n' >&2
    exit 1
fi
"$reference" --nocapture

install -d "$bin" "$demo" "$globe"
install -m 0755 "$release/hoth" "$bin/hoth"
install -m 0755 "$release/examples/orbit_globe" "$bin/hoth-orbit-globe"
install -m 0755 "$release/examples/pipeline_bench" "$bin/hoth-benchmark"
install -m 0755 "$release/examples/verify" "$bin/hoth-verify"
install -m 0755 "$reference" "$bin/hoth-reference"
install -m 0755 deploy/package-entrypoint.sh "$bin/hoth-container"
install -m 0755 deploy/run-fixture.sh "$bin/run-fixture"
cp -R tests/fixtures /package/fixtures
chmod -R a+rX /package/fixtures

"$bin/hoth" catalog import --database "$scratch/catalog.sqlite" --format omm-json \
    --file tests/fixtures/catalog/population.json --source local-fixture \
    --acquired-at 2026-11-17T00:00:00Z
"$bin/hoth" catalog snapshot --database "$scratch/catalog.sqlite" --month 2021-11
"$bin/hoth" catalog export --database "$scratch/catalog.sqlite" --output "$demo/catalog.sqlite"
"$bin/hoth" scenario prepare --database "$demo/catalog.sqlite" \
    --config tests/fixtures/scenarios/standalone.yaml --output "$demo/mission.json"
"$bin/hoth" scenario prepare --database "$demo/catalog.sqlite" \
    --config tests/fixtures/scenarios/globe.yaml --output "$demo/globe.json"
"$bin/hoth-orbit-globe" --manifest "$demo/globe.json" --output "$globe/index.html"
chmod 0644 "$demo/"* "$globe/index.html"
