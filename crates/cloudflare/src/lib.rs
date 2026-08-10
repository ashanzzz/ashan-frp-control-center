use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct CloudflareClient {
    http: Client,
    base_url: String,
    token: String,
    zone_id: String,
}

impl CloudflareClient {
    pub fn new(
        base_url: impl Into<String>,
        token: impl Into<String>,
        zone_id: impl Into<String>,
    ) -> Result<Self> {
        let http = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("build Cloudflare HTTP client")?;

        Ok(Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            token: token.into(),
            zone_id: zone_id.into(),
        })
    }

    pub fn configured(&self) -> bool {
        !self.token.trim().is_empty() && !self.zone_id.trim().is_empty()
    }

    pub async fn list_a_records(&self) -> Result<Vec<DnsRecord>> {
        self.require_config()?;
        let mut page = 1_u64;
        let mut records = Vec::new();

        loop {
            let page_value = page.to_string();
            let response: CfEnvelope<Vec<DnsRecord>> = self
                .http
                .get(format!(
                    "{}/zones/{}/dns_records",
                    self.base_url, self.zone_id
                ))
                .bearer_auth(&self.token)
                .query(&[
                    ("type", "A"),
                    ("per_page", "500"),
                    ("page", page_value.as_str()),
                ])
                .send()
                .await
                .with_context(|| format!("list Cloudflare A records page {page}"))?
                .error_for_status()
                .with_context(|| format!("Cloudflare A-record HTTP status page {page}"))?
                .json()
                .await
                .with_context(|| format!("decode Cloudflare A-record response page {page}"))?;

            let total_pages = response
                .result_info
                .as_ref()
                .and_then(|info| info.total_pages)
                .unwrap_or(page);
            let mut current = ensure_success(response)?;
            records.append(&mut current);

            if page >= total_pages {
                break;
            }
            page += 1;
        }

        Ok(records)
    }

    pub async fn upsert_a_record(
        &self,
        name: &str,
        ip: &str,
        existing: Option<&DnsRecord>,
    ) -> Result<DnsRecord> {
        self.require_config()?;
        let request = if let Some(record) = existing {
            // Failover changes only the address. PATCHing only `content` preserves
            // TTL, proxied/orange-cloud mode, comments and other record settings.
            self.http
                .patch(format!(
                    "{}/zones/{}/dns_records/{}",
                    self.base_url, self.zone_id, record.id
                ))
                .bearer_auth(&self.token)
                .json(&DnsPatch { content: ip })
        } else {
            self.http
                .post(format!(
                    "{}/zones/{}/dns_records",
                    self.base_url, self.zone_id
                ))
                .bearer_auth(&self.token)
                .json(&DnsMutation {
                    record_type: "A",
                    name,
                    content: ip,
                    ttl: 1,
                    proxied: false,
                    comment: Some("Managed by Ashan FRP Control Center"),
                })
        };

        let response: CfEnvelope<DnsRecord> = request
            .send()
            .await
            .with_context(|| format!("write Cloudflare A record {name}"))?
            .error_for_status()
            .with_context(|| format!("Cloudflare A-record HTTP status for {name}"))?
            .json()
            .await
            .with_context(|| format!("decode Cloudflare A-record response for {name}"))?;
        ensure_success(response)
    }

    pub async fn health(&self) -> Result<()> {
        self.list_a_records().await.map(|_| ())
    }

    fn require_config(&self) -> Result<()> {
        if self.configured() {
            Ok(())
        } else {
            Err(anyhow!("Cloudflare token or zone id not configured"))
        }
    }
}

fn ensure_success<T>(response: CfEnvelope<T>) -> Result<T> {
    if response.success {
        response
            .result
            .ok_or_else(|| anyhow!("Cloudflare success response missing result"))
    } else {
        Err(anyhow!(
            "Cloudflare API error: {}",
            serde_json::to_string(&response.errors).unwrap_or_default()
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsRecord {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub content: String,
    #[serde(default)]
    pub proxied: bool,
    #[serde(default)]
    pub ttl: i64,
}

#[derive(Debug, Serialize)]
struct DnsPatch<'a> {
    content: &'a str,
}

#[derive(Debug, Serialize)]
struct DnsMutation<'a> {
    #[serde(rename = "type")]
    record_type: &'a str,
    name: &'a str,
    content: &'a str,
    ttl: i64,
    proxied: bool,
    comment: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct CfEnvelope<T> {
    success: bool,
    result: Option<T>,
    #[serde(default)]
    errors: Vec<serde_json::Value>,
    #[serde(default)]
    result_info: Option<CfResultInfo>,
}

#[derive(Debug, Deserialize)]
struct CfResultInfo {
    total_pages: Option<u64>,
}
