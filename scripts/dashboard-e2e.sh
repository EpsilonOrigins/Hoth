#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

inputs=$(mktemp -d)
chmod 0755 "$inputs"
export HOTH_INPUT_DIR="$inputs" HOTH_DASHBOARD_PORT=0 HOTH_RUN_ID=dashboard-e2e
export HOTH_SIM_SUBNET=172.29.188.0/24 HOTH_DASHBOARD_IP=172.29.188.12 HOTH_ZENOH_IP=172.29.188.10
export HOTH_ELEMENTS_PATH=/inputs/elements.json HOTH_ELEMENTS_FORMAT=omm-json
compose=(docker compose -p "hoth-dashboard-e2e-$$")

cleanup() {
    local status=$?
    trap - EXIT
    if (( status != 0 )); then
        "${compose[@]}" logs --no-color hoth dashboard || true
    fi
    "${compose[@]}" down --volumes --timeout 5
    rm -rf "$inputs"
    exit "$status"
}
trap cleanup EXIT

sed 's/172\.29\.187\./172.29.188./g' deploy/dashboard-inputs/scenario.yaml > "$inputs/scenario.yaml"
cp tests/fixtures/catalog/population.json "$inputs/elements.json"
chmod 0644 "$inputs/"*.{yaml,json}

if [[ ${HOTH_SKIP_BUILD:-0} != 1 ]]; then
    "${compose[@]}" build hoth dashboard
fi
up() {
    "${compose[@]}" up --wait --wait-timeout 60 --no-build "$@"
}

verify() {
    local expected_start="$1" source_date="$2" first_range="$3"
    local address
    address=$("${compose[@]}" port dashboard 8080)
    python3 - "$address" "$expected_start" "$source_date" "$first_range" <<'PY'
import json
import re
import struct
import sys
import time
from datetime import datetime
from urllib.request import urlopen

address, start, source_date, first_range = sys.argv[1:]
base = f"http://{address}"

def get(path):
    with urlopen(base + path, timeout=5) as response:
        return response.read()

data = json.loads(re.search(
    rb'<script id="mission-data" type="application/json">(.*?)</script>', get("/")
)[1])
assert data["timeline"]["start"] == start, data["timeline"]
assert data["wire_monitor"] and data["timeline"]["live"]
assert len(data["tracks"]) == len({track["norad"] for track in data["tracks"]}) == 100
assert data["tracks"][0]["provenance"]["source_epoch_utc"].startswith(source_date)

radar = data["radars"][0]
origin = radar["origin"]
# The first range-boundary coordinate is the minimum elevation and azimuth.
distance = sum((axis - center) ** 2 for axis, center in zip(radar["lines"][0][0], origin)) ** 0.5
assert abs(distance - float(first_range)) < 1e-6, distance

deadline = time.monotonic() + 12
while True:
    wire = json.loads(get("/api/dis/70000"))
    live = json.loads(get("/api/live"))
    if wire["packet"] and live["frame"] and wire["counters"]["entities_seen"] == 100:
        break
    if time.monotonic() >= deadline:
        raise AssertionError("did not receive both UDP and Zenoh telemetry")
    time.sleep(0.15)

assert wire["counters"]["accepted"] >= 100
packet = wire["packet"]
pdu = packet["pdu"]
raw = bytes.fromhex(" ".join(line[6:] for line in packet["hex"].splitlines()))
assert len(raw) == 144 == pdu["length_bytes"]
assert struct.unpack_from("!BBBB", raw) == (7, 37, 1, 1)
assert struct.unpack_from("!HHH", raw, 12) == (5, 12, 1)
assert pdu["entity_id"] == {"site": 5, "application": 12, "entity": 1}
assert int(raw[129:140].rstrip(b"\x00")) == packet["norad_id"] == 70000
assert struct.unpack_from("!ddd", raw, 48) == tuple(pdu["position_ecef_m"])
assert raw[36:48] == struct.pack("!fff", *pdu["velocity_ecef_m_per_s"])
assert struct.unpack_from("!I", raw, 4)[0] == pdu["timestamp_raw"]
assert pdu["timestamp_absolute"] and pdu["timestamp_seconds_into_hour"] >= datetime.fromisoformat(start).minute * 60
assert datetime.fromisoformat(live["frame"]["utc"]) >= datetime.fromisoformat(start)
assert len(live["frame"]["states"]) == 100 and all(live["frame"]["states"])
print(f"Verified source epoch {source_date}, simulation epoch {start}, FOV range {first_range} m and decoded UDP plus Zenoh.")
PY
}

up
"${compose[@]}" exec -T hoth sh -ec 'command -v hoth >/dev/null && ! command -v hoth-orbit-globe >/dev/null'
"${compose[@]}" exec -T dashboard sh -ec 'command -v hoth-orbit-globe >/dev/null && ! command -v hoth >/dev/null'
verify 2026-11-17T00:00:00Z 2021-11-15 20000

sed -i 's/2026-11-17T00:00:00Z/2026-11-18T00:15:00Z/; s/range_min_m: 20000/range_min_m: 40000/' "$inputs/scenario.yaml"
sed -i 's/2021-11-15T00:00:00Z/2021-11-16T00:00:00Z/g' "$inputs/elements.json"
up --force-recreate hoth dashboard
verify 2026-11-18T00:15:00Z 2021-11-16 40000
