use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
struct CloudflareConfig {
    base_url: String,
    token: String,
    zone_id: String,
}

#[derive(Clone)]
pub struct CloudflareClient {
    http: Client,
    config: Arc<RwLock<CloudflareConfig>>,
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
            config: Arc::new(RwLock::new(CloudflareConfig {
                base_url: normalize_base_url(base_url.into()),
                token: token.into(),
                zone_id: zone_id.into(),
            })),
        })
    }

    fn snapshot(&self) -> CloudflareConfig {
        self.config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn reconfigure(
        &self,
        base_url: impl Into<String>,
        token: impl Into<String>,
        zone_id: impl Into<String>,
    ) {
        let mut config = self
            .config
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        config.base_url = normalize_base_url(base_url.into());
        config.token = token.into();
        config.zone_id = zone_id.into();
    }

    pub fn configured(&self) -> bool {
        let config = self.snapshot();
        !config.token.trim().is_empty() && !config.zone_id.trim().is_empty()
    }

    pub fn token_configured(&self) -> bool {
        !self.snapshot().token.trim().is_empty()
    }

    pub async fn list_a_records(&self) -> Result<Vec<DnsRecord>> {
        let config = self.require_config()?;
        let mut page = 1_u64;
        let mut records = Vec::new();

        loop {
            let page_value = page.to_string();
            let response: CfEnvelope<Vec<DnsRecord>> = self
                .http
                .get(format!(
                    "{}/zones/{}/dns_records",
                    config.base_url, config.zone_id
                ))
                .bearer_auth(&config.token)
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
        let config = self.require_config()?;
        let request = if let Some(record) = existing {
            // Failover changes only the address. PATCHing only `content` preserves
            // TTL, proxied/orange-cloud mode, comments and other record settings.
            self.http
                .patch(format!(
                    "{}/zones/{}/dns_records/{}",
                    config.base_url, config.zone_id, record.id
                ))
                .bearer_auth(&config.token)
                .json(&DnsPatch { content: ip })
        } else {
            self.http
                .post(format!(
                    "{}/zones/{}/dns_records",
                    config.base_url, config.zone_id
                ))
                .bearer_auth(&config.token)
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

    pub async fn verify_token(&self) -> Result<String> {
        let config = self.require_token()?;
        let response: CfEnvelope<TokenVerifyResult> = self
            .http
            .get(format!("{}/user/tokens/verify", config.base_url))
            .bearer_auth(&config.token)
            .send()
            .await
            .context("verify Cloudflare API token")?
            .error_for_status()
            .context("Cloudflare token verify HTTP status")?
            .json()
            .await
            .context("decode Cloudflare token verify response")?;
        let status = ensure_success(response)?.status;
        if status.eq_ignore_ascii_case("active") {
            Ok(status)
        } else {
            Err(anyhow!("Cloudflare API token status is {status}"))
        }
    }

    pub async fn list_zones(&self) -> Result<Vec<CloudflareZone>> {
        let config = self.require_token()?;
        let mut page = 1_u64;
        let mut zones = Vec::new();
        loop {
            let page_value = page.to_string();
            let response: CfEnvelope<Vec<CloudflareZone>> = self
                .http
                .get(format!("{}/zones", config.base_url))
                .bearer_auth(&config.token)
                .query(&[("per_page", "50"), ("page", page_value.as_str())])
                .send()
                .await
                .with_context(|| format!("list Cloudflare zones page {page}"))?
                .error_for_status()
                .with_context(|| format!("Cloudflare zone-list HTTP status page {page}"))?
                .json()
                .await
                .with_context(|| format!("decode Cloudflare zone-list response page {page}"))?;
            let total_pages = response
                .result_info
                .as_ref()
                .and_then(|info| info.total_pages)
                .unwrap_or(page);
            let mut current = ensure_success(response)?;
            zones.append(&mut current);
            if page >= total_pages {
                break;
            }
            page += 1;
        }
        Ok(zones)
    }

    pub async fn health(&self) -> Result<()> {
        self.verify_token().await?;
        self.list_a_records().await.map(|_| ())
    }

    fn require_token(&self) -> Result<CloudflareConfig> {
        let config = self.snapshot();
        if !config.token.trim().is_empty() {
            Ok(config)
        } else {
            Err(anyhow!("Cloudflare API token is not configured"))
        }
    }

    fn require_config(&self) -> Result<CloudflareConfig> {
        let config = self.require_token()?;
        if !config.zone_id.trim().is_empty() {
            Ok(config)
        } else {
            Err(anyhow!("Cloudflare zone id is not configured"))
        }
    }
}

fn normalize_base_url(value: String) -> String {
    value.trim().trim_end_matches('/').to_owned()
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
pub struct CloudflareZone {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Deserialize)]
struct TokenVerifyResult {
    status: String,
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
