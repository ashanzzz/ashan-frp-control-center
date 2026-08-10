use ashan_frp_domain::{
    ApiResponse, DashboardSnapshot, FrpcEvent, LayerState, LayerStatus, NodeSummary,
    TunnelPlanInput,
};
use dioxus::prelude::*;
use gloo_net::http::Request;
use gloo_timers::future::TimeoutFuture;

static CSS: Asset = asset!("/assets/main.css");

fn main() {
    dioxus::launch(App);
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Control,
    Chml,
    Dns,
    Frpc,
    Activity,
}

#[component]
fn App() -> Element {
    let mut tab = use_signal(|| Tab::Control);
    let dashboard = use_signal(|| None::<DashboardSnapshot>);
    let error = use_signal(|| None::<String>);
    let mut busy = use_signal(|| false);

    let mut dashboard_task = dashboard;
    let mut error_task = error;
    use_future(move || async move {
        loop {
            match get_json::<DashboardSnapshot>("/api/v1/dashboard").await {
                Ok(value) => {
                    dashboard_task.set(Some(value));
                    error_task.set(None);
                }
                Err(err) => error_task.set(Some(err)),
            }
            TimeoutFuture::new(10000).await;
        }
    });

    rsx! {
        document::Stylesheet { href: CSS }
        div { class: "shell",
            aside { class: "sidebar",
                div { class: "brand",
                    "Ashan FRP"
                    small { "Control Center" }
                }
                nav {
                    button {
                        class: nav_class(*tab.read(), Tab::Control),
                        onclick: move |_| tab.set(Tab::Control),
                        "隧道主控台"
                    }
                    button {
                        class: nav_class(*tab.read(), Tab::Chml),
                        onclick: move |_| tab.set(Tab::Chml),
                        "ChmlFrp"
                    }
                    button {
                        class: nav_class(*tab.read(), Tab::Dns),
                        onclick: move |_| tab.set(Tab::Dns),
                        "DNS"
                    }
                    button {
                        class: nav_class(*tab.read(), Tab::Frpc),
                        onclick: move |_| tab.set(Tab::Frpc),
                        "FRPC"
                    }
                    button {
                        class: nav_class(*tab.read(), Tab::Activity),
                        onclick: move |_| tab.set(Tab::Activity),
                        "活动"
                    }
                }
            }
            main { class: "main",
                if let Some(err) = error.read().as_ref() {
                    div { class: "alert error", "API: {err}" }
                }
                if let Some(snapshot) = dashboard.read().as_ref() {
                    Header {
                        snapshot: snapshot.clone(),
                        busy: *busy.read(),
                        on_reconcile: move |_| {
                            busy.set(true);
                            let mut busy_after = busy;
                            spawn(async move {
                                let _ = post_empty("/api/v1/reconcile").await;
                                busy_after.set(false);
                            });
                        },
                        on_failover: move |_| {
                            busy.set(true);
                            let mut busy_after = busy;
                            spawn(async move {
                                let _ = post_empty("/api/v1/failover").await;
                                busy_after.set(false);
                            });
                        },
                    }
                    match *tab.read() {
                        Tab::Control => rsx! { ControlPage { snapshot: snapshot.clone() } },
                        Tab::Chml => rsx! { ChmlPage { snapshot: snapshot.clone() } },
                        Tab::Dns => rsx! { DnsPage { snapshot: snapshot.clone() } },
                        Tab::Frpc => rsx! { FrpcPage { snapshot: snapshot.clone() } },
                        Tab::Activity => rsx! { ActivityPage { snapshot: snapshot.clone() } },
                    }
                } else {
                    div { class: "loading", "正在连接控制中心…" }
                }
            }
        }
    }
}

fn nav_class(active: Tab, target: Tab) -> &'static str {
    if active == target { "nav active" } else { "nav" }
}

