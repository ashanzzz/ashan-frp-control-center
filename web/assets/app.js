const state = {
  page: "control",
  dashboard: null,
  nodes: [],
  logs: [],
  providerSettings: null,
  cloudflareZones: [],
  busy: false,
  refreshing: false,
};

const pageMeta = {
  control: ["隧道主控台", "计划隧道与 ChmlFrp / FRPC / Cloudflare 三层事实状态"],
  nodes: ["ChmlFrp", "统一活动节点、备用节点与隔离状态"],
  dns: ["Cloudflare DNS", "受管 A 记录只在新 FRPC 链路验证后切换"],
  frpc: ["FRPC Runtime", "ChmlFrp 生成配置；FRPC 负责运行、日志与故障信号"],
  activity: ["活动", "全局同步、故障切换、回滚与 DNS 操作审计"],
  settings: ["设置", "Provider 连接与全局路由策略；Token 可在 WebUI 保存并立即生效"],
};

const $ = (selector) => document.querySelector(selector);
const esc = (value) => String(value ?? "")
  .replaceAll("&", "&amp;")
  .replaceAll("<", "&lt;")
  .replaceAll(">", "&gt;")
  .replaceAll('"', "&quot;")
  .replaceAll("'", "&#39;");

function statusBadge(status) {
  const classes = { ok: "ok", failed: "bad", drift: "warn", starting: "warn" };
  const cls = classes[status?.state] ?? "";
  const detail = status?.detail ? `<small class="detail">${esc(status.detail)}</small>` : "";
  return `<span class="badge ${cls}">${esc(status?.label ?? "未知")}</span>${detail}`;
}

async function api(path, options = {}) {
  const response = await fetch(path, {
    headers: { "Content-Type": "application/json", ...(options.headers || {}) },
    ...options,
  });
  let payload = null;
  try { payload = await response.json(); } catch { /* no body */ }
  if (!response.ok) {
    throw new Error(payload?.message || payload?.error?.message || `HTTP ${response.status}`);
  }
  return payload?.data;
}

function setAlert(message = "", type = "error") {
  $("#alert").innerHTML = message ? `<div class="alert ${type}">${esc(message)}</div>` : "";
}

async function refresh() {
  if (state.refreshing) return;
  state.refreshing = true;
  try {
    const [dashboard, health] = await Promise.all([
      api("/api/v1/dashboard"),
      api("/api/v1/health"),
    ]);
    state.dashboard = dashboard;
    $("#version").textContent = `v${health.version}`;
    setAlert();
    render();
  } catch (error) {
    setAlert(`控制中心 API：${error.message}`);
  } finally {
    state.refreshing = false;
  }
}

function metric(title, value, sub, kind = "") {
  return `<article class="metric ${kind}"><span>${esc(title)}</span><strong>${esc(value)}</strong><span>${esc(sub)}</span></article>`;
}

function renderGlobal() {
  const d = state.dashboard;
  if (!d) return;
  const active = d.active_node;
  const standby = d.standby_node;
  const chmlUnavailable = d.chmlfrp_health.configured && !d.chmlfrp_health.connected;
  const activeName = active?.name || d.routing.active_node || "未配置";
  const standbyName = standby?.name || d.routing.standby_node || "未配置";
  const activeSub = active?.real_ip || (chmlUnavailable ? "ChmlFrp API 不可用" : "");
  const standbySub = standby?.real_ip || (chmlUnavailable ? "ChmlFrp API 不可用" : "");
  $("#global-cards").innerHTML = [
    metric("当前活动节点", activeName, activeSub, d.routing.active_node ? "good" : "bad"),
    metric("备用节点", standbyName, standbySub, d.routing.standby_node ? "" : "bad"),
    metric("FRPC", d.frpc.running ? "Running" : "Stopped", d.frpc.connected ? "Connected" : "Disconnected", d.frpc.running && d.frpc.connected ? "good" : "bad"),
    metric("自动故障切换", d.routing.failover_enabled ? "已开启" : "已关闭", `隔离 ${d.routing.quarantine_days} 天 · ${d.routing.state}`, d.routing.failover_enabled ? "good" : ""),
  ].join("");
}

