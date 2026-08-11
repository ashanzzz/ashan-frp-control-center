const state = {
  page: "overview",
  dashboard: null,
  nodes: [],
  remoteTunnels: [],
  dnsRecords: [],
  logs: [],
  providerSettings: null,
  cloudflareZones: [],
  chmlDiagnostics: null,
  dnsDiagnostics: null,
  lastDeletedDns: null,
  providerFeedback: { chmlfrp: null, cloudflare: null },
  busy: false,
  refreshing: false,
};

const pageMeta = {
  overview: ["概览", "全局路由、Provider、FRPC 与受管资源健康状态"],
  tunnels: ["隧道", "Ashan 计划、ChmlFrp 远端事实与纳管关系"],
  chmlfrp: ["ChmlFrp", "直接查看和管理远端隧道与节点；受管资源受 GLOBAL ROUTING 保护"],
  dns: ["Cloudflare DNS", "直接管理当前 Zone 的 DNS；Ashan HA 记录受保护"],
  frpc: ["FRPC Runtime", "ChmlFrp 生成配置；FRPC 负责运行、日志与故障信号"],
  activity: ["活动", "全局同步、故障切换、Provider CRUD 与诊断审计"],
  settings: ["设置", "只配置 Provider 连接、Zone 与 GLOBAL ROUTING 策略"],
};

const $ = (selector) => document.querySelector(selector);
const esc = (value) => String(value ?? "")
  .replaceAll("&", "&amp;")
  .replaceAll("<", "&lt;")
  .replaceAll(">", "&gt;")
  .replaceAll('"', "&quot;")
  .replaceAll("'", "&#39;");

function statusBadge(status) {
  const classes = { ok: "ok", failed: "bad", drift: "warn", starting: "warn", waiting: "warn", disabled: "" };
  const cls = classes[status?.state] ?? "";
  const detail = status?.detail ? `<small class="detail">${esc(status.detail)}</small>` : "";
  return `<span class="badge ${cls}">${esc(status?.label ?? "未知")}</span>${detail}`;
}

function simpleBadge(label, kind = "") {
  return `<span class="badge ${kind}">${esc(label)}</span>`;
}

async function api(path, options = {}) {
  const response = await fetch(path, {
    headers: { "Content-Type": "application/json", ...(options.headers || {}) },
    ...options,
  });
  let payload = null;
  try { payload = await response.json(); } catch { /* no body */ }
  if (!response.ok) throw new Error(payload?.message || payload?.error?.message || `HTTP ${response.status}`);
  return payload?.data;
}

function setAlert(message = "", type = "error") {
  $("#alert").innerHTML = message ? `<div class="alert ${type}">${esc(message)}</div>` : "";
}

function metric(title, value, sub, kind = "") {
  return `<article class="metric ${kind}"><span>${esc(title)}</span><strong>${esc(value)}</strong><span>${esc(sub)}</span></article>`;
}

async function refresh() {
  if (state.refreshing) return;
  state.refreshing = true;
  try {
    const [dashboard, health] = await Promise.all([api("/api/v1/dashboard"), api("/api/v1/health")]);
    state.dashboard = dashboard;
    $("#version").textContent = `v${health.version}`;
    render();
  } catch (error) {
    setAlert(`控制中心 API：${error.message}`);
  } finally {
    state.refreshing = false;
  }
}

async function loadProviderSettings() {
  try { state.providerSettings = await api("/api/v1/settings/providers"); }
  catch (error) { state.providerSettings = null; setAlert(`读取 Provider 设置失败：${error.message}`); }
}

async function loadNodes() {
  if (!state.dashboard?.chmlfrp_health?.configured && !state.providerSettings?.chmlfrp_token_configured) {
    state.nodes = [];
    return;
  }
  try { state.nodes = await api("/api/v1/nodes"); }
  catch { state.nodes = []; }
}

async function loadRemoteTunnels() {
  if (!state.dashboard?.chmlfrp_health?.configured && !state.providerSettings?.chmlfrp_token_configured) {
    state.remoteTunnels = [];
    return;
  }
  try { state.remoteTunnels = await api("/api/v1/chmlfrp/tunnels"); }
  catch (error) { state.remoteTunnels = []; if (state.page === "chmlfrp" || state.page === "tunnels") setAlert(`读取 ChmlFrp 隧道失败：${error.message}`); }
}

async function loadDnsRecords() {
  if (!state.dashboard?.cloudflare_health?.configured) {
    state.dnsRecords = [];
    return;
  }
  try { state.dnsRecords = await api("/api/v1/dns/records"); }
  catch (error) { state.dnsRecords = []; if (state.page === "dns") setAlert(`读取 DNS 失败：${error.message}`); }
}

async function loadLogs() {
  try { state.logs = await api("/api/v1/frpc/logs"); }
  catch { state.logs = []; }
}

async function loadPageData(page) {
  if (page === "overview") await Promise.all([loadRemoteTunnels(), loadDnsRecords()]);
  if (page === "tunnels") await loadRemoteTunnels();
  if (page === "chmlfrp") await Promise.all([loadRemoteTunnels(), loadNodes()]);
  if (page === "dns") await loadDnsRecords();
  if (page === "frpc") await loadLogs();
  if (page === "settings") {
    await loadProviderSettings();
    await refresh();
    await loadNodes();
  }
}

function providerVisual(health, configured) {
  if (!configured) return { label: "未配置", kind: "", sub: "等待配置" };
  if (health?.connected) return { label: "Connected", kind: "good", sub: health.message || "API 正常" };
  return { label: "Disconnected", kind: "bad", sub: health?.message || "连接失败" };
}

function renderGlobal() {
  const d = state.dashboard;
  if (!d) return;
  const p = state.providerSettings;
  const chmlConfigured = p?.chmlfrp_token_configured ?? d.chmlfrp_health.configured;
  const cfConfigured = p?.cloudflare_api_token_configured ?? d.cloudflare_health.configured;
  const chml = providerVisual(d.chmlfrp_health, chmlConfigured);
  let cf = providerVisual(d.cloudflare_health, cfConfigured);
  if (cfConfigured && p && !p.cloudflare_zone_id) cf = { label: "需要 Zone", kind: "warn", sub: "Token 已保存，尚未选择 Zone" };
  const activeName = d.active_node?.name || d.routing.active_node || "未配置";
  const standbyName = d.standby_node?.name || d.routing.standby_node || "未配置";
  const haReady = Boolean(d.routing.active_node && d.routing.standby_node && d.routing.failover_enabled);
  $("#global-cards").innerHTML = [
    metric("ChmlFrp API", chml.label, chml.sub, chml.kind),
    metric("Cloudflare", cf.label, cf.sub, cf.kind),
    metric("ACTIVE", activeName, d.active_node?.real_ip || (d.routing.active_node ? "节点详情待刷新" : "等待初始化"), d.routing.active_node ? "good" : "warn"),
    metric("STANDBY", standbyName, d.standby_node?.real_ip || (d.routing.standby_node ? "节点详情待刷新" : "等待选择"), d.routing.standby_node ? "good" : "warn"),
    metric("FRPC", d.frpc.running ? "Running" : "Stopped", d.frpc.connected ? "Connected" : "Disconnected", d.frpc.running && d.frpc.connected ? "good" : "warn"),
    metric("GLOBAL HA", haReady ? "Ready" : "Not Ready", `自动切换 ${d.routing.failover_enabled ? "ON" : "OFF"} · ${d.routing.state}`, haReady ? "good" : "warn"),
  ].join("");
  $("#reconcile-btn").disabled = !d.routing.active_node;
  $("#failover-btn").disabled = !d.routing.active_node || !d.routing.standby_node;
}

