# Operations

## Persistent data

Back up `/data` as one unit. It contains SQLite state, FRPC configuration revisions and the FRPC runtime files. On Unraid use `/mnt/cache/appdata/ashan-frp-control-center`.

## Upgrade

Pull the new GHCR image and restart the container. Database migrations run automatically on startup. Do not delete `/data` during upgrades.

## Recovery states

- `idle`: converged steady state.
- `failover`: global failover in progress.
- `dns_switching`: target runtime verified; DNS is being changed.
- `degraded_dns`: target runtime is Active but DNS convergence failed. Run Global Reconcile after fixing Cloudflare.
- `failed`: failure before target runtime was committed; inspect Activity and FRPC logs.

## FRPC restart persistence

If the FRPC binary and the last generated configuration exist when the container starts, the service automatically starts FRPC. FRPC startup failure does not prevent the WebUI from starting.

## Quarantine management

Use the ChmlFrp page or `POST /api/v1/nodes/{name}/unquarantine` to clear a quarantine early. This only returns the node to the candidate pool. The current Active node does not change until a future explicit/global failover selects another node.

## Provider outages

The dashboard distinguishes a provider API outage from a missing resource. Fix ChmlFrp/Cloudflare connectivity before using missing-resource indicators for repair decisions. Provider HTTP requests have finite connect/request timeouts so a stalled upstream cannot hold the global operation indefinitely.

## Safe tunnel deletion

Deleting a plan is deliberately conservative. If the same ChmlFrp tunnel still exists, or a managed Cloudflare A record still exists, the API refuses to remove the local plan. Clean up the external resource first, then delete the plan. This keeps the local source of truth from losing ownership of an orphaned resource.

## Dependency lock

`Cargo.lock` is committed and release builds use `--locked`. If a dependency update changes the lockfile, merge it only after the pull-request CI passes tests, strict Clippy and the release build.
