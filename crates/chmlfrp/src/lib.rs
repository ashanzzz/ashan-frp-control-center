use anyhow::{anyhow, Context, Result};
use ashan_frp_domain::{NodeSummary, TunnelPlan};
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct ChmlFrpClient {
    http: Client,
    base_url: String,
    token: String,
}

impl ChmlFrpClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self { http: Client::new(), base_url: base_url.into().trim_end_matches('/').to_string(), token: token.into() }
    }
    pub fn configured(&self) -> bool { !self.token.trim().is_empty() }

    pub async fn list_tunnels(&self) -> Result<Vec<RemoteTunnel>> {
        self.require_token()?;
        let resp: Envelope<Vec<RemoteTunnel>> = self.http.get(format!("{}/tunnel", self.base_url))
            .query(&[("token", &self.token)]).send().await?.error_for_status()?.json().await?;
        ensure_success(&resp.state, resp.code, &resp.msg)?;
        Ok(resp.data.unwrap_or_default())
    }

    pub async fn list_nodes(&self) -> Result<Vec<NodeSummary>> {
        let resp: Envelope<Vec<NodeListItem>> = self.http.get(format!("{}/node", self.base_url))
            .send().await?.error_for_status()?.json().await?;
        ensure_success(&resp.state, resp.code, &resp.msg)?;
        let mut out = Vec::new();
        for n in resp.data.unwrap_or_default() {
            let detail = if self.configured() { self.node_info(&n.name).await.ok() } else { None };
            out.push(NodeSummary {
                id:n.id, name:n.name.clone(), area:n.area,
                web_supported:n.web.eq_ignore_ascii_case("yes"), state:detail.as_ref().map(|d| d.state.clone()).unwrap_or_else(|| "unknown".into()),
                real_ip:detail.as_ref().map(|d| d.real_ip.clone()), host:detail.as_ref().map(|d| d.ip.clone()),
                load1:detail.as_ref().map(|d| d.load1), bandwidth_usage_percent:detail.as_ref().map(|d| d.bandwidth_usage_percent), quarantined_until:None,
            });
        }
        Ok(out)
    }

    pub async fn node_info(&self, node: &str) -> Result<NodeInfo> {
        self.require_token()?;
        let resp: Envelope<NodeInfo> = self.http.get(format!("{}/nodeinfo", self.base_url))
            .query(&[("token", self.token.as_str()), ("node", node)]).send().await?.error_for_status()?.json().await?;
        ensure_success(&resp.state, resp.code, &resp.msg)?;
        resp.data.ok_or_else(|| anyhow!("ChmlFrp nodeinfo returned no data"))
    }

    pub async fn create_tunnel(&self, plan: &TunnelPlan, target_node: &str) -> Result<RemoteTunnel> {
        self.require_token()?;
        let payload = TunnelMutation::from_plan(&self.token, plan, target_node, 0);
        let resp: Envelope<RemoteTunnel> = self.http.post(format!("{}/create_tunnel", self.base_url))
            .json(&payload).send().await?.error_for_status()?.json().await?;
        ensure_success(&resp.state, resp.code, &resp.msg)?;
        resp.data.ok_or_else(|| anyhow!("ChmlFrp create_tunnel returned no data"))
    }

    /// Reconcile an existing ChmlFrp tunnel to the control-center plan and the
    /// single global target node.  The plan is authoritative for local address,
    /// protocol and domain; only provider-owned transport details are preserved.
    pub async fn sync_tunnel(&self, remote: &RemoteTunnel, plan: &TunnelPlan, target_node: &str) -> Result<()> {
        self.require_token()?;
        let is_web = matches!(plan.protocol.to_ascii_lowercase().as_str(), "http" | "https");
        let payload = TunnelMutation {
            token:self.token.clone(), tunnelname:plan.name.clone(), node:target_node.to_string(),
            porttype:plan.protocol.to_ascii_lowercase(), localip:plan.local_ip.clone(), localport:plan.local_port,
            remoteport:if is_web { 0 } else { remote.remote_port_number() },
            encryption:parse_boolish(&remote.encryption), compression:parse_boolish(&remote.compression),
            extraparams:remote.extra_params.clone().unwrap_or_default(),
            banddomain:if is_web { plan.domain.clone() } else { remote.effective_domain() },
        };
        let resp: Envelope<serde_json::Value> = self.http.post(format!("{}/update_tunnel", self.base_url))
            .form(&payload).send().await?.error_for_status()?.json().await?;
        ensure_success(&resp.state, resp.code, &resp.msg)
            .with_context(|| format!("sync tunnel {} to global node {}", plan.name, target_node))
    }

    pub async fn generated_config(&self, node: &str, tunnel_names: &[String]) -> Result<String> {
        self.require_token()?;
        let names = tunnel_names.join(",");
        let resp: Envelope<String> = self.http.get(format!("{}/tunnel_config", self.base_url))
            .query(&[("token", self.token.as_str()), ("node", node), ("tunnel_names", names.as_str())])
            .send().await?.error_for_status()?.json().await?;
        ensure_success(&resp.state, resp.code, &resp.msg)?;
        resp.data.ok_or_else(|| anyhow!("ChmlFrp tunnel_config returned no config"))
    }

    pub async fn health(&self) -> Result<()> {
        if !self.configured() { return Err(anyhow!("CHMLFRP_TOKEN is not configured")); }
        self.list_tunnels().await.map(|_| ())
    }

    fn require_token(&self) -> Result<()> {
        if self.configured() { Ok(()) } else { Err(anyhow!("CHMLFRP_TOKEN is not configured")) }
    }
}