function managedNames() {
  return new Set((state.dashboard?.tunnel_rows || []).map((row) => row.plan.name));
}

function isRemoteManaged(tunnel) {
  return managedNames().has(tunnel.tunnel_name);
}

function managedDnsPlan(record) {
  if (String(record.record_type).toUpperCase() !== "A") return null;
  return (state.dashboard?.tunnel_rows || []).map((row) => row.plan).find((plan) => plan.dns_managed && (
    plan.cloudflare_record_id === record.id || String(plan.domain).toLowerCase() === String(record.name).toLowerCase()
  )) || null;
}

function dnsEditorSupports(recordOrType) {
  const type = String(recordOrType?.record_type ?? recordOrType ?? "").toUpperCase();
  return ["A", "AAAA", "CNAME", "TXT", "MX"].includes(type);
}

function routingDistribution() {
  const counts = new Map();
  for (const tunnel of state.remoteTunnels) counts.set(tunnel.node || "未知", (counts.get(tunnel.node || "未知") || 0) + 1);
  return [...counts.entries()].sort((a, b) => b[1] - a[1]);
}

function renderBootstrapBanner() {
  if (!state.dashboard?.chmlfrp_health?.connected || state.dashboard.routing.active_node) return "";
  const distribution = routingDistribution();
  const detail = distribution.length
    ? distribution.map(([node, count]) => `${esc(node)} ${count} 条`).join(" · ")
    : "尚未发现远端隧道，也可以先选择 ACTIVE / STANDBY";
  return `<div class="onboarding-banner"><div><strong>GLOBAL ROUTING 尚未初始化</strong><p>ChmlFrp 已连接。${detail}</p></div><button id="start-bootstrap">开始初始化</button></div>`;
}

function renderOverview() {
  const d = state.dashboard;
  const healthy = d.tunnel_rows.filter((row) => row.overall.state === "ok").length;
  const attention = d.tunnel_rows.length - healthy;
  const unmanaged = state.remoteTunnels.filter((tunnel) => !isRemoteManaged(tunnel)).length;
  const rows = d.tunnel_rows.map((row) => `<tr>
    <td><strong>${esc(row.plan.name)}</strong><small>${esc(row.plan.protocol)}</small></td>
    <td>${statusBadge(row.chmlfrp)}</td><td>${statusBadge(row.frpc)}</td><td>${statusBadge(row.cloudflare)}</td><td>${statusBadge(row.overall)}</td>
  </tr>`).join("");
  return `${renderBootstrapBanner()}
    <div class="summary-strip">
      <div><span>受管隧道</span><strong>${d.tunnel_rows.length}</strong></div>
      <div><span>正常</span><strong>${healthy}</strong></div>
      <div><span>需关注</span><strong>${attention}</strong></div>
      <div><span>ChmlFrp 未纳管</span><strong>${unmanaged}</strong></div>
      <div><span>Cloudflare Records</span><strong>${state.dnsRecords.length}</strong></div>
    </div>
    <div class="panel-head"><div><h2>受管隧道健康</h2><p>这里只看状态；修改资源请进入“隧道 / ChmlFrp / DNS”。</p></div><button data-goto="tunnels">管理隧道</button></div>
    <div class="table-wrap"><table><thead><tr><th>隧道</th><th>ChmlFrp</th><th>FRPC</th><th>Cloudflare</th><th>整体</th></tr></thead><tbody>${rows || `<tr><td colspan="5" class="muted">尚未纳管任何隧道</td></tr>`}</tbody></table></div>`;
}

function renderTunnels() {
  const d = state.dashboard;
  const managedRows = d.tunnel_rows.map((row) => `<tr>
    <td><strong>${esc(row.plan.name)}</strong><small>${esc(row.plan.protocol)}</small></td>
    <td>${esc(row.plan.local_ip)}:${esc(row.plan.local_port)}</td>
    <td>${esc(row.plan.domain || "—")}</td>
    <td>${statusBadge(row.overall)}</td>
    <td><div class="inline-actions"><button data-plan-edit="${row.plan.id}">编辑计划</button><button class="danger ghost-danger" data-plan-unmanage="${row.plan.id}">解除纳管</button></div></td>
  </tr>`).join("");
  const unmanaged = state.remoteTunnels.filter((tunnel) => !isRemoteManaged(tunnel));
  const remoteRows = unmanaged.map((tunnel) => `<tr>
    <td><strong>${esc(tunnel.tunnel_name)}</strong><small>ID ${esc(tunnel.tunnel_id)}</small></td>
    <td>${esc(tunnel.node)}</td><td>${esc(tunnel.port_type)}</td><td>${esc(tunnel.local_ip)}:${esc(tunnel.local_port)}</td>
    <td>${esc(tunnel.remote_endpoint || tunnel.band_domain || "—")}</td>
    <td><button data-import-tunnel="${tunnel.tunnel_id}">导入为计划</button></td>
  </tr>`).join("");
  return `<div class="panel-head"><div><h2>受管隧道</h2><p>Plan 是 Ashan desired state。节点由 ACTIVE 统一管理。</p></div><button id="new-plan">+ 新建计划</button></div>
    <div class="table-wrap"><table><thead><tr><th>隧道</th><th>本地地址</th><th>域名</th><th>整体</th><th>操作</th></tr></thead><tbody>${managedRows || `<tr><td colspan="5" class="muted">没有受管隧道</td></tr>`}</tbody></table></div>
    <div class="section-separator"></div>
    <div class="panel-head"><div><h2>ChmlFrp 未纳管资源</h2><p>导入只创建本地 Plan，不会立即修改远端；批量导入默认不管理 DNS。</p></div><div class="actions"><button id="refresh-tunnels">刷新</button><button id="import-all">批量导入</button></div></div>
    <div class="table-wrap"><table><thead><tr><th>隧道</th><th>节点</th><th>协议</th><th>本地</th><th>外部</th><th>操作</th></tr></thead><tbody>${remoteRows || `<tr><td colspan="6" class="muted">没有未纳管 ChmlFrp 隧道</td></tr>`}</tbody></table></div>`;
}

function renderDiagnostics(data, type) {
  if (!data) return "";
  if (type === "chml") {
    const cfg = data.config_generate || {};
    return `<div class="diagnostic-grid">
      <div>${simpleBadge("PASS", "ok")} 认证 / 隧道读取 <strong>${esc(data.tunnel_read?.count ?? 0)}</strong></div>
      <div>${simpleBadge("PASS", "ok")} 节点读取 <strong>${esc(data.node_read?.count ?? 0)}</strong></div>
      <div>${cfg.tested ? simpleBadge(cfg.ok ? "PASS" : "FAIL", cfg.ok ? "ok" : "bad") : simpleBadge("SKIP")} 配置生成 ${cfg.tested ? esc(cfg.node || "") : "没有可测试隧道"}</div>
      <div>${data.delete_supported ? simpleBadge("可用", "ok") : simpleBadge("官方受限", "warn")} 删除 API</div>
    </div>`;
  }
  return `<div class="diagnostic-grid">
    <div>${simpleBadge("PASS", "ok")} Token ${esc(data.authentication)}</div>
    <div>${simpleBadge(data.read?.ok ? "PASS" : "FAIL", data.read?.ok ? "ok" : "bad")} Read</div>
    <div>${simpleBadge(data.create?.ok ? "PASS" : "FAIL", data.create?.ok ? "ok" : "bad")} Create</div>
    <div>${simpleBadge(data.update?.ok ? "PASS" : "FAIL", data.update?.ok ? "ok" : "bad")} Update</div>
    <div>${simpleBadge(data.delete?.ok ? "PASS" : "FAIL", data.delete?.ok ? "ok" : "bad")} Delete / Cleanup</div>
  </div>`;
}