#[component]
fn Header(
    snapshot: DashboardSnapshot,
    busy: bool,
    on_reconcile: EventHandler<()>,
    on_failover: EventHandler<()>,
) -> Element {
    let active_name = snapshot
        .active_node
        .as_ref()
        .map(|node| node.name.clone())
        .unwrap_or_else(|| "未配置".into());
    let active_ip = snapshot
        .active_node
        .as_ref()
        .and_then(|node| node.real_ip.clone())
        .unwrap_or_default();
    let standby_name = snapshot
        .standby_node
        .as_ref()
        .map(|node| node.name.clone())
        .unwrap_or_else(|| "未配置".into());
    let standby_ip = snapshot
        .standby_node
        .as_ref()
        .and_then(|node| node.real_ip.clone())
        .unwrap_or_default();

    rsx! {
        header { class: "topbar",
            div {
                h1 { "隧道主控台" }
                p { "全部隧道统一节点 · FRPC 日志驱动全局故障切换" }
            }
            div { class: "actions",
                button {
                    disabled: busy,
                    onclick: move |_| on_reconcile.call(()),
                    "全局同步"
                }
                button {
                    class: "danger",
                    disabled: busy,
                    onclick: move |_| on_failover.call(()),
                    "手动全局切换"
                }
            }
        }
        div { class: "global-grid",
            Metric {
                title: "当前活动节点",
                value: active_name,
                sub: active_ip,
                kind: "good",
            }
            Metric {
                title: "备用节点",
                value: standby_name,
                sub: standby_ip,
                kind: "neutral",
            }
            Metric {
                title: "FRPC",
                value: (if snapshot.frpc.running { "Running" } else { "Stopped" }).into(),
                sub: (if snapshot.frpc.connected { "Connected" } else { "Waiting" }).into(),
                kind: if snapshot.frpc.running { "good" } else { "bad" },
            }
            Metric {
                title: "自动故障切换",
                value: (if snapshot.routing.failover_enabled { "已开启" } else { "已关闭" }).into(),
                sub: format!("隔离期 {} 天", snapshot.routing.quarantine_days),
                kind: if snapshot.routing.failover_enabled { "good" } else { "neutral" },
            }
        }
        if let Some(job) = snapshot.failover_job_id {
            div { class: "failover-banner",
                strong { "GLOBAL FAILOVER" }
                span { "Job {job}" }
                span { "所有受管隧道作为一个整体切换" }
            }
        }
    }
}

#[component]
fn Metric(title: &'static str, value: String, sub: String, kind: &'static str) -> Element {
    rsx! {
        div { class: "metric {kind}",
            small { "{title}" }
            strong { "{value}" }
            span { "{sub}" }
        }
    }
}

#[component]
fn ControlPage(snapshot: DashboardSnapshot) -> Element {
    let mut show_new = use_signal(|| false);
    rsx! {
        section { class: "panel",
            div { class: "panel-head",
                div {
                    h2 { "计划隧道" }
                    p { "ChmlFrp / FRPC / Cloudflare 三层事实状态" }
                }
                button { onclick: move |_| show_new.set(true), "+ 新建隧道" }
            }
            div { class: "table-wrap",
                table {
                    thead {
                        tr {
                            th { "隧道" }
                            th { "本地地址" }
                            th { "域名" }
                            th { "ChmlFrp" }
                            th { "FRPC" }
                            th { "Cloudflare" }
                            th { "整体状态" }
                        }
                    }
                    tbody {
                        for row in snapshot.tunnel_rows.iter() {
                            tr {
                                td {
                                    strong { "{row.plan.name}" }
                                    small { "{row.plan.protocol}" }
                                }
                                td { "{row.plan.local_ip}:{row.plan.local_port}" }
                                td { "{row.plan.domain}" }
                                StatusCell { status: row.chmlfrp.clone() }
                                StatusCell { status: row.frpc.clone() }
                                StatusCell { status: row.cloudflare.clone() }
                                StatusCell { status: row.overall.clone() }
                            }
                        }
                    }
                }
            }
            if *show_new.read() {
                NewTunnelDialog { on_close: move |_| show_new.set(false) }
            }
        }
    }
}

