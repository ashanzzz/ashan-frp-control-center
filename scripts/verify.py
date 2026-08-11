#!/usr/bin/env python3
from __future__ import annotations
import sqlite3
import sys
import tomllib
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
errors: list[str] = []

for path in ROOT.rglob("*.toml"):
    try:
        tomllib.loads(path.read_text(encoding="utf-8"))
    except Exception as exc:
        errors.append(f"TOML {path.relative_to(ROOT)}: {exc}")

migrations = sorted((ROOT / "migrations").glob("*.sql"))
db = sqlite3.connect(":memory:")
try:
    for migration in migrations:
        db.executescript(migration.read_text(encoding="utf-8"))
    columns = {row[1] for row in db.execute("PRAGMA table_info(tunnel_plans)")}
    forbidden = {"node_id", "active_node_id", "standby_node_id"}
    if columns & forbidden:
        errors.append(f"per-tunnel node columns forbidden: {sorted(columns & forbidden)}")
    if db.execute("SELECT singleton_id,quarantine_days FROM routing_state").fetchone() != (1, 30):
        errors.append("routing_state seed must be singleton=1 and quarantine_days=30")
    indexes = {row[1] for row in db.execute("PRAGMA index_list(tunnel_plans)")}
    if "idx_tunnel_plans_managed_domain" not in indexes:
        errors.append("managed DNS domains must have a partial unique index")
    db.execute(
        "INSERT INTO tunnel_plans(name,local_ip,local_port,protocol,domain,dns_managed) "
        "VALUES('tcp-a','127.0.0.1',10001,'tcp','',0)"
    )
    db.execute(
        "INSERT INTO tunnel_plans(name,local_ip,local_port,protocol,domain,dns_managed) "
        "VALUES('tcp-b','127.0.0.1',10002,'tcp','',0)"
    )
    provider_row = db.execute(
        "SELECT singleton_id,chmlfrp_base_url,cloudflare_api_base FROM provider_settings"
    ).fetchone()
    if provider_row != (1, "https://cf-v2.uapis.cn", "https://api.cloudflare.com/client/v4"):
        errors.append("provider_settings singleton/default API bases are invalid")
finally:
    db.close()


required_files = [
    ".editorconfig",
    ".github/dependabot.yml",
    ".github/pull_request_template.md",
    ".github/ISSUE_TEMPLATE/bug_report.yml",
    "CONTRIBUTING.md",
    "SECURITY.md",
    "docs/REVIEW.md",
]
for relative in required_files:
    if not (ROOT / relative).is_file():
        errors.append(f"standard project file missing: {relative}")

workflows = sorted((ROOT / ".github" / "workflows").glob("*.yml"))
if [p.name for p in workflows] != ["ci.yml"]:
    errors.append("exactly one GitHub workflow is allowed: ci.yml")