function renderChmlfrp() {
  const d = state.dashboard;
  const p = state.providerSettings;
  const connected = d.chmlfrp_health.connected;
  const rows = state.remoteTunnels.map((tunnel) => {
    const managed = isRemoteManaged(tunnel);
    const endpoint = tunnel.band_domain || tunnel.remote_endpoint || "—";
    return `<tr><td><strong>${esc(tunnel.tunnel_name)}</strong><small>ID ${esc(tunnel.tunnel_id)}</small></td><td>${esc(tunnel.node)}</td><td>${esc(tunnel.port_type)}</td><td>${esc(tunnel.local_ip)}:${esc(tunnel.local_port)}</td><td>${esc(endpoint)}</td><td>${esc(tunnel.tunnel_state || "—")}</td><td>${managed ? simpleBadge("Managed", "ok") : simpleBadge("Unmanaged")}</td><td><div class="inline-actions">${managed ? `<button disabled title="受管资源请修改 Plan 后执行全局同步">由 HA 管理</button>` : `<button data-remote-edit="${tunnel.tunnel_id}">编辑</button><button data-remote-test="${tunnel.tunnel_id}">写入测试</button><button data-import-tunnel="${tunnel.tunnel_id}">导入</button>`}<button class="danger" disabled title="ChmlFrp v2 官方删除接口当前不可用">删除不可用</button></div></td></tr>`;
  }).join("");
  const nodeRows = state.nodes.map((node) => {
    let role = "—";
    if (d.routing.active_node === node.name) role = simpleBadge("ACTIVE", "ok");
    else if (d.routing.standby_node === node.name) role = simpleBadge("STANDBY", "warn");
    const quarantine = node.quarantined_until ? `${simpleBadge("隔离", "bad")} ${esc(node.quarantined_until)} <button data-unquarantine="${esc(node.name)}">解除</button>` : "—";
    return `<tr><td><strong>${esc(node.name)}</strong><small>${esc(node.area)}</small></td><td>${esc(node.real_ip || "—")}</td><td>${esc(node.state)}</td><td>${role}</td><td>${quarantine}</td></tr>`;
  }).join("");
  return `<div class="provider-console-head"><div><h2>ChmlFrp API</h2><p>${esc(d.chmlfrp_health.message)}</p></div>${simpleBadge(connected ? "Connected" : (p?.chmlfrp_token_configured ? "Disconnected" : "未配置"), connected ? "ok" : (p?.chmlfrp_token_configured ? "bad" : ""))}</div>
    <div class="actions resource-toolbar"><button id="chml-refresh">刷新资源</button><button id="chml-diag">API 诊断</button><button id="chml-create" ${connected ? "" : "disabled"}>+ 创建 ChmlFrp 隧道</button>${!connected ? `<button data-goto="settings">前往设置</button>` : ""}</div>
    ${renderDiagnostics(state.chmlDiagnostics, "chml")}
    <h2 class="section-title">远端隧道 <span class="count-pill">${state.remoteTunnels.length}</span></h2>
    <div class="table-wrap"><table><thead><tr><th>隧道</th><th>节点</th><th>协议</th><th>本地</th><th>外部</th><th>状态</th><th>纳管</th><th>操作</th></tr></thead><tbody>${rows || `<tr><td colspan="8" class="muted">${connected ? "未读取到隧道" : "请先配置 ChmlFrp"}</td></tr>`}</tbody></table></div>
    <div class="section-separator"></div><h2 class="section-title">节点 <span class="count-pill">${state.nodes.length}</span></h2>
    <div class="table-wrap"><table><thead><tr><th>节点</th><th>真实 IP</th><th>状态</th><th>角色</th><th>隔离</th></tr></thead><tbody>${nodeRows || `<tr><td colspan="5" class="muted">暂无节点数据</td></tr>`}</tbody></table></div>`;
}

function renderDns() {
  const d = state.dashboard;
  const p = state.providerSettings;
  const connected = d.cloudflare_health.connected;
  const rows = state.dnsRecords.map((record) => {
    const plan = managedDnsPlan(record);
    const owner = plan ? `${simpleBadge("Ashan HA", "ok")}<small>${esc(plan.name)}</small>` : simpleBadge("Manual");
    const proxy = ["A", "AAAA", "CNAME"].includes(String(record.record_type).toUpperCase()) ? (record.proxied ? "Proxied" : "DNS only") : "—";
    const editorSupported = dnsEditorSupports(record);
    const actions = plan
      ? `<button disabled title="先在计划隧道中解除 DNS 纳管">HA 保护</button>`
      : `${editorSupported ? `<button data-dns-edit="${esc(record.id)}">编辑</button>` : `<button disabled title="v0.4 编辑器暂不支持该记录类型">此类型只读</button>`}<button class="danger" data-dns-delete="${esc(record.id)}">删除</button>`;
    return `<tr><td>${simpleBadge(record.record_type)}</td><td><strong>${esc(record.name)}</strong><small>${esc(record.comment || "")}</small></td><td class="mono-cell">${esc(record.content)}</td><td>${esc(proxy)}</td><td>${record.ttl === 1 ? "Auto" : esc(record.ttl)}</td><td>${owner}</td><td><div class="inline-actions">${actions}</div></td></tr>`;
  }).join("");
  let statusLabel = "未配置";
  let kind = "";
  if (p?.cloudflare_api_token_configured && !p.cloudflare_zone_id) { statusLabel = "需要 Zone"; kind = "warn"; }
  else if (connected) { statusLabel = "Connected"; kind = "ok"; }
  else if (p?.cloudflare_api_token_configured) { statusLabel = "Disconnected"; kind = "bad"; }
  const restoreBanner = state.lastDeletedDns ? `<div class="restore-banner"><div><strong>刚刚删除：${esc(state.lastDeletedDns.record_type)} ${esc(state.lastDeletedDns.name)}</strong><p>已保留删除前快照，可一键重建；离开/刷新页面后不再显示此快捷入口。</p></div><button id="dns-restore">恢复</button><button id="dns-dismiss-restore">关闭</button></div>` : "";
  return `<div class="provider-console-head"><div><h2>Cloudflare DNS</h2><p>Zone: ${esc(p?.cloudflare_zone_id || "未选择")} · ${esc(d.cloudflare_health.message)}</p></div>${simpleBadge(statusLabel, kind)}</div>
    <div class="actions resource-toolbar"><button id="dns-refresh">刷新</button><button id="dns-diag" ${connected ? "" : "disabled"}>完整 CRUD 测试</button><button id="dns-create" ${connected ? "" : "disabled"}>+ 添加记录</button>${!connected ? `<button data-goto="settings">前往设置</button>` : ""}</div>
    ${restoreBanner}
    ${renderDiagnostics(state.dnsDiagnostics, "dns")}
    <div class="table-wrap"><table><thead><tr><th>类型</th><th>名称</th><th>内容</th><th>Proxy</th><th>TTL</th><th>管理者</th><th>操作</th></tr></thead><tbody>${rows || `<tr><td colspan="7" class="muted">${connected ? "当前 Zone 没有 DNS 记录" : "请先配置 Cloudflare Token 与 Zone"}</td></tr>`}</tbody></table></div>`;
}

