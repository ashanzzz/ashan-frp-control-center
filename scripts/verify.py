#!/usr/bin/env python3
"""Offline repository checks that do not require a Rust toolchain."""
from __future__ import annotations
import sqlite3
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
errors: list[str] = []

for path in ROOT.rglob('*.toml'):
    try:
        tomllib.loads(path.read_text(encoding='utf-8'))
    except Exception as exc:
        errors.append(f'TOML {path.relative_to(ROOT)}: {exc}')

try:
    import yaml  # type: ignore
except Exception:
    yaml = None

if yaml:
    for pattern in ('*.yaml', '*.yml'):
        for path in ROOT.rglob(pattern):
            try:
                yaml.safe_load(path.read_text(encoding='utf-8'))
            except Exception as exc:
                errors.append(f'YAML {path.relative_to(ROOT)}: {exc}')

migration = ROOT / 'migrations' / '0001_init.sql'
try:
    db = sqlite3.connect(':memory:')
    db.executescript(migration.read_text(encoding='utf-8'))
    cols = {row[1] for row in db.execute('PRAGMA table_info(tunnel_plans)')}
    forbidden = {'active_node_id', 'standby_node_id', 'node_id'}
    if cols & forbidden:
        errors.append(f'per-tunnel node columns are forbidden: {sorted(cols & forbidden)}')
    row = db.execute('SELECT singleton_id, quarantine_days FROM routing_state').fetchone()
    if row != (1, 30):
        errors.append(f'unexpected routing_state seed: {row}')
finally:
    try:
        db.close()
    except Exception:
        pass

# Guard the core product invariant in source text.
all_rust = '\n'.join(p.read_text(encoding='utf-8') for p in ROOT.rglob('*.rs'))
if 'GLOBAL_FAILOVER' not in all_rust:
    errors.append('GLOBAL_FAILOVER invariant marker missing')
if 'wait_ready' not in all_rust:
    errors.append('FRPC readiness verification missing')
if 'sync_tunnel' not in all_rust:
    errors.append('global ChmlFrp reconcile operation missing')
if 'DnsPatch' not in all_rust or '.json(&DnsPatch { content: ip })' not in all_rust:
    errors.append('Cloudflare content-only PATCH preservation missing')
if 'active_node_id' in (ROOT / 'migrations' / '0001_init.sql').read_text(encoding='utf-8'):
    errors.append('per-tunnel active node leaked into schema')

workflow_dir = ROOT / '.github/workflows'
workflow_files = sorted([*workflow_dir.glob('*.yml'), *workflow_dir.glob('*.yaml')])
expected_workflow = workflow_dir / 'ci.yml'
if workflow_files != [expected_workflow]:
    errors.append(
        'exactly one GitHub workflow is allowed: .github/workflows/ci.yml; found: '
        + ', '.join(str(p.relative_to(ROOT)) for p in workflow_files)
    )
else:
    workflow_text = expected_workflow.read_text(encoding='utf-8')
    if 'branches: [main]' not in workflow_text:
        errors.append('CI workflow must be scoped to main for push/PR automation')
    push_block = workflow_text.split('  pull_request:', 1)[0]
    if '\n    tags:' in push_block or '\n      tags:' in push_block:
        errors.append('tag-triggered CI is forbidden because it duplicates the main-branch build')
    if 'run: bash scripts/build-web.sh' not in workflow_text:
        errors.append('CI must invoke the web build explicitly with bash scripts/build-web.sh')
    if 'run: bash scripts/stage-release.sh' not in workflow_text:
        errors.append('CI must invoke release staging explicitly with bash scripts/stage-release.sh')
    if 'docker/build-push-action@v6' not in workflow_text:
        errors.append('CI must package/publish the runtime image with build-push-action@v6')

coordinator = (ROOT / 'apps/server/src/coordinator.rs').read_text(encoding='utf-8')
if 'queued_global_failover' not in coordinator or 'runtime_generation' not in coordinator:
    errors.append('generation-safe queued global failover is missing')
if 'promote_active_node' not in coordinator or 'finalize_active_node' not in coordinator:
    errors.append('runtime-before-DNS active-node commit path is missing')
if 'ALL_MANAGED_TUNNELS' in coordinator:
    pass
# There must be one node target for the complete plan set, never a per-tunnel node assignment.
if 'active_node_id' in all_rust or 'standby_node_id' in all_rust:
    errors.append('per-tunnel-style node identifier leaked into Rust source')

# Obvious unfinished implementation markers are forbidden in a release archive.
for token in ('TODO', 'FIXME', 'unimplemented!()', 'todo!()'):
    hits = []
    for path in ROOT.rglob('*'):
        if path.is_file() and '.git' not in path.parts:
            try:
                text = path.read_text(encoding='utf-8')
            except Exception:
                continue
            if token in text and path != Path(__file__):
                hits.append(str(path.relative_to(ROOT)))
    if hits:
        errors.append(f'{token} found in: {hits}')


# Build-path invariants: Dioxus must always target the web binary explicitly,
# and the Dockerfile must only package already-verified artifacts.
# Shell scripts are intentionally treated as data files: CI and helper scripts must
# invoke them through bash and never depend on Git executable-bit preservation.
for shell_path in ROOT.glob('scripts/*.sh'):
    mode = shell_path.stat().st_mode
    # Filesystem mode is not authoritative across ZIP/OS boundaries, so source calls
    # are checked below instead of requiring +x.

for caller in (ROOT / '.github/workflows/ci.yml', ROOT / 'scripts/verify.sh', ROOT / 'scripts/build-release.sh'):
    text = caller.read_text(encoding='utf-8')
    if './scripts/' in text:
        errors.append(f'direct shell-script execution is forbidden; use bash scripts/... in {caller.relative_to(ROOT)}')

build_web = (ROOT / 'scripts' / 'build-web.sh').read_text(encoding='utf-8')
if '--package ashan-frp-web' not in build_web:
    errors.append('Dioxus web build must explicitly select --package ashan-frp-web')
if 'dx build' in (ROOT / 'Dockerfile').read_text(encoding='utf-8') or 'cargo build' in (ROOT / 'Dockerfile').read_text(encoding='utf-8'):
    errors.append('Dockerfile must be runtime-only and must not rebuild Rust/Dioxus')
if 'COPY .release/ashan-frp-server' not in (ROOT / 'Dockerfile').read_text(encoding='utf-8'):
    errors.append('Dockerfile must consume the staged verified server artifact')

if errors:
    print('STATIC VERIFY FAILED')
    for error in errors:
        print(' -', error)
    sys.exit(1)
print('STATIC VERIFY OK')