fn ensure_success(state: &str, code: i64, msg: &str) -> Result<()> {
    if state.eq_ignore_ascii_case("success") || code == 200 { Ok(()) } else { Err(anyhow!("ChmlFrp error code={code} state={state}: {msg}")) }
}
fn parse_boolish(v: &str) -> bool { matches!(v.to_ascii_lowercase().as_str(), "1"|"true"|"yes"|"on") }

#[derive(Debug, Deserialize)]
struct Envelope<T> { msg:String, code:i64, state:String, data:Option<T> }

#[derive(Debug, Clone, Deserialize)]
struct NodeListItem { id:i64, name:String, area:String, web:String }

#[derive(Debug, Clone, Deserialize)]
pub struct NodeInfo {
    #[serde(default)] pub id:i64,
    #[serde(default)] pub name:String,
    #[serde(default)] pub area:String,
    #[serde(default)] pub state:String,
    #[serde(default, rename="realIp")] pub real_ip:String,
    #[serde(default)] pub ip:String,
    #[serde(default)] pub load1:f64,
    #[serde(default)] pub bandwidth_usage_percent:f64,
    #[serde(default)] pub web:String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteTunnel {
    #[serde(default, rename = "tunnelID", alias = "id")]
    pub tunnel_id: i64,
    #[serde(default, rename = "tunnelName", alias = "name")]
    pub tunnel_name: String,
    #[serde(default)]
    pub node: String,
    #[serde(default, rename = "localIP", alias = "localip")]
    pub local_ip: String,
    #[serde(default, rename = "localPort", alias = "nport")]
    pub local_port: i64,
    #[serde(
        default,
        rename = "remotePort",
        alias = "dorp",
        deserialize_with = "deserialize_string_or_number"
    )]
    pub remote_endpoint: String,
    #[serde(default, rename = "portType", alias = "type")]
    pub port_type: String,
    #[serde(default, rename = "bandDomain")]
    pub band_domain: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_bool")]
    pub encryption: String,
    #[serde(default, deserialize_with = "deserialize_string_or_bool")]
    pub compression: String,
    #[serde(default, rename = "extraParams", alias = "ap")]
    pub extra_params: Option<String>,
    #[serde(default, rename = "tunnelState", alias = "state")]
    pub tunnel_state: String,
}

impl RemoteTunnel {
    fn remote_port_number(&self) -> i64 {
        self.remote_endpoint.parse().unwrap_or(0)
    }

    fn effective_domain(&self) -> String {
        self.band_domain.clone().unwrap_or_else(|| {
            if matches!(self.port_type.to_ascii_lowercase().as_str(), "http" | "https") {
                self.remote_endpoint.clone()
            } else {
                String::new()
            }
        })
    }
}

fn deserialize_string_or_number<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(value) => value,
        serde_json::Value::Number(value) => value.to_string(),
        other => other.to_string(),
    })
}

fn deserialize_string_or_bool<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(value) => value,
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        other => other.to_string(),
    })
}

#[derive(Debug, Serialize)]
struct TunnelMutation {
    token:String,
    tunnelname:String,
    node:String,
    porttype:String,
    localip:String,
    localport:i64,
    remoteport:i64,
    encryption:bool,
    compression:bool,
    extraparams:String,
    banddomain:String,
}
impl TunnelMutation {
    fn from_plan(token:&str, plan:&TunnelPlan, node:&str, remote_port:i64) -> Self {
        Self { token:token.into(), tunnelname:plan.name.clone(), node:node.into(), porttype:plan.protocol.to_ascii_lowercase(),
            localip:plan.local_ip.clone(), localport:plan.local_port, remoteport:remote_port, encryption:false, compression:true,
            extraparams:String::new(), banddomain:plan.domain.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::RemoteTunnel;

    #[test]
    fn parses_current_documented_list_shape() {
        let tunnel: RemoteTunnel = serde_json::from_value(serde_json::json!({
            "id": 17,
            "name": "web",
            "node": "node-a",
            "type": "http",
            "localip": "192.168.8.11",
            "nport": 3001,
            "dorp": "api.example.com",
            "encryption": "false",
            "compression": "true",
            "ap": "",
            "state": "true"
        })).unwrap();
        assert_eq!(tunnel.tunnel_name, "web");
        assert_eq!(tunnel.local_port, 3001);
        assert_eq!(tunnel.effective_domain(), "api.example.com");
    }

    #[test]
    fn parses_legacy_camel_case_shape() {
        let tunnel: RemoteTunnel = serde_json::from_value(serde_json::json!({
            "tunnelID": 17,
            "tunnelName": "tcp-a",
            "node": "node-a",
            "portType": "tcp",
            "localIP": "127.0.0.1",
            "localPort": 8080,
            "remotePort": 12345,
            "encryption": false,
            "compression": true,
            "tunnelState": "false"
        })).unwrap();
        assert_eq!(tunnel.remote_port_number(), 12345);
        assert_eq!(tunnel.encryption, "false");
    }
}