function renderFrpc() {
  const d = state.dashboard;
  const logs = state.logs.map((event) => `${event.at} [${event.severity}] ${event.proxy_name ? `[${event.proxy_name}] ` : ""}${event.raw}`).join("\n");
  return `<div class="panel-head"><div><h2>运行状态</h2><p>配置路径：${esc(d.frpc.config_path)}</p></div><div class="actions"><button data-frpc="start">启动</button><button data-frpc="restart">重启</button><button data-frpc="stop" class="danger">停止</button></div></div>
    <dl class="kv"><dt>进程</dt><dd>${d.frpc.running ? "Running" : "Stopped"}</dd><dt>PID</dt><dd>${esc(d.frpc.pid ?? "—")}</dd><dt>服务端</dt><dd>${d.frpc.connected ? "Connected" : "Disconnected"}</dd><dt>配置 Revision</dt><dd>${esc(d.frpc.config_revision ?? "—")}</dd><dt>最近错误</dt><dd>${esc(d.frpc.last_error || "无")}</dd></dl><br><h2>实时日志</h2><pre id="log-view" class="log-view">${esc(logs || "暂无日志")}</pre>`;
}

function renderActivity() {
  const events = state.dashboard.recent_activity.map((event) => `<article class="activity"><header><span>${esc(event.kind)} · ${esc(event.level)}</span><time>${esc(event.created_at)}</time></header><strong>${esc(event.message)}</strong>${event.node_name ? `<div>Node: ${esc(event.node_name)}</div>` : ""}${event.tunnel_name ? `<div>Tunnel: ${esc(event.tunnel_name)}</div>` : ""}</article>`).join("");
  return `<div class="activity-list">${events || `<p class="muted">暂无活动记录</p>`}</div>`;
}

function providerFeedbackHtml(key, fallback) {
  const feedback = state.providerFeedback[key];
  if (!feedback) return `<div id="${key === "chmlfrp" ? "chml" : "cf"}-test-result" class="provider-result muted">${esc(fallback || "尚未测试")}</div>`;
  const cls = feedback.kind === "ok" ? "provider-ok" : feedback.kind === "bad" ? "provider-bad" : "provider-pending";
  return `<div id="${key === "chmlfrp" ? "chml" : "cf"}-test-result" class="provider-result ${cls}">${esc(feedback.message)}</div>`;
}

function renderSettings() {
  const r = state.dashboard.routing;
  const p = state.providerSettings;
  if (!p) return `<p class="muted">正在读取 Provider 设置…</p>`;
  const chmlStatus = state.dashboard.chmlfrp_health;
  const cfStatus = state.dashboard.cloudflare_health;
  const chmlBadge = !p.chmlfrp_token_configured ? simpleBadge("未配置") : chmlStatus.connected ? simpleBadge("Connected", "ok") : simpleBadge("Disconnected", "bad");
  let cfBadge = simpleBadge("未配置");
  if (p.cloudflare_api_token_configured && !p.cloudflare_zone_id) cfBadge = simpleBadge("需要 Zone", "warn");
  else if (p.cloudflare_api_token_configured) cfBadge = cfStatus.connected ? simpleBadge("Connected", "ok") : simpleBadge("Disconnected", "bad");
  const zoneOptions = cloudflareZoneOptions(p.cloudflare_zone_id);
  const nodeOptions = (selected, allowEmpty = true) => `${allowEmpty ? `<option value="">未配置</option>` : ""}${state.nodes.map((node) => `<option value="${esc(node.name)}" ${node.name === selected ? "selected" : ""}>${esc(node.name)} · ${esc(node.area)} · ${esc(node.state)}</option>`).join("")}`;
  const routingBody = r.active_node ? `<form id="routing-form"><div class="form-grid"><label>ACTIVE<input value="${esc(r.active_node)}" disabled></label><label>STANDBY<select id="routing-standby">${nodeOptions(r.standby_node)}</select></label></div><label>故障节点隔离天数<input id="routing-days" type="number" min="1" max="3650" value="${esc(r.quarantine_days)}"></label><label class="check compact-check"><input id="routing-enabled" type="checkbox" ${r.failover_enabled ? "checked" : ""}> 启用 FRPC 日志驱动自动全局切换</label><div class="actions"><button type="submit">保存路由策略</button></div><p class="provider-note">ACTIVE 已锁定。更换 ACTIVE 必须使用 GLOBAL FAILOVER。</p></form>` : `<div class="empty-state"><strong>尚未初始化 ACTIVE / STANDBY</strong><p>先连接 ChmlFrp 并读取节点，再通过初始化向导确认。向导只保存路由策略，不会立即迁移隧道。</p><button id="routing-bootstrap" ${chmlStatus.connected ? "" : "disabled"}>初始化 GLOBAL ROUTING</button></div>`;
  return `<div class="settings-stack">
    <section class="settings-card"><div class="settings-card-head"><div><h2>ChmlFrp 连接</h2><p>Token 保存到 /data SQLite；保存后立即热更新并就地验证。</p></div>${chmlBadge}</div>
      <label>API Base URL<input id="chml-base" value="${esc(p.chmlfrp_base_url)}" autocomplete="off" spellcheck="false"></label>
      <label>API Token<input id="chml-token" type="password" autocomplete="new-password" placeholder="${p.chmlfrp_token_configured ? "已保存；留空保持当前 Token" : "粘贴 ChmlFrp API Token"}"></label>
      ${providerFeedbackHtml("chmlfrp", chmlStatus.message)}
      <div class="provider-actions"><button type="button" id="chml-test">测试连接</button><button type="button" id="chml-save">保存并立即验证</button>${p.chmlfrp_token_configured ? `<button type="button" id="chml-clear" class="danger">清除 Token</button>` : ""}<a class="button-link" href="https://panel.chmlfrp.net/" target="_blank" rel="noopener noreferrer">打开 ChmlFrp 官方面板获取 API Token</a></div>
      <p class="provider-note">不要粘贴包含 access_token / refresh_token 的完整浏览器授权 URL；这里需要 ChmlFrp API Token。</p>
    </section>
    <section class="settings-card"><div class="settings-card-head"><div><h2>Cloudflare</h2><p>保存 Token/Zone 后立即验证；DNS 资源操作在“DNS”页面完成。</p></div>${cfBadge}</div>
      <label>API Base URL<input id="cf-base" value="${esc(p.cloudflare_api_base)}" autocomplete="off" spellcheck="false"></label>
      <label>API Token<input id="cf-token" type="password" autocomplete="new-password" placeholder="${p.cloudflare_api_token_configured ? "已保存；留空保持当前 Token" : "粘贴 Cloudflare scoped API Token"}"></label>
      <div class="form-grid provider-zone-grid"><label>Zone<select id="cf-zone-list">${zoneOptions}</select></label><label>Zone ID<input id="cf-zone" value="${esc(p.cloudflare_zone_id)}" placeholder="读取 Zone 自动填入，也可手动输入" autocomplete="off"></label></div>
      ${providerFeedbackHtml("cloudflare", cfStatus.message)}
      <div class="provider-actions"><button type="button" id="cf-zones">验证 Token / 读取 Zone</button><button type="button" id="cf-test">测试 Token + Zone</button><button type="button" id="cf-save">保存并立即验证</button>${p.cloudflare_api_token_configured ? `<button type="button" id="cf-clear" class="danger">清除 Token</button>` : ""}</div>
      <p class="provider-note">完整写权限可在 DNS 页面通过临时 TXT 记录执行 Create → Read → Update → Delete → Cleanup 诊断。</p>
    </section>
    <section class="settings-card"><div class="settings-card-head"><div><h2>GLOBAL ROUTING</h2><p>所有受管隧道共享一个 ACTIVE。STANDBY 是全局切换候选，不属于某一条隧道。</p></div>${simpleBadge(r.active_node ? "Initialized" : "未初始化", r.active_node ? "ok" : "warn")}</div>${routingBody}</section>
  </div>`;
}