workflow = (ROOT / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")
for required in [
    "actions/checkout@v7",
    "cargo fmt --all -- --check",
    "cargo test --workspace --locked",
    "cargo clippy --workspace --all-targets --locked -- -D warnings",
    "--target x86_64-unknown-linux-musl",
    "docker/setup-buildx-action@v4",
    "docker/login-action@v4",
    "docker/build-push-action@v7",
]:
    if required not in workflow:
        errors.append(f"CI reproducibility command missing: {required}")


if "cargo fmt --all\n" in workflow or "cargo generate-lockfile" in workflow:
    errors.append("CI must validate committed sources/lockfile; it must not mutate them")
if "git diff --exit-code" not in workflow:
    errors.append("CI must verify that quality/build steps leave the checkout unchanged")

cargo_path = ROOT / "Cargo.toml"
cargo = cargo_path.read_text(encoding="utf-8")
workspace_version = tomllib.loads(cargo)["workspace"]["package"]["version"]
if f"org.opencontainers.image.version={workspace_version}" not in workflow:
    errors.append("CI OCI version label must match workspace package version")
if "apps/web" in cargo or (ROOT / "Dioxus.toml").exists():
    errors.append("compiled Web/WASM frontend is forbidden; WebUI must be static Axum assets")
if not (ROOT / "web" / "index.html").is_file() or not (ROOT / "web" / "assets" / "app.js").is_file():
    errors.append("static WebUI assets are missing")

web_js = (ROOT / "web" / "assets" / "app.js").read_text(encoding="utf-8")
if web_js.count('api("/api/v1/failover", { method: "POST" })') != 1:
    errors.append("manual failover UI action must issue exactly one /api/v1/failover POST")
for required in [
    "/api/v1/settings/providers",
    "/api/v1/settings/providers/test/chmlfrp",
    "/api/v1/settings/providers/test/cloudflare",
    "/api/v1/settings/providers/cloudflare/zones",
    "https://panel.chmlfrp.net/",
]:
    if required not in web_js:
        errors.append(f"WebUI provider setup marker missing: {required}")

dockerfile = (ROOT / "Dockerfile").read_text(encoding="utf-8")
if "cargo build" in dockerfile or "dx build" in dockerfile or "FROM rust:" in dockerfile:
    errors.append("Dockerfile must package the verified runtime artifact; it must not rebuild Rust/WebUI")
if "COPY .release/ashan-frp-server" not in dockerfile or "COPY .release/web/" not in dockerfile:
    errors.append("Dockerfile must consume .release server and WebUI artifacts")

dockerignore = (ROOT / ".dockerignore").read_text(encoding="utf-8")
if any(line.strip().rstrip("/") == ".release" for line in dockerignore.splitlines()):
    errors.append(".dockerignore must not exclude .release; GHCR publish consumes it")

rust = "\n".join(p.read_text(encoding="utf-8") for p in ROOT.rglob("*.rs"))
if "GLOBAL_FAILOVER" not in rust:
    errors.append("GLOBAL_FAILOVER invariant marker missing")
if "active_node_id" in rust or "standby_node_id" in rust:
    errors.append("per-tunnel node assignment leaked into Rust source")
if "MigrationError::TargetNode" not in rust:
    errors.append("typed target-node failover error missing")
if "RoutingPhase::DegradedDns" not in rust or "RoutingPhase::Failed" not in rust:
    errors.append("typed routing recovery phases missing")
for required in [
    "ProviderSettingsView",
    "ProviderSettingsUpdate",
    "seed_provider_settings_from_env",
    "env_bootstrap_complete",
    "pub fn reconfigure",
    "/user/tokens/verify",
    "apply_provider_settings",
]:
    if required not in rust:
        errors.append(f"runtime provider-settings marker missing: {required}")
if "chmlfrp_token: String" in (ROOT / "crates" / "domain" / "src" / "lib.rs").read_text(encoding="utf-8"):
    errors.append("ProviderSettingsView must never expose the stored ChmlFrp token")
if "cloudflare_api_token: String" in (ROOT / "crates" / "domain" / "src" / "lib.rs").read_text(encoding="utf-8"):
    errors.append("ProviderSettingsView must never expose the stored Cloudflare token")

if "stop FRPC after confirmed active-node failure" not in rust:
    errors.append("automatic failover must stop the confirmed failed active FRPC runtime")

try:
    unraid_root = ET.parse(ROOT / "unraid" / "ashan-frp-control-center.xml").getroot()
    configs = {node.attrib.get("Name"): node for node in unraid_root.findall("Config")}
    if configs["ChmlFrp API Token"].attrib.get("Required") != "false":
        errors.append("Unraid ChmlFrp token must not be required")
    if configs["ChmlFrp API Token"].attrib.get("Display") != "advanced-hide":
        errors.append("Unraid ChmlFrp token must be an advanced bootstrap option")
    always = [
        node.attrib.get("Name") for node in unraid_root.findall("Config")
        if node.attrib.get("Display") == "always"
    ]
    if always != ["WebUI Port", "AppData (Cache)"]:
        errors.append(f"Unraid essential visible fields drifted: {always}")
except Exception as exc:
    errors.append(f"Unraid template parse/validation failed: {exc}")

if errors:
    print("VERIFY FAILED")
    for error in errors:
        print(" -", error)
    sys.exit(1)
print("VERIFY OK")
