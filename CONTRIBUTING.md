# Contributing

1. Keep changes focused and preserve the global-node product invariants in README.
2. Run `bash scripts/check.sh` before opening a pull request.
3. Keep Clippy warning-free; do not suppress warnings without a documented reason.
4. Add tests for changes to log classification, node eligibility, routing phases or failover behavior.
5. Do not add another push-triggered GitHub workflow; CI/CD is intentionally single-pipeline.