function cloudflareZoneOptions(currentZoneId = "") {
  const options = [`<option value="">${state.cloudflareZones.length ? "请选择 Zone" : "读取 Zone 后可选择"}</option>`];
  const currentKnown = state.cloudflareZones.some((zone) => zone.id === currentZoneId);
  if (currentZoneId && !currentKnown) options.push(`<option value="${esc(currentZoneId)}" selected>当前 / 手动 Zone · ${esc(currentZoneId)}</option>`);
  for (const zone of state.cloudflareZones) options.push(`<option value="${esc(zone.id)}" ${zone.id === currentZoneId ? "selected" : ""}>${esc(zone.name)} · ${esc(zone.status || "unknown")}</option>`);
  return options.join("");
}

function render() {
  if (!state.dashboard) return;
  renderGlobal();
  const [title, subtitle] = pageMeta[state.page];
  $("#page-title").textContent = title;
  $("#page-subtitle").textContent = subtitle;
  document.querySelectorAll("#nav button").forEach((button) => button.classList.toggle("active", button.dataset.page === state.page));
  const renderers = { overview: renderOverview, tunnels: renderTunnels, chmlfrp: renderChmlfrp, dns: renderDns, frpc: renderFrpc, activity: renderActivity, settings: renderSettings };
  $("#content").classList.remove("loading");
  $("#content").innerHTML = renderers[state.page]();
  bindPageActions();
}

function openPlan(plan = null) {
  $("#tunnel-dialog-title").textContent = plan ? "编辑计划隧道" : "新增计划隧道";
  $("#tunnel-id").value = plan?.id || "";
  $("#tunnel-name").value = plan?.name || "";
  $("#tunnel-ip").value = plan?.local_ip || "192.168.8.11";
  $("#tunnel-port").value = plan?.local_port || "";
  $("#tunnel-protocol").value = plan?.protocol || "http";
  $("#tunnel-domain").value = plan?.domain || "";
  $("#tunnel-dns").checked = plan?.dns_managed ?? false;
  $("#plan-node-label").textContent = state.dashboard.routing.active_node || "尚未初始化 ACTIVE";
  $("#dialog-error").textContent = "";
  $("#tunnel-dialog").showModal();
}

function remoteNodeOptions(selected = "") {
  return state.nodes.map((node) => `<option value="${esc(node.name)}" ${node.name === selected ? "selected" : ""}>${esc(node.name)} · ${esc(node.area)} · ${esc(node.state)}</option>`).join("");
}

function updateRemoteProtocolFields() {
  const protocol = $("#remote-protocol").value.toLowerCase();
  const web = protocol === "http" || protocol === "https";
  $("#remote-port-field").classList.toggle("hidden", web);
  $("#remote-domain-field").classList.toggle("hidden", !web);
  $("#remote-port").required = !web;
  $("#remote-domain").required = web;
}

function openRemoteTunnel(tunnel = null) {
  $("#remote-tunnel-title").textContent = tunnel ? "编辑未纳管 ChmlFrp 隧道" : "创建 ChmlFrp 隧道";
  $("#remote-tunnel-id").value = tunnel?.tunnel_id || "";
  $("#remote-name").value = tunnel?.tunnel_name || "";
  $("#remote-name").disabled = Boolean(tunnel);
  $("#remote-node").innerHTML = remoteNodeOptions(tunnel?.node || "");
  $("#remote-protocol").value = (tunnel?.port_type || "http").toLowerCase();
  $("#remote-local-ip").value = tunnel?.local_ip || "192.168.8.11";
  $("#remote-local-port").value = tunnel?.local_port || "";
  const numericEndpoint = /^\d+$/.test(String(tunnel?.remote_endpoint || "")) ? tunnel.remote_endpoint : "";
  $("#remote-port").value = numericEndpoint;
  $("#remote-domain").value = tunnel?.band_domain || (["http", "https"].includes(String(tunnel?.port_type).toLowerCase()) ? tunnel?.remote_endpoint || "" : "");
  $("#remote-encryption").checked = String(tunnel?.encryption || "false") === "true";
  $("#remote-compression").checked = tunnel ? String(tunnel.compression) !== "false" : true;
  $("#remote-extra").value = tunnel?.extra_params || "";
  $("#remote-dialog-error").textContent = "";
  updateRemoteProtocolFields();
  $("#remote-tunnel-dialog").showModal();
}

function updateDnsTypeFields() {
  const type = $("#dns-type").value.toUpperCase();
  $("#dns-priority-field").classList.toggle("hidden", type !== "MX");
  $("#dns-proxy-field").classList.toggle("hidden", !["A", "AAAA", "CNAME"].includes(type));
}

function openDnsRecord(record = null) {
  $("#dns-dialog-title").textContent = record ? "编辑 DNS Record" : "添加 DNS Record";
  $("#dns-id").value = record?.id || "";
  $("#dns-type").value = record?.record_type || "A";
  $("#dns-name").value = record?.name || "";
  $("#dns-content").value = record?.content || "";
  $("#dns-ttl").value = String(record?.ttl || 1);
  if (!["1", "60", "300", "600", "3600", "86400"].includes($("#dns-ttl").value)) $("#dns-ttl").innerHTML += `<option value="${esc(record.ttl)}" selected>${esc(record.ttl)} 秒</option>`;
  $("#dns-proxied").checked = Boolean(record?.proxied);
  $("#dns-priority").value = record?.priority ?? 10;
  $("#dns-comment").value = record?.comment || "";
  $("#dns-dialog-error").textContent = "";
  updateDnsTypeFields();
  $("#dns-dialog").showModal();
}

function openBootstrap() {
  const distribution = routingDistribution();
  const recommended = distribution[0]?.[0] || state.nodes.find((node) => String(node.state).toLowerCase() === "online")?.name || "";
  $("#bootstrap-distribution").innerHTML = distribution.length ? `<strong>当前远端分布</strong>${distribution.map(([node, count]) => `<div><span>${esc(node)}</span><b>${count} 条</b></div>`).join("")}` : `<p class="muted">当前没有 ChmlFrp 隧道；请选择准备使用的 ACTIVE / STANDBY。</p>`;
  $("#bootstrap-active").innerHTML = remoteNodeOptions(recommended);
  $("#bootstrap-standby").innerHTML = `<option value="">未配置</option>${remoteNodeOptions("")}`;
  if (recommended) $("#bootstrap-active").value = recommended;
  $("#bootstrap-days").value = state.dashboard.routing.quarantine_days || 30;
  $("#bootstrap-enabled").checked = state.dashboard.routing.failover_enabled;
  $("#bootstrap-error").textContent = "";
  $("#bootstrap-dialog").showModal();
}

