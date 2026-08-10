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

required_workflows = [
    ROOT / '.github/workflows/ci.yml',
    ROOT / '.github/workflows/build.yml',
    ROOT / '.github/workflows/build-push.yml',
]
for workflow in required_workflows:
    if not workflow.exists():
        errors.append(f'missing GitHub workflow: {workflow.relative_to(ROOT)}')

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

if errors:
    print('STATIC VERIFY FAILED')
    for error in errors:
        print(' -', error)
    sys.exit(1)
print('STATIC VERIFY OK')
