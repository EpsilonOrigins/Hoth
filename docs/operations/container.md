# HOTH container package

Build the package from the repository root with Docker BuildKit.

```sh
docker build -f deploy/Dockerfile --target package -t hoth:local .
docker run --rm -p 127.0.0.1:8080:8080 hoth:local
```

Open `http://localhost:8080` to see all 100 satellites in the bundled 95-minute replay. The image includes the HOTH CLI, a prepared 100-object SQLite catalog and manifests, the globe generator, the compiled Vallado reference suite, and the benchmark and telemetry verifier. The builder compiles the artifacts and runs the reference suite. The runtime uses Debian slim as UID 10001 and serves the demo with BusyBox HTTP.

## Commands

```sh
docker run --rm hoth:local --help
docker run --rm --network none hoth:local validate-orbits --nocapture
docker run --rm --network none hoth:local globe > orbit-globe.html
docker run --rm --network none hoth:local catalog validate --database /opt/hoth/demo/catalog.sqlite
docker run --rm --network none hoth:local run --manifest /opt/hoth/demo/mission.json
docker run --rm --network none hoth:local benchmark 1000 25 60
```

The standalone fixture sends DIS to UDP port 3000 on container loopback. Use Compose for a verified receiver and router. To use your own manifest with the container, mount it and use its in-container path. A host bind mount must give UID 10001 read access to inputs and write access to output directories. A Docker named volume mounted at `/data` can hold a writable catalog.

```sh
docker run --rm --network none \
  -v "$PWD/data:/data" hoth:local \
  globe --manifest /data/mission.json --output /data/custom-globe.html --norad 25544
```

## Integration and deployment

The Compose harness uses the same package image for the runtime and verifier. It starts a local Zenoh router, checks both transports and controller playback, and returns nonzero if either required container fails. The smoke script also verifies the packaged commands on an isolated network.

```sh
bash deploy/smoke.sh
HOTH_SKIP_BUILD=1 bash scripts/e2e.sh
```

Run `bash scripts/e2e.sh` to build through Compose first. Both scripts accept `HOTH_IMAGE` to select a different tag. The Zenoh router image is a separate Compose service. Preload `eclipse/zenoh:1.7.2` to run Compose in an air-gapped environment. The `production` target packages the runtime CLI for Helm. [Kubernetes deployment](kubernetes.md)

## Transfer or build with a pre-fetched registry

Transfer both the package and the Compose router in one archive with `docker save` and `docker load`.

```sh
docker save hoth:local eclipse/zenoh:1.7.2 | gzip -1 > hoth-container.tar.gz
docker load -i hoth-container.tar.gz
```

If the build network cannot reach crates.io, fetch the locked dependencies in a trusted environment first and supply its Cargo registry as a read-only build context. Populate the registry with `cargo fetch --locked` for the target platform. The BuildKit frontend and base images must be available to the builder.

```sh
docker build --network=none -f deploy/Dockerfile --target package \
  --build-context "cargo-registry=${CARGO_HOME:-$HOME/.cargo}/registry" \
  -t hoth:local .
```

For a proxy that requires a private CA, you can instead supply a trusted certificate bundle using `--secret id=cargo_ca,src=/path/to/ca-bundle.pem`. The registry and any supplied CA are used in the build stage.
