# Architecture

Ashan FRP Control Center is a single-process Rust control plane plus a managed `frpc` child process.

## Source of truth

`tunnel_plans` in SQLite defines tunnel name, local endpoint, protocol, public identifier/domain and DNS ownership. A tunnel never stores a node identifier. `routing_state` is a singleton that owns the global active/standby node relationship.

## Execution layers

1. ChmlFrp: materializes all planned tunnels on the current global node and returns a complete frpc configuration.
2. FRPC: runs that complete configuration and emits runtime evidence.
3. Cloudflare: converges managed A records to the verified active node IP.

## Consistency model

Observe -> Compare -> Plan -> Apply -> Verify. Reconcile repairs desired/observed drift. Emergency failover requires all planned ChmlFrp resources to exist before mutation.

## WebUI

`web/` contains static assets. Axum serves them with `ServeDir`; the browser calls `/api/v1/*` and receives FRPC events through SSE. Removing a compiled frontend eliminates a second language/toolchain and keeps the release artifact deterministic.