function renderControl() {
  const d = state.dashboard;
  const rows = d.tunnel_rows.map((row) => `
    <tr>
      <td><strong>${esc(row.plan.name)}</strong><small>${esc(row.plan.protocol)}</small></td>
      <td>${esc(row.plan.local_ip)}:${esc(row.plan.local_port)}</td>
      <td>${esc(row.plan.domain)}</td>
      <td>${statusBadge(row.chmlfrp)}</td>
      <td>${statusBadge(row.frpc)}</td>
      <td>${statusBadge(row.cloudflare)}</td>
      <td>${statusBadge(row.overall)}</td>
      <td><div class="inline-actions"><button data-edit="${row.plan.id}">编辑</button><button class="danger" data-delete="${row.plan.id}">删除</button></div></td>
    </tr>`).join("");
  const banner = d.failover_job_id ? `<div class="failover-banner"><strong>GLOBAL FAILOVER</strong> · Job ${esc(d.failover_job_id)} · 所有受管隧道正在作为一个整体切换</div>` : "";
  return `${banner}<div class="panel-head"><div><h2>计划隧道</h2><p>节点统一显示在顶部，不属于单条隧道。</p></div><button id="new-tunnel">+ 新建隧道</button></div>
    <div class="table-wrap"><table><thead><tr><th>隧道</th><th>本地地址</th><th>域名</th><th>ChmlFrp</th><th>FRPC</th><th>Cloudflare</th><th>整体状态</th><th></th></tr></thead><tbody>${rows || `<tr><td colspan="8" class="muted">还没有计划隧道</td></tr>`}</tbody></table></div>`;
}

function renderNodes() {
  const d = state.dashboard;
  const card = (label, node) => `<article class="node-card"><small>${label}</small><h3>${esc(node?.name || "未配置")}</h3><p>${esc(node?.real_ip || "")}</p><span>${esc(node?.area || "")} ${esc(node?.state || "")}</span>${node?.quarantined_until ? `<p><span class="badge bad">隔离至 ${esc(node.quarantined_until)}</span></p>` : ""}</article>`;
  const all = state.nodes.map((node) => {
    const quarantine = node.quarantined_until
      ? `<span class="badge bad">${esc(node.quarantined_until)}</span> <button data-unquarantine="${esc(node.name)}">解除隔离</button>`
      : "—";
    return `<tr><td><strong>${esc(node.name)}</strong><small>${esc(node.area)}</small></td><td>${esc(node.real_ip || "")}</td><td>${esc(node.state)}</td><td>${node.web_supported ? "是" : "否"}</td><td>${quarantine}</td></tr>`;
  }).join("");
  return `<div class="node-grid">${card("ACTIVE", d.active_node)}${card("STANDBY", d.standby_node)}</div><br><div class="table-wrap"><table><thead><tr><th>节点</th><th>真实 IP</th><th>状态</th><th>Web</th><th>隔离</th></tr></thead><tbody>${all}</tbody></table></div>`;
}

