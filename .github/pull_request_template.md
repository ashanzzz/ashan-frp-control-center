## Summary

Describe the change and why it is needed.

## Product invariants

- [ ] No per-tunnel node assignment was introduced.
- [ ] Any node failover still migrates the complete managed tunnel set.
- [ ] Cloudflare DNS remains after ChmlFrp + FRPC runtime verification.
- [ ] Local/config/auth FRPC errors do not trigger node failover.

## Verification

- [ ] `bash scripts/check.sh`
- [ ] Tests added or updated where behavior changed.
- [ ] README/docs updated for user-visible or operational changes.
