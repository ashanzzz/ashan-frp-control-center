use anyhow::{Context, Result, anyhow, bail};
use regex::Regex;
use reqwest::Url;
use rustls_pki_types::ServerName;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls;

#[derive(Debug, Clone)]
pub struct ImapConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub proxy: Option<String>,
    pub sender_domain: String,
    pub max_wait_secs: u64,
    pub poll_interval_secs: u64,
}

impl ImapConfig {
    pub fn from_env() -> Result<Option<Self>> {
        let host = match std::env::var("IMAP_HOST") {
            Ok(v) if !v.trim().is_empty() => v.trim().to_owned(),
            _ => return Ok(None),
        };
        let username = std::env::var("IMAP_USERNAME")
            .context("IMAP_USERNAME is required when IMAP_HOST is set")?
            .trim()
            .to_owned();
        let password = std::env::var("IMAP_PASSWORD")
            .context("IMAP_PASSWORD is required when IMAP_HOST is set")?
            .trim()
            .to_owned();
        let port = std::env::var("IMAP_PORT")
            .unwrap_or_else(|_| "993".into())
            .parse::<u16>()
            .context("IMAP_PORT parse error")?;
        let proxy = std::env::var("IMAP_PROXY")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(|v| v.trim().to_owned());
        let sender_domain = std::env::var("EMAIL_VERIFICATION_SENDER_DOMAIN")
            .unwrap_or_else(|_| "chmlfrp.cn".into())
            .trim()
            .to_owned();
        let max_wait_secs = std::env::var("EMAIL_VERIFICATION_MAX_WAIT_SECS")
            .unwrap_or_else(|_| "120".into())
            .parse::<u64>()
            .unwrap_or(120);
        let poll_interval_secs = std::env::var("EMAIL_VERIFICATION_POLL_INTERVAL_SECS")
            .unwrap_or_else(|_| "5".into())
            .parse::<u64>()
            .unwrap_or(5);

        Ok(Some(Self {
            host,
            port,
            username,
            password,
            proxy,
            sender_domain,
            max_wait_secs,
            poll_interval_secs,
        }))
    }
}

pub struct EmailVerificationFetcher {
    config: ImapConfig,
}

impl EmailVerificationFetcher {
    pub fn new(config: ImapConfig) -> Self {
        Self { config }
    }

    pub async fn fetch_latest_code(&self) -> Result<String> {
        let start = Instant::now();
        let timeout = Duration::from_secs(self.config.max_wait_secs);
        let poll_interval = Duration::from_secs(self.config.poll_interval_secs.max(2));

        while start.elapsed() < timeout {
            match self.fetch_single_attempt().await {
                Ok(Some(code)) => return Ok(code),
                Ok(None) => {}
                Err(err) => {
                    tracing::warn!("IMAP fetch verification code attempt failed: {err:#}");
                }
            }
            tokio::time::sleep(poll_interval).await;
        }

        bail!(
            "Timed out after {}s waiting for verification code from {}",
            self.config.max_wait_secs,
            self.config.sender_domain
        )
    }

    async fn fetch_single_attempt(&self) -> Result<Option<String>> {
        let mut stream = self
            .connect_tls()
            .await
            .context("connect IMAP TLS stream")?;
        let mut session = ImapWireSession::new(&mut stream);

        session
            .read_response_line()
            .await
            .context("read greeting")?;

        let user = self.config.username.replace('"', "");
        let pass = self.config.password.replace('"', "");
        session
            .send_command("A01", &format!("LOGIN \"{user}\" \"{pass}\""))
            .await
            .context("IMAP LOGIN command")?;

        session
            .send_command("A02", "SELECT INBOX")
            .await
            .context("IMAP SELECT INBOX command")?;

        let search_query = format!("SEARCH FROM \"{}\"", self.config.sender_domain);
        let search_resp = session
            .send_command("A03", &search_query)
            .await
            .context("IMAP SEARCH command")?;

        let ids = parse_search_ids(&search_resp);
        let latest_id = match ids.into_iter().max() {
            Some(id) => id,
            None => {
                let _ = session.send_command("A99", "LOGOUT").await;
                return Ok(None);
            }
        };

        let fetch_cmd = format!("FETCH {latest_id} (BODY[TEXT])");
        let fetch_resp = session
            .send_command("A04", &fetch_cmd)
            .await
            .context("IMAP FETCH body command")?;

        let code = extract_verification_code(&fetch_resp);
        let _ = session.send_command("A99", "LOGOUT").await;

        Ok(code)
    }

    async fn connect_tls(&self) -> Result<TlsStream<TcpStream>> {
        let raw_stream = if let Some(ref proxy_url) = self.config.proxy {
            connect_via_http_proxy(proxy_url, &self.config.host, self.config.port).await?
        } else {
            let addr = format!("{}:{}", self.config.host, self.config.port);
            TcpStream::connect(&addr)
                .await
                .with_context(|| format!("connect TCP {addr}"))?
        };

        let mut root_cert_store = rustls::RootCertStore::empty();
        for cert in webpki_root_certs::TLS_SERVER_ROOT_CERTS {
            let _ = root_cert_store.add(cert.clone());
        }

        let client_config = rustls::ClientConfig::builder()
            .with_root_certificates(root_cert_store)
            .with_no_client_auth();

        let connector = TlsConnector::from(Arc::new(client_config));
        let server_name = ServerName::try_from(self.config.host.clone())
            .map_err(|e| anyhow!("invalid server name {}: {e}", self.config.host))?;

        let tls_stream = connector
            .connect(server_name, raw_stream)
            .await
            .with_context(|| {
                format!(
                    "TLS handshake with {}:{}",
                    self.config.host, self.config.port
                )
            })?;

        Ok(tls_stream)
    }
}