function renderDns() {
  const d = state.dashboard;
  const target = d.active_node?.real_ip || "";
  const rows = d.tunnel_rows.filter((row) => row.plan.dns_managed).map((row) => `<tr><td>${esc(row.plan.domain)}</td><td>${esc(row.plan.name)}</td><td>${statusBadge(row.cloudflare)}</td><td>${esc(target)}</td></tr>`).join("");
  return `<div class="panel-head"><div><h2>Cloudflare</h2><p>${esc(d.cloudflare_health.message)}</p></div><span class="badge ${d.cloudflare_health.connected ? "ok" : "bad"}">${d.cloudflare_health.connected ? "Connected" : "Disconnected"}</span></div><div class="table-wrap"><table><thead><tr><th>域名</th><th>隧道</th><th>状态</th><th>目标 IP</th></tr></thead><tbody>${rows || `<tr><td colspan="4" class="muted">没有受管 DNS</td></tr>`}</tbody></table></div>`;
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

function renderSettings() {
  const r = state.dashboard.routing;
  const p = state.providerSettings;
  if (!p) return `<p class="muted">正在读取 Provider 设置…</p>`;

  const nodeOptions = (selected) => `<option value="">未配置</option>${state.nodes.map((node) => `<option value="${esc(node.name)}" ${node.name === selected ? "selected" : ""}>${esc(node.name)} · ${esc(node.area)}</option>`).join("")}`;
  const activeLocked = r.active_node ? "disabled" : "";
  const chmlStatus = state.dashboard.chmlfrp_health;
  const cfStatus = state.dashboard.cloudflare_health;
  const zoneOptions = cloudflareZoneOptions(p.cloudflare_zone_id);

  return `<div class="settings-stack">
    <section class="settings-card">
      <div class="settings-card-head">
        <div><h2>ChmlFrp 连接</h2><p>Token 保存到本机 /data SQLite，保存后立即热更新，不需要重启 Docker。</p></div>
        <span class="badge ${chmlStatus.connected ? "ok" : (p.chmlfrp_token_configured ? "bad" : "")}">${p.chmlfrp_token_configured ? (chmlStatus.connected ? "Connected" : "Configured") : "未配置"}</span>
      </div>
      <label>API Base URL<input id="chml-base" value="${esc(p.chmlfrp_base_url)}" autocomplete="off" spellcheck="false"></label>
      <label>API Token<input id="chml-token" type="password" autocomplete="new-password" placeholder="${p.chmlfrp_token_configured ? "已保存；留空保持当前 Token" : "粘贴 ChmlFrp API Token"}"></label>
      <div id="chml-test-result" class="provider-result muted">${esc(chmlStatus.message || "尚未测试")}</div>
      <div class="provider-actions">
        <button type="button" id="chml-test">测试连接</button>
        <button type="button" id="chml-save">保存并立即生效</button>
        ${p.chmlfrp_token_configured ? `<button type="button" id="chml-clear" class="danger">清除 Token</button>` : ""}
        <a class="button-link" href="https://panel.chmlfrp.net/" target="_blank" rel="noopener noreferrer">打开 ChmlFrp 官方面板 / 授权</a>
      </div>
      <p class="provider-note">ChmlFrp 官方客户端存在浏览器授权登录流程，但目前没有找到面向第三方自托管应用公开的 OAuth Client 注册与回调规范。本控制器不会冒用 ChmlFrp 自己的 client_id；现阶段通过官方面板获取/管理 Token 最稳妥。</p>
    </section>

    <section class="settings-card">
      <div class="settings-card-head">
        <div><h2>Cloudflare</h2><p>可先验证 Token，再自动读取可访问的 Zone；测试只读取，不写 DNS。</p></div>
        <span class="badge ${cfStatus.connected ? "ok" : (p.cloudflare_api_token_configured ? "bad" : "")}">${p.cloudflare_api_token_configured ? (cfStatus.connected ? "Connected" : "Configured") : "未配置"}</span>
      </div>
      <label>API Base URL<input id="cf-base" value="${esc(p.cloudflare_api_base)}" autocomplete="off" spellcheck="false"></label>
      <label>API Token<input id="cf-token" type="password" autocomplete="new-password" placeholder="${p.cloudflare_api_token_configured ? "已保存；留空保持当前 Token" : "粘贴 Cloudflare scoped API Token"}"></label>
      <div class="form-grid provider-zone-grid">
        <label>Zone<select id="cf-zone-list">${zoneOptions}</select></label>
        <label>Zone ID<input id="cf-zone" value="${esc(p.cloudflare_zone_id)}" placeholder="可读取 Zone 自动填入，也可手动输入" autocomplete="off"></label>
      </div>
      <div id="cf-test-result" class="provider-result muted">${esc(cfStatus.message || "尚未测试")}</div>
      <div class="provider-actions">
        <button type="button" id="cf-zones">验证 Token / 读取 Zone</button>
        <button type="button" id="cf-test">测试 Token + Zone</button>
        <button type="button" id="cf-save">保存并立即生效</button>
        ${p.cloudflare_api_token_configured ? `<button type="button" id="cf-clear" class="danger">清除 Token</button>` : ""}
      </div>
      <p class="provider-note">推荐 Token 权限至少包含 Zone Read 与受管 Zone 的 DNS Edit。这里的“测试”不会创建、修改或删除 DNS，因此不会用写操作探测 DNS Edit 权限。</p>
    </section>

    <section class="settings-card">
      <div class="settings-card-head"><div><h2>全局路由策略</h2><p>已有活动节点后不能直接改 Active；必须使用 GLOBAL FAILOVER，让全部隧道作为一个整体迁移。</p></div></div>
      <form id="routing-form">
        <div class="form-grid">
          <label>活动节点<select id="routing-active" ${activeLocked}>${nodeOptions(r.active_node)}</select></label>
          <label>备用节点<select id="routing-standby">${nodeOptions(r.standby_node)}</select></label>
        </div>
        <label>故障节点隔离天数<input id="routing-days" type="number" min="1" max="3650" value="${esc(r.quarantine_days)}"></label>
        <label class="check compact-check"><input id="routing-enabled" type="checkbox" ${r.failover_enabled ? "checked" : ""}> 启用 FRPC 日志驱动的自动全局切换</label>
        <div class="actions"><button type="submit">保存路由策略</button></div>
      </form>
    </section>
  </div>`;
}

function cloudflareZoneOptions(currentZoneId = "") {
  const options = [`<option value="">${state.cloudflareZones.length ? "请选择 Zone" : "读取 Zone 后可选择"}</option>`];
  const currentKnown = state.cloudflareZones.some((zone) => zone.id === currentZoneId);
  if (currentZoneId && !currentKnown) {
    options.push(`<option value="${esc(currentZoneId)}" selected>当前 / 手动 Zone · ${esc(currentZoneId)}</option>`);
  }
  for (const zone of state.cloudflareZones) {
    options.push(`<option value="${esc(zone.id)}" ${zone.id === currentZoneId ? "selected" : ""}>${esc(zone.name)} · ${esc(zone.status || "unknown")}</option>`);
  }
  return options.join("");
}

function render() {
  if (!state.dashboard) return;
  renderGlobal();
  const [title, subtitle] = pageMeta[state.page];
  $("#page-title").textContent = title;
  $("#page-subtitle").textContent = subtitle;
  document.querySelectorAll("#nav button").forEach((button) => button.classList.toggle("active", button.dataset.page === state.page));
  const renderers = { control: renderControl, nodes: renderNodes, dns: renderDns, frpc: renderFrpc, activity: renderActivity, settings: renderSettings };
  $("#content").classList.remove("loading");
  $("#content").innerHTML = renderers[state.page]();
  bindPageActions();
}

async function loadNodes() {
  try { state.nodes = await api("/api/v1/nodes"); } catch { state.nodes = []; }
}

async function loadProviderSettings() {
  try {
    state.providerSettings = await api("/api/v1/settings/providers");
  } catch (error) {
    state.providerSettings = null;
    setAlert(`读取 Provider 设置失败：${error.message}`);
  }
}

async function loadLogs() {
  try { state.logs = await api("/api/v1/frpc/logs"); if (state.page === "frpc") render(); } catch { /* dashboard remains usable */ }
}

function openTunnel(plan = null) {
  $("#tunnel-dialog-title").textContent = plan ? "编辑计划隧道" : "新增计划隧道";
  $("#tunnel-id").value = plan?.id || "";
  $("#tunnel-name").value = plan?.name || "";
  $("#tunnel-ip").value = plan?.local_ip || "192.168.8.11";
  $("#tunnel-port").value = plan?.local_port || "";
  $("#tunnel-protocol").value = plan?.protocol || "http";
  $("#tunnel-domain").value = plan?.domain || "";
  $("#tunnel-dns").checked = plan?.dns_managed ?? true;
  $("#dialog-error").textContent = "";
  $("#tunnel-dialog").showModal();
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

function bindPageActions() {
  $("#new-tunnel")?.addEventListener("click", () => openTunnel());
  document.querySelectorAll("[data-edit]").forEach((button) => button.addEventListener("click", () => {
    const id = Number(button.dataset.edit);
    const plan = state.dashboard.tunnel_rows.find((row) => row.plan.id === id)?.plan;
    if (plan) openTunnel(plan);
  }));
  document.querySelectorAll("[data-delete]").forEach((button) => button.addEventListener("click", async () => {
    const id = Number(button.dataset.delete);
    const plan = state.dashboard.tunnel_rows.find((row) => row.plan.id === id)?.plan;
    if (!plan || !(await confirmAction("删除计划", `只在 ChmlFrp 远端隧道和受管 DNS 都已清理时允许删除“${plan.name}”。继续？`))) return;
    try { await api(`/api/v1/tunnels/${id}`, { method: "DELETE" }); await refresh(); } catch (error) { setAlert(error.message); }
  }));
  document.querySelectorAll("[data-unquarantine]").forEach((button) => button.addEventListener("click", async () => {
    const name = button.dataset.unquarantine;
    if (!(await confirmAction("解除节点隔离", `节点“${name}”将重新进入候选池，但不会自动回切。继续？`))) return;
    try {
      await api(`/api/v1/nodes/${encodeURIComponent(name)}/unquarantine`, { method: "POST" });
      await loadNodes();
      await refresh();
    } catch (error) {
      setAlert(error.message);
    }
  }));
  document.querySelectorAll("[data-frpc]").forEach((button) => button.addEventListener("click", async () => {
    try { await api(`/api/v1/frpc/${button.dataset.frpc}`, { method: "POST" }); await refresh(); await loadLogs(); } catch (error) { setAlert(error.message); }
  }));
  $("#routing-form")?.addEventListener("submit", saveRouting);
  $("#chml-test")?.addEventListener("click", testChmlFrp);
  $("#chml-save")?.addEventListener("click", saveChmlFrpSettings);
  $("#chml-clear")?.addEventListener("click", clearChmlFrpToken);
  $("#cf-zones")?.addEventListener("click", loadCloudflareZones);
  $("#cf-test")?.addEventListener("click", testCloudflare);
  $("#cf-save")?.addEventListener("click", saveCloudflareSettings);
  $("#cf-clear")?.addEventListener("click", clearCloudflareToken);
  $("#cf-zone-list")?.addEventListener("change", (event) => {
    if (event.target.value) $("#cf-zone").value = event.target.value;
  });
}

function providerResult(selector, message, ok = true) {
  const element = $(selector);
  if (!element) return;
  element.textContent = message;
  element.classList.remove("muted", "provider-ok", "provider-bad");
  element.classList.add(ok ? "provider-ok" : "provider-bad");
}

function chmlProbeBody() {
  return {
    base_url: $("#chml-base")?.value.trim() || state.providerSettings.chmlfrp_base_url,
    token: $("#chml-token")?.value.trim() || null,
  };
}

function cloudflareProbeBody() {
  return {
    base_url: $("#cf-base")?.value.trim() || state.providerSettings.cloudflare_api_base,
    token: $("#cf-token")?.value.trim() || null,
    zone_id: $("#cf-zone")?.value.trim() || null,
  };
}

async function testChmlFrp() {
  try {
    providerResult("#chml-test-result", "正在测试…", true);
    const result = await api("/api/v1/settings/providers/test/chmlfrp", { method: "POST", body: JSON.stringify(chmlProbeBody()) });
    providerResult("#chml-test-result", `${result.message} · ${result.tunnels} 条隧道`, true);
  } catch (error) {
    providerResult("#chml-test-result", error.message, false);
  }
}

async function saveChmlFrpSettings() {
  const p = state.providerSettings;
  const body = {
    chmlfrp_base_url: $("#chml-base").value.trim(),
    chmlfrp_token: $("#chml-token").value.trim() || null,
    clear_chmlfrp_token: false,
    cloudflare_api_base: p.cloudflare_api_base,
    cloudflare_api_token: null,
    clear_cloudflare_api_token: false,
    cloudflare_zone_id: p.cloudflare_zone_id,
  };
  await saveProviderSettings(body, "ChmlFrp 配置已保存并立即生效");
}

async function clearChmlFrpToken() {
  if (!(await confirmAction("清除 ChmlFrp Token", "清除后 ChmlFrp 管理、节点详情、同步和故障切换将不可用，直到重新配置 Token。继续？"))) return;
  const p = state.providerSettings;
  const body = {
    chmlfrp_base_url: $("#chml-base").value.trim(),
    chmlfrp_token: null,
    clear_chmlfrp_token: true,
    cloudflare_api_base: p.cloudflare_api_base,
    cloudflare_api_token: null,
    clear_cloudflare_api_token: false,
    cloudflare_zone_id: p.cloudflare_zone_id,
  };
  await saveProviderSettings(body, "ChmlFrp Token 已清除");
}

async function loadCloudflareZones() {
  try {
    providerResult("#cf-test-result", "正在验证 Token 并读取 Zone…", true);
    const zones = await api("/api/v1/settings/providers/cloudflare/zones", { method: "POST", body: JSON.stringify(cloudflareProbeBody()) });
    state.cloudflareZones = zones;
    const select = $("#cf-zone-list");
    const current = $("#cf-zone").value.trim() || state.providerSettings.cloudflare_zone_id;
    select.innerHTML = cloudflareZoneOptions(current);
    if (!current && zones.length === 1) {
      select.value = zones[0].id;
      $("#cf-zone").value = zones[0].id;
    }
    providerResult("#cf-test-result", `Token 有效，读取到 ${zones.length} 个可访问 Zone`, true);
  } catch (error) {
    providerResult("#cf-test-result", error.message, false);
  }
}

async function testCloudflare() {
  try {
    providerResult("#cf-test-result", "正在测试…", true);
    const result = await api("/api/v1/settings/providers/test/cloudflare", { method: "POST", body: JSON.stringify(cloudflareProbeBody()) });
    const detail = result.dns_read_tested ? ` · 读取到 ${result.a_records} 条 A 记录` : "";
    providerResult("#cf-test-result", `${result.message}${detail}`, true);
  } catch (error) {
    providerResult("#cf-test-result", error.message, false);
  }
}

async function saveCloudflareSettings() {
  const p = state.providerSettings;
  const body = {
    chmlfrp_base_url: p.chmlfrp_base_url,
    chmlfrp_token: null,
    clear_chmlfrp_token: false,
    cloudflare_api_base: $("#cf-base").value.trim(),
    cloudflare_api_token: $("#cf-token").value.trim() || null,
    clear_cloudflare_api_token: false,
    cloudflare_zone_id: $("#cf-zone").value.trim(),
  };
  await saveProviderSettings(body, "Cloudflare 配置已保存并立即生效");
}

async function clearCloudflareToken() {
  if (!(await confirmAction("清除 Cloudflare Token", "清除后受管 DNS 不会自动切换，直到重新配置 Cloudflare。继续？"))) return;
  const p = state.providerSettings;
  const body = {
    chmlfrp_base_url: p.chmlfrp_base_url,
    chmlfrp_token: null,
    clear_chmlfrp_token: false,
    cloudflare_api_base: $("#cf-base").value.trim(),
    cloudflare_api_token: null,
    clear_cloudflare_api_token: true,
    cloudflare_zone_id: $("#cf-zone").value.trim(),
  };
  await saveProviderSettings(body, "Cloudflare Token 已清除");
}

async function saveProviderSettings(body, successMessage) {
  try {
    state.providerSettings = await api("/api/v1/settings/providers", { method: "PUT", body: JSON.stringify(body) });
    state.cloudflareZones = [];
    await loadNodes();
    await refresh();
    setAlert(successMessage, "info");
    if (state.page === "settings") render();
  } catch (error) {
    setAlert(error.message);
  }
}

async function saveRouting(event) {
  event.preventDefault();
  const current = state.dashboard.routing;
  const active = current.active_node || $("#routing-active").value || null;
  const standby = $("#routing-standby").value || null;
  const body = { active_node: active, standby_node: standby, quarantine_days: Number($("#routing-days").value), failover_enabled: $("#routing-enabled").checked };
  try { await api("/api/v1/routing", { method: "PUT", body: JSON.stringify(body) }); await loadNodes(); await refresh(); } catch (error) { setAlert(error.message); }
}

$("#tunnel-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const id = $("#tunnel-id").value;
  const body = { name: $("#tunnel-name").value.trim(), local_ip: $("#tunnel-ip").value.trim(), local_port: Number($("#tunnel-port").value), protocol: $("#tunnel-protocol").value, domain: $("#tunnel-domain").value.trim(), dns_managed: $("#tunnel-dns").checked };
  try {
    await api(id ? `/api/v1/tunnels/${id}` : "/api/v1/tunnels", { method: id ? "PUT" : "POST", body: JSON.stringify(body) });
    $("#tunnel-dialog").close();
    await refresh();
  } catch (error) { $("#dialog-error").textContent = error.message; }
});
$("#dialog-cancel").addEventListener("click", () => $("#tunnel-dialog").close());

document.querySelectorAll("#nav button").forEach((button) => button.addEventListener("click", async () => {
  state.page = button.dataset.page;
  if (state.page === "nodes" || state.page === "settings") await loadNodes();
  if (state.page === "settings") await loadProviderSettings();
  if (state.page === "frpc") await loadLogs();
  render();
}));

$("#reconcile-btn").addEventListener("click", async () => {
  if (state.busy) return;
  state.busy = true;
  try { const result = await api("/api/v1/reconcile", { method: "POST" }); setAlert(`全局同步任务已完成：${result.job_id}`, "info"); await refresh(); } catch (error) { setAlert(error.message); } finally { state.busy = false; }
});

$("#failover-btn").addEventListener("click", async () => {
  if (state.busy || !(await confirmAction("手动全局切换", "这会把所有受管隧道作为一个整体切换到备用节点，并在 FRPC 验证后统一更新受管 DNS。继续？"))) return;
  state.busy = true;
  try { const result = await api("/api/v1/failover", { method: "POST" }); setAlert(`全局切换任务已完成：${result.job_id}`, "info"); await refresh(); } catch (error) { setAlert(error.message); } finally { state.busy = false; }
});

try {
  const source = new EventSource("/api/v1/events");
  source.addEventListener("frpc", (event) => {
    try {
      state.logs.push(JSON.parse(event.data));
      state.logs = state.logs.slice(-500);
      if (state.page === "frpc") render();
    } catch { /* ignore malformed event */ }
  });
} catch { /* polling remains available */ }

await loadNodes();
await refresh();
await loadLogs();
setInterval(refresh, 15000);
