# Kubernetes deployment

Build and publish or transfer the production image.

```sh
docker build -f deploy/Dockerfile --target production -t hoth:0.1.0 .
helm lint deploy/helm/hoth
helm template simulation deploy/helm/hoth > rendered.yaml
kubeconform -strict -summary -kubernetes-version 1.32.0 rendered.yaml
```

Provide an existing PVC with the manifest at `storage.manifestPath` and read permissions for UID 10001. The PVC can also hold a validated SQLite catalog used for preparation. The run command only loads the manifest. Set `image.repository` and `image.tag` to the image available to your execution nodes. The chart creates a finite Job by default. For an orchestrator that supervises restarts, `workload=Deployment` enables a singleton Deployment with a Recreate rollout. Restarting the pod starts a new run from the prepared manifest. Use a new Job for a new manifest.

The pod runs without service-account credentials with a read-only root filesystem. A small memory-backed volume holds a heartbeat. Readiness appears after manifest and publishers initialize. Liveness checks the runtime heartbeat. In external mode the service is ready for controller commands before the first tick.

Set `control.enabled=true` and provide a `runId` for managed execution. The service exposes the local control adapter inside the cluster. Use an isolated network and explicit controller ingress. Configure `networkPolicy.enabled` and supply lists of `{cidr, port}` entries for `networkPolicy.dis` and `networkPolicy.zenoh`, and CIDR strings for `networkPolicy.controllers`. Policies must match the destinations in the prepared scenario. DIS defaults to unicast. Multicast requires explicit YAML opt-in and validation with the deployed network. A policy's effective treatment of Service IPs depends on the CNI's address translation behavior.

Static Helm and Kubernetes schema checks exercise both Job and Deployment configurations. No cluster is needed for those checks.