function confirmAction(title, message) {
  return new Promise((resolve) => {
    const dialog = $("#confirm-dialog");
    $("#confirm-title").textContent = title;
    $("#confirm-message").textContent = message;
    dialog.returnValue = "cancel";
    dialog.addEventListener("close", () => resolve(dialog.returnValue === "default"), { once: true });
    dialog.showModal();
  });
}

async function gotoPage(page) {
  state.page = page;
  await loadPageData(page);
  render();
}

async function importRemote(id) {
  try {
    await api("/api/v1/tunnels/import", { method: "POST", body: JSON.stringify({ tunnel_id: Number(id), dns_managed: false }) });
    setAlert("已导入为 Ashan 计划；远端尚未修改，DNS 默认未纳管。", "info");
    await Promise.all([refresh(), loadRemoteTunnels()]);
    render();
  } catch (error) { setAlert(error.message); }
}

function bindPageActions() {
  document.querySelectorAll("[data-goto]").forEach((button) => button.addEventListener("click", () => gotoPage(button.dataset.goto)));
  $("#start-bootstrap")?.addEventListener("click", openBootstrap);
  $("#routing-bootstrap")?.addEventListener("click", openBootstrap);
  $("#new-plan")?.addEventListener("click", () => openPlan());
  document.querySelectorAll("[data-plan-edit]").forEach((button) => button.addEventListener("click", () => {
    const id = Number(button.dataset.planEdit);
    const plan = state.dashboard.tunnel_rows.find((row) => row.plan.id === id)?.plan;
    if (plan) openPlan(plan);
  }));
  document.querySelectorAll("[data-plan-unmanage]").forEach((button) => button.addEventListener("click", async () => {
    const id = Number(button.dataset.planUnmanage);
    const plan = state.dashboard.tunnel_rows.find((row) => row.plan.id === id)?.plan;
    if (!plan || !(await confirmAction("解除 Ashan 纳管", `只删除本地计划“${plan.name}”；ChmlFrp 远端隧道和 Cloudflare DNS 都保持不变。继续？`))) return;
    try { await api(`/api/v1/tunnels/${id}/unmanage`, { method: "POST" }); await Promise.all([refresh(), loadRemoteTunnels()]); render(); } catch (error) { setAlert(error.message); }
  }));
  document.querySelectorAll("[data-import-tunnel]").forEach((button) => button.addEventListener("click", () => importRemote(button.dataset.importTunnel)));
  $("#import-all")?.addEventListener("click", async () => {
    if (!(await confirmAction("批量导入", "将所有未纳管 ChmlFrp 隧道创建为本地 Plan；不会修改远端，默认不自动管理 DNS。继续？"))) return;
    try { const result = await api("/api/v1/tunnels/import-all", { method: "POST" }); setAlert(`批量导入完成：${result.imported.length} 条，跳过 ${result.skipped.length} 条。`, "info"); await Promise.all([refresh(), loadRemoteTunnels()]); render(); } catch (error) { setAlert(error.message); }
  });
  $("#refresh-tunnels")?.addEventListener("click", async () => { await loadRemoteTunnels(); render(); });
  $("#chml-refresh")?.addEventListener("click", async () => { await Promise.all([loadRemoteTunnels(), loadNodes(), refresh()]); render(); });
  $("#chml-create")?.addEventListener("click", () => openRemoteTunnel());
  $("#chml-diag")?.addEventListener("click", async () => {
    try { state.chmlDiagnostics = await api("/api/v1/chmlfrp/diagnostics", { method: "POST" }); render(); } catch (error) { setAlert(error.message); }
  });
  document.querySelectorAll("[data-remote-edit]").forEach((button) => button.addEventListener("click", () => {
    const tunnel = state.remoteTunnels.find((item) => item.tunnel_id === Number(button.dataset.remoteEdit));
    if (tunnel) openRemoteTunnel(tunnel);
  }));
  document.querySelectorAll("[data-remote-test]").forEach((button) => button.addEventListener("click", async () => {
    const id = Number(button.dataset.remoteTest);
    const tunnel = state.remoteTunnels.find((item) => item.tunnel_id === id);
    if (!tunnel || !(await confirmAction("ChmlFrp 安全写入测试", `将对未纳管隧道“${tunnel.tunnel_name}”把当前配置原值重新提交一次，然后重新读取比对。理论上不改变配置，但属于真实写 API 调用。继续？`))) return;
    try {
      const result = await api(`/api/v1/chmlfrp/tunnels/${id}/test-write`, { method: "POST" });
      setAlert(`ChmlFrp 写入测试 PASS：${result.message}`, "info");
      await loadRemoteTunnels();
      render();
    } catch (error) { setAlert(`ChmlFrp 写入测试失败：${error.message}`); }
  }));
  document.querySelectorAll("[data-unquarantine]").forEach((button) => button.addEventListener("click", async () => {
    const name = button.dataset.unquarantine;
    if (!(await confirmAction("解除节点隔离", `节点“${name}”将重新进入候选池，但不会自动回切。继续？`))) return;
    try { await api(`/api/v1/nodes/${encodeURIComponent(name)}/unquarantine`, { method: "POST" }); await Promise.all([loadNodes(), refresh()]); render(); } catch (error) { setAlert(error.message); }
  }));
  $("#dns-refresh")?.addEventListener("click", async () => { await loadDnsRecords(); render(); });
  $("#dns-create")?.addEventListener("click", () => openDnsRecord());
  $("#dns-diag")?.addEventListener("click", async () => {
    if (!(await confirmAction("Cloudflare 完整 CRUD 测试", "将临时创建一个 _ashan-api-test-*.Zone TXT 记录，完成读取、更新、删除并自动清理。继续？"))) return;
    try { state.dnsDiagnostics = await api("/api/v1/dns/diagnostics", { method: "POST" }); await loadDnsRecords(); render(); } catch (error) { setAlert(error.message); }
  });
  document.querySelectorAll("[data-dns-edit]").forEach((button) => button.addEventListener("click", () => {
    const record = state.dnsRecords.find((item) => item.id === button.dataset.dnsEdit);
    if (record) openDnsRecord(record);
  }));
  document.querySelectorAll("[data-dns-delete]").forEach((button) => button.addEventListener("click", async () => {
    const record = state.dnsRecords.find((item) => item.id === button.dataset.dnsDelete);
    if (!record || !(await confirmAction("删除 DNS Record", `${record.record_type} ${record.name} → ${record.content}\n删除前快照会写入活动记录。继续？`))) return;
    try {
      const result = await api(`/api/v1/dns/records/${encodeURIComponent(record.id)}`, { method: "DELETE" });
      state.lastDeletedDns = dnsEditorSupports(result.snapshot) ? result.snapshot : null;
      setAlert(`已删除 ${record.name}；删除前快照已记录。${state.lastDeletedDns ? " 可在 DNS 页立即恢复。" : ""}`, "info");
      await loadDnsRecords();
      render();
    } catch (error) { setAlert(error.message); }
  }));
  $("#dns-restore")?.addEventListener("click", async () => {
    const record = state.lastDeletedDns;
    if (!record) return;
    const body = { type: record.record_type, name: record.name, content: record.content, ttl: record.ttl || 1, proxied: ["A", "AAAA", "CNAME"].includes(String(record.record_type).toUpperCase()) ? Boolean(record.proxied) : null, priority: record.priority ?? null, comment: record.comment || null };
    try {
      await api("/api/v1/dns/records", { method: "POST", body: JSON.stringify(body) });
      state.lastDeletedDns = null;
      setAlert(`DNS 记录 ${record.name} 已根据删除前快照恢复。`, "info");
      await loadDnsRecords();
      render();
    } catch (error) { setAlert(`恢复失败：${error.message}`); }
  });
  $("#dns-dismiss-restore")?.addEventListener("click", () => { state.lastDeletedDns = null; render(); });
  document.querySelectorAll("[data-frpc]").forEach((button) => button.addEventListener("click", async () => {
    try { await api(`/api/v1/frpc/${button.dataset.frpc}`, { method: "POST" }); await Promise.all([refresh(), loadLogs()]); render(); } catch (error) { setAlert(error.message); }
  }));
  $("#routing-form")?.addEventListener("submit", saveRouting);
  $("#chml-test")?.addEventListener("click", () => testChmlFrp());
  $("#chml-save")?.addEventListener("click", saveChmlFrpSettings);
  $("#chml-clear")?.addEventListener("click", clearChmlFrpToken);
  $("#cf-zones")?.addEventListener("click", loadCloudflareZones);
  $("#cf-test")?.addEventListener("click", () => testCloudflare());
  $("#cf-save")?.addEventListener("click", saveCloudflareSettings);
  $("#cf-clear")?.addEventListener("click", clearCloudflareToken);
  $("#cf-zone-list")?.addEventListener("change", (event) => { if (event.target.value) $("#cf-zone").value = event.target.value; });
}