#[component]
fn StatusCell(status: LayerStatus) -> Element {
    let class_name = match status.state {
        LayerState::Ok => "ok",
        LayerState::Failed => "bad",
        LayerState::Drift | LayerState::Starting => "warn",
        _ => "muted",
    };
    rsx! {
        td {
            span { class: "badge {class_name}", "{status.label}" }
            if let Some(detail) = status.detail {
                small { class: "detail", "{detail}" }
            }
        }
    }
}

#[component]
fn NewTunnelDialog(on_close: EventHandler<()>) -> Element {
    let mut name = use_signal(String::new);
    let mut ip = use_signal(|| "192.168.8.11".to_string());
    let mut port = use_signal(String::new);
    let mut protocol = use_signal(|| "http".to_string());
    let mut domain = use_signal(String::new);
    let error = use_signal(|| None::<String>);
    let close_cancel = on_close.clone();
    let close_save = on_close.clone();

    rsx! {
        div { class: "modal",
            div { class: "dialog",
                h2 { "新增计划隧道" }
                p { "节点无需填写：所有隧道自动使用全局活动节点。" }
                label {
                    "隧道名"
                    input { value: "{name}", oninput: move |event| name.set(event.value()) }
                }
                label {
                    "本地 IP"
                    input { value: "{ip}", oninput: move |event| ip.set(event.value()) }
                }
                label {
                    "本地端口"
                    input { value: "{port}", oninput: move |event| port.set(event.value()) }
                }
                label {
                    "协议"
                    select {
                        value: "{protocol}",
                        onchange: move |event| protocol.set(event.value()),
                        option { value: "http", "HTTP" }
                        option { value: "https", "HTTPS" }
                        option { value: "tcp", "TCP" }
                        option { value: "udp", "UDP" }
                    }
                }
                label {
                    "域名 / 公网标识"
                    input { value: "{domain}", oninput: move |event| domain.set(event.value()) }
                }
                if let Some(message) = error.read().as_ref() {
                    div { class: "alert error", "{message}" }
                }
                div { class: "actions",
                    button {
                        class: "ghost",
                        onclick: move |_| close_cancel.call(()),
                        "取消"
                    }
                    button {
                        onclick: move |_| {
                            let input = TunnelPlanInput {
                                name: name.read().clone(),
                                local_ip: ip.read().clone(),
                                local_port: port.read().parse().unwrap_or(0),
                                protocol: protocol.read().clone(),
                                domain: domain.read().clone(),
                                dns_managed: true,
                            };
                            let close = close_save.clone();
                            spawn(async move {
                                match post_json("/api/v1/tunnels", &input).await {
                                    Ok(()) => close.call(()),
                                    Err(err) => error.set(Some(err)),
                                }
                            });
                        },
                        "保存计划"
                    }
                }
            }
        }
    }
}

#[component]
fn ChmlPage(snapshot: DashboardSnapshot) -> Element {
    rsx! {
        section { class: "panel",
            h2 { "ChmlFrp 全局节点" }
            p { "所有隧道统一使用同一个活动节点；不存在单隧道节点选择。" }
            div { class: "node-cards",
                NodeCard { title: "ACTIVE", node: snapshot.active_node.clone() }
                NodeCard { title: "STANDBY", node: snapshot.standby_node.clone() }
            }
            div { class: "notice",
                "节点切换只能是全局切换：任意确认的 Node 级 FRPC 故障会迁移全部受管隧道。"
            }
        }
    }
}

#[component]
fn NodeCard(title: &'static str, node: Option<NodeSummary>) -> Element {
    rsx! {
        div { class: "node-card",
            small { "{title}" }
            if let Some(node) = node {
                h3 { "{node.name}" }
                p { "{node.real_ip.unwrap_or_default()}" }
                span { "{node.area} · {node.state}" }
                if let Some(until) = node.quarantined_until {
                    span { class: "badge bad", "隔离至 {until}" }
                }
            } else {
                h3 { "未配置" }
            }
        }
    }
}

