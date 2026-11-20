#!/usr/bin/env bash
set -euo pipefail

image=${HOTH_IMAGE:-hoth:local}
run=(docker run --rm --network none --read-only --tmpfs /tmp)

[[ $("${run[@]}" --entrypoint id "$image" -u) == 10001 ]]
"${run[@]}" "$image" --version
"${run[@]}" "$image" --help | grep -F 'validate-orbits'
"${run[@]}" "$image" validate-orbits --nocapture
"${run[@]}" "$image" catalog validate --database /opt/hoth/demo/catalog.sqlite
"${run[@]}" "$image" run --manifest /opt/hoth/demo/mission.json --ready-file /tmp/ready
"${run[@]}" "$image" benchmark 5 10 1

status=0
error=$("${run[@]}" "$image" --invalid 2>&1) || status=$?
if [[ $status != 2 || $error != *"hoth --help"* ]]; then
    printf 'Expected exit 2 and usage guidance, got exit %s and output %s\n' "$status" "$error" >&2
    exit 1
fi

"${run[@]}" --entrypoint /bin/sh "$image" -ec '
    hoth-container globe --manifest /opt/hoth/demo/globe.json \
        --output /tmp/generated.html --norad 70001
    grep -qF "HOTH Orbit Test Harness" /tmp/generated.html
    test "$(grep -oF "\"norad\":" /tmp/generated.html | wc -l)" -eq 1
    hoth-container globe > /tmp/exported.html
    test "$(grep -oF "\"norad\":" /tmp/exported.html | wc -l)" -eq 100
    hoth-container serve-globe &
    server=$!
    trap "kill $server 2>/dev/null || true" EXIT
    served=false
    for attempt in 1 2 3 4 5; do
        if busybox wget -Y off -T 1 -q -O /tmp/served.html http://127.0.0.1:8080/ 2>/dev/null; then
            cmp /tmp/served.html /tmp/exported.html
            served=true
            break
        fi
        sleep 1
    done
    if ! "$served"; then
        printf "Globe HTTP server did not respond\n" >&2
        exit 1
    fi
    kill "$server"
    wait "$server" || true
    hoth-container globe --manifest /opt/hoth/demo/globe.json --serve 127.0.0.1:8080 --step-ms 40 &
    server=$!
    trap "kill $server 2>/dev/null || true" EXIT
    for attempt in 1 2 3 4 5; do
        if busybox wget -Y off -T 1 -q -O /tmp/frame.json http://127.0.0.1:8080/api/frame/1 2>/dev/null; then
            grep -qF "2026-11-17T00:00:00.040Z" /tmp/frame.json
            exit 0
        fi
        sleep 1
    done
    printf "On-demand replay server did not respond\n" >&2
    exit 1
'
printf 'Container smoke passed (nonroot CLI, offline reference, runtime, benchmark and globe).\n'
