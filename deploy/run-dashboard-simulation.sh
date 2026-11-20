#!/bin/sh
set -eu

scratch=$(mktemp -d)
stage=$(mktemp -d /mission/.prepare.XXXXXX)
trap 'rm -rf "$scratch" "$stage"' EXIT
config="$scratch/scenario.yaml"
if [ ! -r /inputs/scenario.yaml ]; then
    echo "mounted /inputs/scenario.yaml must be readable by UID 10001" >&2
    exit 1
fi
cp /inputs/scenario.yaml "$config"
elements=${HOTH_ELEMENTS_PATH:-/inputs/elements.json}
if [ ! -e "$elements" ] && [ -z "${HOTH_ELEMENTS_PATH:-}" ]; then
    elements=/opt/hoth/population.json
fi

hoth catalog import --database "$scratch/catalog.sqlite" --file "$elements" \
    --format "${HOTH_ELEMENTS_FORMAT:-omm-json}" --source mounted-input
hoth catalog snapshot --config "$config" --database "$scratch/catalog.sqlite"
hoth scenario prepare --config "$config" --database "$scratch/catalog.sqlite" \
    --output "$stage/manifest.json"
mv "$stage/manifest.json" /mission/manifest.json
rm -rf "$scratch" "$stage"
trap - EXIT

# The dashboard can now load the manifest and bind its receiver before the
# runtime sends its first batch.
touch /tmp/hoth-manifest.ready
attempt=0
until busybox wget -Y off -T 1 -q -O /dev/null "http://${HOTH_DASHBOARD_IP:?}:8080/healthz"; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 30 ]; then
        echo "dashboard did not open its telemetry receiver" >&2
        exit 1
    fi
    sleep 1
done

exec hoth run --manifest /mission/manifest.json --run-id "${HOTH_RUN_ID:-dashboard}" \
    --control 0.0.0.0:4123 --ready-file /tmp/hoth.ready
