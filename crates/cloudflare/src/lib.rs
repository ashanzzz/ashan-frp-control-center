use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct CloudflareClient {
    http: Client,
    base_url: String,
    token: String,
    zone_id: String,
}

impl CloudflareClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>, zone_id: impl Into<String>) -> Self {
        Self { http:Client::new(), base_url:base_url.into().trim_end_matches('/').into(), token:token.into(), zone_id:zone_id.into() }
    }
    pub fn configured(&self) -> bool { !self.token.trim().is_empty() && !self.zone_id.trim().is_empty() }

    pub async fn list_a_records(&self) -> Result<Vec<DnsRecord>> {
        self.require_config()?;
        let resp: CfEnvelope<Vec<DnsRecord>> = self.http.get(format!("{}/zones/{}/dns_records", self.base_url, self.zone_id))
            .bearer_auth(&self.token).query(&[("type","A"),("per_page","500")]).send().await?.error_for_status()?.json().await?;
        ensure_success(resp)
    }

    pub async fn upsert_a_record(&self, name: &str, ip: &str, existing: Option<&DnsRecord>) -> Result<DnsRecord> {
        self.require_config()?;
        let response = if let Some(record) = existing {
            // A failover changes only the address. PATCHing just `content` preserves
            // TTL, Proxied/orange-cloud mode, comments and every other record setting.
            self.http
                .patch(format!("{}/zones/{}/dns_records/{}", self.base_url, self.zone_id, record.id))
                .bearer_auth(&self.token)
                .json(&DnsPatch { content: ip })
                .send()
                .await?
        } else {
            self.http
                .post(format!("{}/zones/{}/dns_records", self.base_url, self.zone_id))
                .bearer_auth(&self.token)
                .json(&DnsMutation {
                    record_type: "A",
                    name,
                    content: ip,
                    ttl: 1,
                    proxied: false,
                    comment: Some("Managed by Ashan FRP Control Center"),
                })
                .send()
                .await?
        };
        let resp: CfEnvelope<DnsRecord> = response.error_for_status()?.json().await?;
        ensure_success(resp)
    }

    pub async fn health(&self) -> Result<()> { self.list_a_records().await.map(|_| ()) }
    fn require_config(&self) -> Result<()> { if self.configured(){Ok(())}else{Err(anyhow!("Cloudflare token or zone id not configured"))} }
}

fn ensure_success<T>(resp:CfEnvelope<T>) -> Result<T> {
    if resp.success { resp.result.ok_or_else(|| anyhow!("Cloudflare success response missing result")) }
    else { Err(anyhow!("Cloudflare API error: {}", serde_json::to_string(&resp.errors).unwrap_or_default())) }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsRecord {
    pub id:String,
    pub name:String,
    #[serde(rename="type")] pub record_type:String,
    pub content:String,
    #[serde(default)] pub proxied:bool,
    #[serde(default)] pub ttl:i64,
}

#[derive(Debug, Serialize)]
struct DnsPatch<'a> {
    content: &'a str,
}

#[derive(Debug, Serialize)]
struct DnsMutation<'a> {
    #[serde(rename="type")] record_type:&'a str,
    name:&'a str,
    content:&'a str,
    ttl:i64,
    proxied:bool,
    comment:Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct CfEnvelope<T> {
    success:bool,
    result:Option<T>,
    #[serde(default)] errors:Vec<serde_json::Value>,
}
