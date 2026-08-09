use ashan_frp_domain::{FaultDomain, FrpcEvent, FrpcEventType};
use chrono::Utc;

pub fn classify(line: &str, local_config_has_duplicates: bool) -> FrpcEvent {
    let lower = line.to_ascii_lowercase();
    let proxy_name = extract_proxy_name(line);
    let mut e = FrpcEvent { at:Utc::now(), raw:line.to_string(), proxy_name, event_type:FrpcEventType::Info,
        fault_domain:FaultDomain::Unknown, severity:"info".into(), triggers_failover:false };

    if contains_any(&lower, &["成功登录至服务器", "login to server success", "login to server succeeded"]){
        e.event_type=FrpcEventType::LoginSuccess; return e;
    }
    if contains_any(&lower, &["proxy added", "代理已添加"]){ e.event_type=FrpcEventType::ProxyAdded; return e; }
    if contains_any(&lower, &["start proxy success", "映射启动成功", "启动成功"]){ e.event_type=FrpcEventType::ProxyStarted; return e; }

    if contains_any(&lower, &["connect to local service", "local service", "connection refused"]){
        e.event_type=FrpcEventType::LocalServiceFailure; e.fault_domain=FaultDomain::Local; e.severity="warning".into(); return e;
    }
    if contains_any(&lower, &["配置文件与记录不匹配", "client proxy parameter", "proxy parameter error"]){
        e.event_type=FrpcEventType::ConfigMismatch; e.fault_domain=FaultDomain::Config; e.severity="error".into(); return e;
    }
    if contains_any(&lower, &["token in login doesn't match", "authentication failed", "invalid token", "token mismatch"]){
        e.event_type=FrpcEventType::AuthFailure; e.fault_domain=FaultDomain::Auth; e.severity="error".into(); return e;
    }
    if contains_any(&lower, &["already exist", "already exists", "proxy name conflict", "隧道端口被占用"]){
        if local_config_has_duplicates {
            e.event_type=FrpcEventType::LocalDuplicateProxy; e.fault_domain=FaultDomain::Config; e.severity="error".into();
        } else {
            e.event_type=FrpcEventType::ServerProxyConflict; e.fault_domain=FaultDomain::Node; e.severity="critical".into(); e.triggers_failover=true;
        }
        return e;
    }
    if contains_any(&lower, &["connection reset by peer", "session shutdown", "control connection closed", "server closed", "heartbeat timeout", "unexpected eof"]){
        e.event_type=FrpcEventType::ServerConnectionFailure; e.fault_domain=FaultDomain::Node; e.severity="critical".into(); e.triggers_failover=true; return e;
    }
    if contains_any(&lower, &["connect to server error", "i/o timeout", "network is unreachable", "no route to host"]){
        e.event_type=FrpcEventType::NetworkAmbiguous; e.fault_domain=FaultDomain::Network; e.severity="error".into(); return e;
    }
    if contains_any(&lower, &["[e]", " error", "failed", "失败"]){
        e.event_type=FrpcEventType::UnknownError; e.severity="error".into();
    }
    e
}

fn contains_any(h:&str, ns:&[&str])->bool{ns.iter().any(|n|h.contains(n))}

pub fn extract_proxy_name(line:&str)->Option<String>{
    let mut groups=Vec::new(); let mut start=None;
    for (i,c) in line.char_indices(){
        if c=='[' { start=Some(i+1); }
        else if c==']' { if let Some(s)=start.take(){ if s<i { groups.push(line[s..i].to_string()); } } }
    }
    groups.into_iter().rev().find(|g| {
        let gl=g.to_ascii_lowercase();
        !gl.ends_with(".go") && !gl.contains("service.go") && !matches!(gl.as_str(),"i"|"w"|"e"|"d"|"info"|"warn"|"error")
    })
}

pub fn config_has_duplicate_proxy_names(config: &str) -> bool {
    let mut seen = std::collections::HashSet::new();
    let mut in_toml_proxy = false;

    for raw in config.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        // Modern frpc TOML repeats [[proxies]] once per proxy. The section name
        // itself is not a proxy name; only the `name = ...` field is.
        if line == "[[proxies]]" {
            in_toml_proxy = true;
            continue;
        }

        // Any other TOML array/table ends the current proxy-name context.
        if line.starts_with("[[") && line.ends_with("]]" ) {
            in_toml_proxy = false;
            continue;
        }

        // Legacy INI uses one section per proxy: [proxy-name].
        if line.starts_with('[') && line.ends_with(']') && !line.starts_with("[[") {
            in_toml_proxy = false;
            let name = line.trim_matches(&['[', ']'][..]).trim();
            if !name.is_empty() && !name.eq_ignore_ascii_case("common")
                && !seen.insert(name.to_string())
            {
                return true;
            }
            continue;
        }

        if in_toml_proxy {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim() == "name" {
                    let name = value.trim().trim_matches(&['"', '\''][..]);
                    if !name.is_empty() && !seen.insert(name.to_string()) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn server_reset_triggers_global_failover(){ let e=classify("[x] connection reset by peer",false); assert!(e.triggers_failover); assert_eq!(e.fault_domain,FaultDomain::Node); }
    #[test] fn local_failure_never_triggers(){ let e=classify("[x] connect to local service error: connection refused",false); assert!(!e.triggers_failover); assert_eq!(e.fault_domain,FaultDomain::Local); }
    #[test] fn conflict_is_node_fault_when_local_config_unique(){ let e=classify("[abc] proxy already exists",false); assert!(e.triggers_failover); }
    #[test] fn conflict_is_config_fault_when_local_config_duplicate(){ let e=classify("[abc] proxy already exists",true); assert!(!e.triggers_failover); assert_eq!(e.fault_domain,FaultDomain::Config); }
    #[test] fn failed_login_is_not_success(){ let e=classify("login to server failed: authentication failed",false); assert_ne!(e.event_type,FrpcEventType::LoginSuccess); }
    #[test] fn ini_duplicate_proxy_is_detected(){ assert!(config_has_duplicate_proxy_names("[common]\nserver_addr=x\n[a]\ntype=http\n[a]\ntype=http")); }
    #[test] fn distinct_toml_proxies_are_not_duplicates(){ assert!(!config_has_duplicate_proxy_names("[[proxies]]\nname = \"a\"\ntype=\"http\"\n[[proxies]]\nname = \"b\"\ntype=\"http\"")); }
    #[test] fn toml_duplicate_names_are_detected(){ assert!(config_has_duplicate_proxy_names("[[proxies]]\nname = \"a\"\n[[proxies]]\nname = \"a\"")); }
}
