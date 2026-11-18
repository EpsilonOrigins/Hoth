#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
compose=(docker compose -p "hoth-e2e-$$" -f deploy/compose.yaml)
if [[ -n ${HOTH_COMPOSE_OVERRIDE:-} ]]; then
    compose+=(-f "$HOTH_COMPOSE_OVERRIDE")
fi
finish() {
    status=$?
    trap - EXIT
    "${compose[@]}" logs --no-color || true
    "${compose[@]}" down --timeout 5 || true
    exit "$status"
}
trap finish EXIT
if [[ ${HOTH_SKIP_BUILD:-0} != 1 ]]; then
    "${compose[@]}" build hoth
fi
"${compose[@]}" up -d --no-build
timeout 45 "${compose[@]}" wait verify
timeout 15 "${compose[@]}" wait hoth
for service in verify hoth; do
    id=$("${compose[@]}" ps -aq "$service")
    [[ -n $id && $(docker inspect --format '{{.State.ExitCode}}' "$id") == 0 ]]
done
