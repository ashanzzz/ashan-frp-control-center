# Global failover

There is only one failover scope: **the complete managed tunnel set**.

## Trigger

FRPC logs are parsed into structured events. Node-scoped events such as confirmed remote session/control failures trigger failover immediately. Ambiguous network failures are promoted only when ChmlFrp independently reports the active node offline. Local-service, configuration and authentication failures never trigger a node switch.

## Transaction order

1. Acquire the global operation lock and reject stale FRPC generations.
2. Quarantine the failed active node for the configured period.
3. Select the preferred healthy standby, excluding active/disabled/quarantined/offline nodes.
4. Preflight Cloudflare uniqueness and target ChmlFrp node health.
5. Move **every enabled managed ChmlFrp tunnel** to the same target node.
6. Verify all ChmlFrp resources match the target node and tunnel plan.
7. Request one complete target-node FRPC configuration from ChmlFrp.
8. Restart FRPC and verify login plus startup of every planned tunnel.
9. Commit the verified target as runtime Active.
10. Update every managed Cloudflare A record to the target node IP.
11. Finalize routing state to `idle`.

If DNS fails after step 9, the state becomes `degraded_dns`; runtime truth remains the new node and a later reconcile completes DNS convergence.

## Forward recovery

A candidate is quarantined only after a typed node/runtime failure. Provider/config/DNS failures do not quarantine the candidate. When candidate B has a confirmed node failure, the coordinator proceeds forward to C instead of rolling traffic back to the already-failed A.

## Runtime rollback rules

Automatic failover starts from a node that has already been confirmed faulty. Once that failure is accepted, the old FRPC runtime is stopped and must not be restarted as a rollback target. A confirmed failed candidate is also stopped before forward recovery continues.

Manual failover is different: the original node is not considered faulty. If a candidate fails before runtime commit, the coordinator restores the original global state. If another candidate will be attempted, that original healthy baseline is restored first so a later error can never restore a quarantined candidate configuration.

Clearing or expiring quarantine only makes a node eligible for future selection; it never moves traffic back automatically.