#[component]
fn DnsPage(snapshot: DashboardSnapshot) -> Element {
    let target_ip = snapshot
        .active_node
        .as_ref()
        .and_then(|node| node.real_ip.clone())
        .unwrap_or_default();
    rsx! {
        section { class: "panel",
            h2 { "Cloudflare DNS" }
            p { "受管 A 记录最终指向全局活动节点 IP；DNS 永远在 FRPC 新链路验证后更新。" }
            div { class: "table-wrap",
                table {
                    thead {
                        tr {
                            th { "域名" }
                            th { "隧道" }
                            th { "Cloudflare" }
                            th { "目标 IP" }
                        }
                    }
                    tbody {
                        for row in snapshot.tunnel_rows.iter().filter(|row| row.plan.dns_managed) {
                            tr {
                                td { "{row.plan.domain}" }
                                td { "{row.plan.name}" }
                                StatusCell { status: row.cloudflare.clone() }
                                td { "{target_ip}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn FrpcPage(snapshot: DashboardSnapshot) -> Element {
    let logs = use_resource(move || async move {
        get_json::<Vec<FrpcEvent>>("/api/v1/frpc/logs")
            .await
            .unwrap_or_default()
    });

    rsx! {
        section { class: "panel",
            h2 { "FRPC Runtime" }
            p { "配置只来自 ChmlFrp；FRPC 负责运行、日志观测和 Node 级故障触发。" }
            div { class: "global-grid",
                Metric {
                    title: "进程",
                    value: (if snapshot.frpc.running { "Running" } else { "Stopped" }).into(),
                    sub: format!("PID {:?}", snapshot.frpc.pid),
                    kind: if snapshot.frpc.running { "good" } else { "bad" },
                }
                Metric {
                    title: "服务端连接",
                    value: (if snapshot.frpc.connected { "Connected" } else { "Disconnected" }).into(),
                    sub: snapshot.frpc.last_error.clone().unwrap_or_default(),
                    kind: if snapshot.frpc.connected { "good" } else { "bad" },
                }
            }
            div { class: "logbox",
                if let Some(items) = logs.read().as_ref() {
                    for event in items.iter() {
                        div { class: "logline",
                            span { class: "time", "{event.at}" }
                            span { class: "level {event.severity}", "{event.severity}" }
                            code { "{event.raw}" }
                            if event.triggers_failover {
                                span { class: "badge bad", "GLOBAL FAILOVER" }
                            }
                        }
                    }
                } else {
                    div { "正在读取 FRPC 日志…" }
                }
            }
        }
    }
}

#[component]
fn ActivityPage(snapshot: DashboardSnapshot) -> Element {
    rsx! {
        section { class: "panel",
            h2 { "活动" }
            div { class: "timeline",
                for event in snapshot.recent_activity.iter() {
                    div { class: "event",
                        span { class: "badge muted", "{event.kind}" }
                        strong { "{event.message}" }
                        small { "{event.created_at}" }
                        if let Some(job) = &event.job_id {
                            code { "{job}" }
                        }
                    }
                }
            }
        }
    }
}

async fn get_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, String> {
    let response = Request::get(url).send().await.map_err(|err| err.to_string())?;
    if !response.ok() {
        return Err(format!("HTTP {}", response.status()));
    }
    let body: ApiResponse<T> = response.json().await.map_err(|err| err.to_string())?;
    Ok(body.data)
}

async fn post_empty(url: &str) -> Result<(), String> {
    let response = Request::post(url).send().await.map_err(|err| err.to_string())?;
    if response.ok() {
        Ok(())
    } else {
        Err(format!("HTTP {}", response.status()))
    }
}

async fn post_json<T: serde::Serialize>(url: &str, value: &T) -> Result<(), String> {
    let request = Request::post(url).json(value).map_err(|err| err.to_string())?;
    let response = request.send().await.map_err(|err| err.to_string())?;
    if response.ok() {
        Ok(())
    } else {
        Err(format!("HTTP {}", response.status()))
    }
}