async fn connect_via_http_proxy(
    proxy_url: &str,
    target_host: &str,
    target_port: u16,
) -> Result<TcpStream> {
    let parsed = Url::parse(proxy_url).context("parse IMAP_PROXY URL")?;
    let proxy_host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("missing proxy host"))?;
    let proxy_port = parsed.port_or_known_default().unwrap_or(7890);
    let proxy_addr = format!("{proxy_host}:{proxy_port}");

    let mut stream = TcpStream::connect(&proxy_addr)
        .await
        .with_context(|| format!("connect HTTP proxy {proxy_addr}"))?;

    let req = format!(
        "CONNECT {target_host}:{target_port} HTTP/1.1\r\nHost: {target_host}:{target_port}\r\nProxy-Connection: Keep-Alive\r\n\r\n"
    );
    stream
        .write_all(req.as_bytes())
        .await
        .context("send HTTP CONNECT")?;

    let mut buf = [0u8; 1024];
    let mut total_read = 0;
    while total_read < buf.len() {
        let n = stream
            .read(&mut buf[total_read..])
            .await
            .context("read proxy response")?;
        if n == 0 {
            bail!("proxy closed connection during CONNECT");
        }
        total_read += n;
        let response_str = String::from_utf8_lossy(&buf[..total_read]);
        if response_str.contains("\r\n\r\n") {
            if response_str.starts_with("HTTP/1.1 200") || response_str.starts_with("HTTP/1.0 200")
            {
                return Ok(stream);
            } else {
                bail!("HTTP proxy returned non-200: {response_str}");
            }
        }
    }
    bail!("HTTP proxy response too long")
}

struct ImapWireSession<'a> {
    stream: &'a mut TlsStream<TcpStream>,
}

impl<'a> ImapWireSession<'a> {
    fn new(stream: &'a mut TlsStream<TcpStream>) -> Self {
        Self { stream }
    }

    async fn send_command(&mut self, tag: &str, command: &str) -> Result<String> {
        let line = format!("{tag} {command}\r\n");
        self.stream.write_all(line.as_bytes()).await?;

        let mut accumulator = String::new();
        let target_ok = format!("{tag} OK");
        let target_no = format!("{tag} NO");
        let target_bad = format!("{tag} BAD");

        loop {
            let l = self.read_response_line().await?;
            accumulator.push_str(&l);
            accumulator.push('\n');

            if l.starts_with(&target_ok) {
                return Ok(accumulator);
            }
            if l.starts_with(&target_no) || l.starts_with(&target_bad) {
                bail!("IMAP command failed: {l}");
            }
        }
    }

    async fn read_response_line(&mut self) -> Result<String> {
        let mut line_bytes = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            let n = self.stream.read(&mut byte).await?;
            if n == 0 {
                if line_bytes.is_empty() {
                    bail!("unexpected EOF from IMAP server");
                }
                break;
            }
            if byte[0] == b'\n' {
                break;
            }
            if byte[0] != b'\r' {
                line_bytes.push(byte[0]);
            }
        }
        Ok(String::from_utf8_lossy(&line_bytes).to_string())
    }
}

pub fn parse_search_ids(response: &str) -> Vec<u32> {
    let mut ids = Vec::new();
    for line in response.lines() {
        if line.starts_with("* SEARCH") {
            for part in line.split_whitespace().skip(2) {
                if let Ok(id) = part.parse::<u32>() {
                    ids.push(id);
                }
            }
        }
    }
    ids
}

pub fn extract_verification_code(content: &str) -> Option<String> {
    let re_context = Regex::new(r"(?i)(?:验证码|code|mfa|token)[^\d\r\n]{0,10}(\d{6})").ok()?;
    if let Some(caps) = re_context.captures(content)
        && let Some(m) = caps.get(1)
    {
        return Some(m.as_str().to_string());
    }
    let re_digits = Regex::new(r"\b(\d{6})\b").ok()?;
    re_digits.find(content).map(|m| m.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_verification_code_chinese() {
        let text =
            "【ChmlFrp】您正在申请登录/授权操作，本次验证码为：849201，有效期为5分钟，请勿泄露。";
        assert_eq!(extract_verification_code(text).as_deref(), Some("849201"));
    }

    #[test]
    fn test_extract_verification_code_english() {
        let text =
            "Your verification code for ChmlFrp account is 610293. It expires in 10 minutes.";
        assert_eq!(extract_verification_code(text).as_deref(), Some("610293"));
    }

    #[test]
    fn test_parse_search_ids() {
        let raw = "* SEARCH 1 2 5 12 88\r\nA03 OK SEARCH completed\r\n";
        assert_eq!(parse_search_ids(raw), vec![1, 2, 5, 12, 88]);
    }
}
