#!/usr/bin/env bash
set -euo pipefail

compose_run=(docker compose -f deploy/compose.yaml run --rm --no-deps --no-build --pull never -T)

banner=$("${compose_run[@]}" hoth)
if [[ $banner != "HOTH — High-Throughput Orbit Telemetry Harness (foundation scaffold)" ]]; then
    printf 'Unexpected scaffold output: %s\n' "$banner" >&2
    exit 1
fi

uid=$("${compose_run[@]}" --entrypoint id hoth -u)
if [[ $uid != 10001 ]]; then
    printf 'Expected the container to run as UID 10001, got %s\n' "$uid" >&2
    exit 1
fi

status=0
error=$("${compose_run[@]}" hoth --invalid 2>&1) || status=$?
if [[ $status != 2 || $error != *"hoth --help"* ]]; then
    printf 'Expected exit 2 and usage guidance, got exit %s and output %s\n' "$status" "$error" >&2
    exit 1
fi

printf 'Compose smoke passed (scaffold, UID %s, invalid argument exit %s).\n' "$uid" "$status"
