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
wait_for_service() {
    local service=$1 limit=$2 id status
    id=$("${compose[@]}" ps -aq "$service")
    [[ -n $id ]]
    status=$(timeout "$limit" docker wait "$id")
    if [[ $status != 0 ]]; then
        printf '%s exited with status %s\n' "$service" "$status" >&2
        return 1
    fi
}
wait_for_service verify 45
wait_for_service hoth 15
