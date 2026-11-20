#!/bin/sh
set -eu

case "${1:-}" in
    serve-globe)
        if [ "$#" -ne 1 ]; then
            printf 'Usage: serve-globe\n' >&2
            exit 2
        fi
        exec busybox httpd -f -p 8080 -h /opt/hoth/globe
        ;;
    globe)
        shift
        if [ "$#" -eq 0 ]; then
            exec cat /opt/hoth/globe/index.html
        fi
        exec hoth-orbit-globe "$@"
        ;;
    validate-orbits)
        shift
        exec hoth-reference "$@"
        ;;
    benchmark)
        shift
        exec hoth-benchmark "$@"
        ;;
    --help|-h)
        hoth "$@"
        cat <<'HELP'

Container commands
  serve-globe       Serve the included demo on port 8080 (default)
  globe             Write the bundled globe HTML to stdout
  globe --manifest PATH --output PATH [--step-seconds SECONDS] [--norad ID]
                    Generate a globe for a prepared manifest
  globe --manifest PATH --serve IP:PORT [--step-ms MS]
                    Serve on-demand replay or add --zenoh IP:PORT --run-id ID for live telemetry
  validate-orbits   Run the offline propagation reference suite
  benchmark         Run the pipeline benchmark
HELP
        ;;
    *) exec hoth "$@" ;;
esac