function setProviderFeedback(key, message, kind = "pending") {
  state.providerFeedback[key] = { message, kind };
  const selector = key === "chmlfrp" ? "#chml-test-result" : "#cf-test-result";
  const element = $(selector);
  if (element) {
    element.textContent = message;
    element.classList.remove("muted", "provider-ok", "provider-bad", "provider-pending");
    element.classList.add(kind === "ok" ? "provider-ok" : kind === "bad" ? "provider-bad" : "provider-pending");
  }
}

function chmlProbeBody(useInputs = true) {
  return {
    base_url: useInputs && $("#chml-base") ? $("#chml-base").value.trim() : state.providerSettings.chmlfrp_base_url,
    token: useInputs && $("#chml-token") ? ($("#chml-token").value.trim() || null) : null,
  };
}

function cloudflareProbeBody(useInputs = true) {
  return {
    base_url: useInputs && $("#cf-base") ? $("#cf-base").value.trim() : state.providerSettings.cloudflare_api_base,
    token: useInputs && $("#cf-token") ? ($("#cf-token").value.trim() || null) : null,
    zone_id: useInputs && $("#cf-zone") ? ($("#cf-zone").value.trim() || null) : (state.providerSettings.cloudflare_zone_id || null),
  };
}

async function testChmlFrp(useInputs = true) {
  try {
    setProviderFeedback("chmlfrp", "正在验证 ChmlFrp Token 并读取隧道…");
    const result = await api("/api/v1/settings/providers/test/chmlfrp", { method: "POST", body: JSON.stringify(chmlProbeBody(useInputs)) });
    setProviderFeedback("chmlfrp", `${result.message} · 已读取 ${result.tunnels} 条隧道`, "ok");
    await refresh();
    return true;
  } catch (error) {
    setProviderFeedback("chmlfrp", `验证失败：${error.message}`, "bad");
    await refresh();
    return false;
  }
}

async function saveChmlFrpSettings() {
  const p = state.providerSettings;
  const body = { chmlfrp_base_url: $("#chml-base").value.trim(), chmlfrp_token: $("#chml-token").value.trim() || null, clear_chmlfrp_token: false, cloudflare_api_base: p.cloudflare_api_base, cloudflare_api_token: null, clear_cloudflare_api_token: false, cloudflare_zone_id: p.cloudflare_zone_id };
  try {
    setProviderFeedback("chmlfrp", "正在保存配置…");
    state.providerSettings = await api("/api/v1/settings/providers", { method: "PUT", body: JSON.stringify(body) });
    setProviderFeedback("chmlfrp", "配置已保存，正在立即验证…");
    render();
    await testChmlFrp(false);
    await Promise.all([loadProviderSettings(), loadNodes(), loadRemoteTunnels()]);
    render();
  } catch (error) { setProviderFeedback("chmlfrp", `保存失败：${error.message}`, "bad"); }
}

async function clearChmlFrpToken() {
  if (!(await confirmAction("清除 ChmlFrp Token", "清除后 ChmlFrp 资源管理、同步和故障切换不可用，直到重新配置。继续？"))) return;
  const p = state.providerSettings;
  const body = { chmlfrp_base_url: $("#chml-base").value.trim(), chmlfrp_token: null, clear_chmlfrp_token: true, cloudflare_api_base: p.cloudflare_api_base, cloudflare_api_token: null, clear_cloudflare_api_token: false, cloudflare_zone_id: p.cloudflare_zone_id };
  try { state.providerSettings = await api("/api/v1/settings/providers", { method: "PUT", body: JSON.stringify(body) }); state.providerFeedback.chmlfrp = { message: "ChmlFrp Token 已清除", kind: "ok" }; await refresh(); state.nodes = []; state.remoteTunnels = []; render(); } catch (error) { setProviderFeedback("chmlfrp", error.message, "bad"); }
}

async function loadCloudflareZones() {
  try {
    setProviderFeedback("cloudflare", "正在验证 Token 并读取可访问 Zone…");
    const zones = await api("/api/v1/settings/providers/cloudflare/zones", { method: "POST", body: JSON.stringify(cloudflareProbeBody(true)) });
    state.cloudflareZones = zones;
    const current = $("#cf-zone").value.trim() || state.providerSettings.cloudflare_zone_id;
    $("#cf-zone-list").innerHTML = cloudflareZoneOptions(current);
    if (!current && zones.length === 1) { $("#cf-zone-list").value = zones[0].id; $("#cf-zone").value = zones[0].id; }
    setProviderFeedback("cloudflare", `Token 有效，读取到 ${zones.length} 个可访问 Zone`, "ok");
  } catch (error) { setProviderFeedback("cloudflare", `读取 Zone 失败：${error.message}`, "bad"); }
}

async function testCloudflare(useInputs = true) {
  try {
    setProviderFeedback("cloudflare", "正在验证 Cloudflare Token / Zone…");
    const result = await api("/api/v1/settings/providers/test/cloudflare", { method: "POST", body: JSON.stringify(cloudflareProbeBody(useInputs)) });
    const detail = result.dns_read_tested ? ` · 已读取 ${result.a_records} 条 A 记录` : ` · 可访问 ${result.zones ?? 0} 个 Zone`;
    setProviderFeedback("cloudflare", `${result.message}${detail}`, "ok");
    await refresh();
    return true;
  } catch (error) {
    setProviderFeedback("cloudflare", `验证失败：${error.message}`, "bad");
    await refresh();
    return false;
  }
}

async function saveCloudflareSettings() {
  const p = state.providerSettings;
  const body = { chmlfrp_base_url: p.chmlfrp_base_url, chmlfrp_token: null, clear_chmlfrp_token: false, cloudflare_api_base: $("#cf-base").value.trim(), cloudflare_api_token: $("#cf-token").value.trim() || null, clear_cloudflare_api_token: false, cloudflare_zone_id: $("#cf-zone").value.trim() };
  try {
    setProviderFeedback("cloudflare", "正在保存配置…");
    state.providerSettings = await api("/api/v1/settings/providers", { method: "PUT", body: JSON.stringify(body) });
    setProviderFeedback("cloudflare", "配置已保存，正在立即验证…");
    render();
    await testCloudflare(false);
    await loadProviderSettings();
    if (state.providerSettings.cloudflare_zone_id) await loadDnsRecords();
    render();
  } catch (error) { setProviderFeedback("cloudflare", `保存失败：${error.message}`, "bad"); }
}

async function clearCloudflareToken() {
  if (!(await confirmAction("清除 Cloudflare Token", "清除后 DNS 读取、CRUD 和 HA DNS 切换不可用。继续？"))) return;
  const p = state.providerSettings;
  const body = { chmlfrp_base_url: p.chmlfrp_base_url, chmlfrp_token: null, clear_chmlfrp_token: false, cloudflare_api_base: $("#cf-base").value.trim(), cloudflare_api_token: null, clear_cloudflare_api_token: true, cloudflare_zone_id: $("#cf-zone").value.trim() };
  try { state.providerSettings = await api("/api/v1/settings/providers", { method: "PUT", body: JSON.stringify(body) }); state.providerFeedback.cloudflare = { message: "Cloudflare Token 已清除", kind: "ok" }; state.dnsRecords = []; await refresh(); render(); } catch (error) { setProviderFeedback("cloudflare", error.message, "bad"); }
}

async function saveRouting(event) {
  event.preventDefault();
  const current = state.dashboard.routing;
  const body = { active_node: current.active_node, standby_node: $("#routing-standby").value || null, quarantine_days: Number($("#routing-days").value), failover_enabled: $("#routing-enabled").checked };
  try { await api("/api/v1/routing", { method: "PUT", body: JSON.stringify(body) }); setAlert("GLOBAL ROUTING 策略已保存。", "info"); await Promise.all([refresh(), loadNodes()]); render(); } catch (error) { setAlert(error.message); }
}

$("#tunnel-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const id = $("#tunnel-id").value;
  const body = { name: $("#tunnel-name").value.trim(), local_ip: $("#tunnel-ip").value.trim(), local_port: Number($("#tunnel-port").value), protocol: $("#tunnel-protocol").value, domain: $("#tunnel-domain").value.trim(), dns_managed: $("#tunnel-dns").checked };
  try { await api(id ? `/api/v1/tunnels/${id}` : "/api/v1/tunnels", { method: id ? "PUT" : "POST", body: JSON.stringify(body) }); $("#tunnel-dialog").close(); setAlert("计划已保存；如需收敛远端，请执行全局同步。", "info"); await refresh(); render(); } catch (error) { $("#dialog-error").textContent = error.message; }
});
$("#dialog-cancel").addEventListener("click", () => $("#tunnel-dialog").close());

$("#remote-protocol").addEventListener("change", updateRemoteProtocolFields);
$("#remote-tunnel-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const id = $("#remote-tunnel-id").value;
  const protocol = $("#remote-protocol").value.toLowerCase();
  const body = { tunnel_name: $("#remote-name").value.trim(), node: $("#remote-node").value, port_type: protocol, local_ip: $("#remote-local-ip").value.trim(), local_port: Number($("#remote-local-port").value), remote_port: ["tcp", "udp"].includes(protocol) ? Number($("#remote-port").value) : 0, domain: ["http", "https"].includes(protocol) ? $("#remote-domain").value.trim() : "", encryption: $("#remote-encryption").checked, compression: $("#remote-compression").checked, extra_params: $("#remote-extra").value.trim() };
  try { await api(id ? `/api/v1/chmlfrp/tunnels/${id}` : "/api/v1/chmlfrp/tunnels", { method: id ? "PUT" : "POST", body: JSON.stringify(body) }); $("#remote-tunnel-dialog").close(); setAlert(id ? "ChmlFrp 未纳管隧道已更新。" : "ChmlFrp 隧道已创建；如需 HA 管理，请点击导入。", "info"); await Promise.all([loadRemoteTunnels(), refresh()]); render(); } catch (error) { $("#remote-dialog-error").textContent = error.message; }
});
$("#remote-dialog-cancel").addEventListener("click", () => $("#remote-tunnel-dialog").close());

$("#dns-type").addEventListener("change", updateDnsTypeFields);
$("#dns-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const id = $("#dns-id").value;
  const type = $("#dns-type").value.toUpperCase();
  const body = { type, name: $("#dns-name").value.trim(), content: $("#dns-content").value.trim(), ttl: Number($("#dns-ttl").value), proxied: ["A", "AAAA", "CNAME"].includes(type) ? $("#dns-proxied").checked : null, priority: type === "MX" ? Number($("#dns-priority").value) : null, comment: $("#dns-comment").value.trim() || null };
  try { await api(id ? `/api/v1/dns/records/${encodeURIComponent(id)}` : "/api/v1/dns/records", { method: id ? "PUT" : "POST", body: JSON.stringify(body) }); $("#dns-dialog").close(); setAlert(id ? "DNS 记录已更新。" : "DNS 记录已创建。", "info"); await loadDnsRecords(); render(); } catch (error) { $("#dns-dialog-error").textContent = error.message; }
});
$("#dns-dialog-cancel").addEventListener("click", () => $("#dns-dialog").close());

$("#bootstrap-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const body = { active_node: $("#bootstrap-active").value, standby_node: $("#bootstrap-standby").value || null, quarantine_days: Number($("#bootstrap-days").value), failover_enabled: $("#bootstrap-enabled").checked };
  try { await api("/api/v1/routing/bootstrap", { method: "POST", body: JSON.stringify(body) }); $("#bootstrap-dialog").close(); setAlert("GLOBAL ROUTING 已初始化。当前没有自动迁移任何隧道；确认计划后请执行“全局同步”。", "info"); await Promise.all([refresh(), loadNodes()]); render(); } catch (error) { $("#bootstrap-error").textContent = error.message; }
});
$("#bootstrap-cancel").addEventListener("click", () => $("#bootstrap-dialog").close());

document.querySelectorAll("#nav button").forEach((button) => button.addEventListener("click", () => gotoPage(button.dataset.page)));

$("#reconcile-btn").addEventListener("click", async () => {
  if (state.busy) return;
  state.busy = true;
  try { const result = await api("/api/v1/reconcile", { method: "POST" }); setAlert(`全局同步完成：${result.job_id}`, "info"); await Promise.all([refresh(), loadRemoteTunnels(), loadDnsRecords()]); render(); } catch (error) { setAlert(error.message); } finally { state.busy = false; }
});

$("#failover-btn").addEventListener("click", async () => {
  if (state.busy || !(await confirmAction("手动 GLOBAL FAILOVER", "所有受管隧道将作为一个整体切换到 STANDBY；FRPC 全部验证成功后才更新受管 DNS。继续？"))) return;
  state.busy = true;
  try { const result = await api("/api/v1/failover", { method: "POST" }); setAlert(`GLOBAL FAILOVER 完成：${result.job_id}`, "info"); await Promise.all([refresh(), loadRemoteTunnels(), loadDnsRecords(), loadNodes()]); render(); } catch (error) { setAlert(error.message); } finally { state.busy = false; }
});

try {
  const source = new EventSource("/api/v1/events");
  source.addEventListener("frpc", (event) => {
    try { state.logs.push(JSON.parse(event.data)); state.logs = state.logs.slice(-500); if (state.page === "frpc") render(); } catch { /* ignore */ }
  });
} catch { /* polling remains available */ }

await loadProviderSettings();
await refresh();
await Promise.all([loadNodes(), loadRemoteTunnels(), loadDnsRecords(), loadLogs()]);
render();
setInterval(refresh, 15000);
