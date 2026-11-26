use anyhow::{Context, Result, anyhow, bail};
use reqwest::{Client, Url, header::LOCATION, redirect::Policy};
use serde::Deserialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const QZHUA_WEB_ORIGIN: &str = "https://account.qzhua.net";
const SSO_TIMEOUT: Duration = Duration::from_secs(20);

/// Token bundle obtained from ChmlFrp SSO login or `/sso/refresh`.
#[derive(Debug, Clone)]
pub struct SsoTokenBundle {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: i64,
    pub expires_at_ms: i64,
}

#[derive(Debug, Deserialize)]
struct QzhuaEnvelope<T> {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    message: String,
    data: Option<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QzhuaCsrf {
    token: String,
    parameter_name: String,
    header_name: String,
    #[serde(default)]
    login_action: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MfaStatusData {
    #[serde(default)]
    verified: bool,
    #[serde(default)]
    enabled_methods: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct MfaVerifyData {
    #[serde(default, rename = "continue")]
    continue_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SsoRefreshResponse {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    message: String,
    data: Option<SsoRefreshData>,
}

#[derive(Debug, Deserialize)]
struct SsoRefreshData {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    expires_at: Option<i64>,
}

pub fn now_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Refresh ChmlFrp SSO `access_token` using `POST {base_url}/sso/refresh`.
pub async fn refresh_sso_token(
    http: &Client,
    base_url: &str,
    refresh_token: &str,
) -> Result<SsoTokenBundle> {
    let trimmed = refresh_token.trim();
    if trimmed.is_empty() {
        bail!("refresh_token is empty");
    }

    let url = format!("{}/sso/refresh", base_url.trim_end_matches('/'));
    let resp = http
        .post(&url)
        .json(&serde_json::json!({ "refresh_token": trimmed }))
        .send()
        .await
        .context("request ChmlFrp /sso/refresh")?;

    let status = resp.status();
    let parsed: SsoRefreshResponse = resp
        .json()
        .await
        .context("decode ChmlFrp /sso/refresh response")?;

    if !status.is_success() || !parsed.success {
        bail!("ChmlFrp /sso/refresh failed ({status}): {}", parsed.message);
    }

    let data = parsed
        .data
        .ok_or_else(|| anyhow!("ChmlFrp /sso/refresh returned no data"))?;
    if data.access_token.trim().is_empty() {
        bail!("ChmlFrp /sso/refresh returned empty access_token");
    }

    let expires_in = data.expires_in.unwrap_or(599);
    let expires_at_ms = data
        .expires_at
        .filter(|v| *v > 0)
        .unwrap_or_else(|| now_epoch_ms() + expires_in * 1000);

    Ok(SsoTokenBundle {
        access_token: data.access_token,
        refresh_token: data
            .refresh_token
            .filter(|v| !v.trim().is_empty())
            .or_else(|| Some(trimmed.to_owned())),
        expires_in,
        expires_at_ms,
    })
}

/// Perform full ChmlFrp SSO login via Qzhua Account Center (`account-api.qzhua.net`),
/// automatically completing TOTP 2FA verification when required.
pub async fn login_via_qzhua_sso(
    base_url: &str,
    username: &str,
    password: &str,
    totp_secret: Option<&str>,
) -> Result<SsoTokenBundle> {
    let user = username.trim();
    if user.is_empty() || password.is_empty() {
        bail!("ChmlFrp 用户名和密码不能为空");
    }

    let http = Client::builder()
        .cookie_store(true)
        .redirect(Policy::none())
        .timeout(SSO_TIMEOUT)
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()
        .context("build Qzhua SSO HTTP client")?;

    // 1. Start ChmlFrp SSO authorize flow -> sets server_name_session cookie & redirects to Qzhua OAuth2.
    let sso_auth_url = format!(
        "{}/sso/authorize?return_url=https://panel.chmlfrp.net/home",
        base_url.trim_end_matches('/')
    );
    let resp1 = http
        .get(&sso_auth_url)
        .send()
        .await
        .context("GET /sso/authorize")?;
    let oauth_url = redirect_location(&resp1).ok_or_else(|| {
        anyhow!(
            "ChmlFrp /sso/authorize 未返回 OAuth2 重定向 ({})",
            resp1.status()
        )
    })?;

    // 2. Follow OAuth2 authorize -> redirects to /login?continue=... and sets AS_SESSION cookie.
    let resp2 = http
        .get(&oauth_url)
        .send()
        .await
        .context("GET Qzhua oauth2/authorize")?;
    let loc2 = redirect_location(&resp2).unwrap_or_default();
    if loc2.contains("/sso/callback") {
        return finish_sso_callback(&http, &loc2).await;
    }

    let auth_origin =
        origin_of(&oauth_url).unwrap_or_else(|| "https://account-api.qzhua.net".into());
    let continue_param = Url::parse(&loc2).ok().and_then(|u| {
        u.query_pairs()
            .find(|(k, _)| k == "continue")
            .map(|(_, v)| v.into_owned())
    });

    // 3. Fetch CSRF token.
    let csrf = fetch_qzhua_csrf(&http, &auth_origin).await?;
    let login_action = csrf
        .login_action
        .unwrap_or_else(|| format!("{auth_origin}/login"));

    // 4. Submit login form.
    let mut form_pairs: Vec<(String, String)> = vec![
        (csrf.parameter_name, csrf.token.clone()),
        ("username".into(), user.to_owned()),
        ("password".into(), password.to_owned()),
        (
            "deviceFingerprint".into(),
            "ashan-frp-control-center".into(),
        ),
    ];
    if let Some(cont) = continue_param {
        form_pairs.push(("continue".into(), cont));
    }

    let resp4 = http
        .post(&login_action)
        .header("Origin", QZHUA_WEB_ORIGIN)
        .header("Referer", format!("{QZHUA_WEB_ORIGIN}/login"))
        .header(&csrf.header_name, &csrf.token)
        .form(&form_pairs)
        .send()
        .await
        .context("POST Qzhua /login")?;

    let status4 = resp4.status();
    let loc4 = redirect_location(&resp4).unwrap_or_default();
    if status4.as_u16() >= 400 {
        let body = resp4.text().await.unwrap_or_default();
        bail!("轻爪账号登录被拒绝 ({status4}): {body}");
    }
    if loc4.contains("error") {
        bail!("轻爪账号或密码错误 (redirect: {loc4})");
    }

    // 5. Handle MFA (TOTP) if redirected to /mfa or MFA status reports unverified.
    let next_url = if loc4.contains("/mfa") {
        verify_qzhua_totp(&http, &auth_origin, totp_secret).await?
    } else if loc4.contains("/oauth2/authorize") || loc4.contains("/sso/callback") {
        loc4
    } else {
        // Double-check MFA status explicitly in case redirect omits /mfa.
        let mfa_resp = http
            .get(format!("{auth_origin}/api/auth/mfa/status"))
            .header("Origin", QZHUA_WEB_ORIGIN)
            .send()
            .await
            .context("GET Qzhua /api/auth/mfa/status")?;
        if mfa_resp.status().is_success() {
            let env: QzhuaEnvelope<MfaStatusData> =
                mfa_resp.json().await.context("decode Qzhua mfa/status")?;
            if env
                .data
                .is_some_and(|d| !d.verified && !d.enabled_methods.is_empty())
            {
                verify_qzhua_totp(&http, &auth_origin, totp_secret).await?
            } else {
                oauth_url.clone()
            }
        } else {
            oauth_url.clone()
        }
    };

    // 6. Follow oauth2/authorize -> sso/callback -> panel.chmlfrp.net/home#access_token=...
    if next_url.contains("/sso/callback") {
        return finish_sso_callback(&http, &next_url).await;
    }

    let resp7 = http
        .get(&next_url)
        .send()
        .await
        .context("follow OAuth2 authorize after MFA")?;
    let callback_url = redirect_location(&resp7).ok_or_else(|| {
        anyhow!(
            "OAuth2 authorize 未重定向到 ChmlFrp callback (status={}, url={next_url})",
            resp7.status()
        )
    })?;

    if !callback_url.contains("/sso/callback") {
        bail!("OAuth2 重定向目标异常: {callback_url}");
    }

    finish_sso_callback(&http, &callback_url).await
}

async fn finish_sso_callback(http: &Client, callback_url: &str) -> Result<SsoTokenBundle> {
    let resp = http
        .get(callback_url)
        .send()
        .await
        .context("GET ChmlFrp /sso/callback")?;
    let final_loc = redirect_location(&resp).ok_or_else(|| {
        anyhow!(
            "ChmlFrp /sso/callback 未返回包含 Token 的重定向 ({})",
            resp.status()
        )
    })?;
    parse_sso_redirect_tokens(&final_loc)
}

async fn verify_qzhua_totp(
    http: &Client,
    auth_origin: &str,
    totp_secret: Option<&str>,
) -> Result<String> {
    let secret = totp_secret
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("轻爪账号已开启 2FA，请先配置 TOTP 密钥"))?;

    // Try current 30s window first, then +/-1 window in case of boundary clock skew.
    let mut last_err = String::new();
    for offset in [0_i64, -1, 1] {
        let csrf = fetch_qzhua_csrf(http, auth_origin).await?;
        let code = generate_totp(secret, offset)?;
        let resp = http
            .post(format!("{auth_origin}/api/auth/mfa/verify"))
            .header("Origin", QZHUA_WEB_ORIGIN)
            .header("Referer", format!("{QZHUA_WEB_ORIGIN}/mfa"))
            .header(&csrf.header_name, &csrf.token)
            .json(&serde_json::json!({
                "method": "totp",
                "code": code,
            }))
            .send()
            .await
            .context("POST Qzhua /api/auth/mfa/verify")?;

        let status = resp.status();
        let env: QzhuaEnvelope<MfaVerifyData> = resp
            .json()
            .await
            .context("decode Qzhua mfa/verify response")?;
        if status.is_success() && env.success {
            let cont = env
                .data
                .and_then(|d| d.continue_url)
                .filter(|u| !u.trim().is_empty())
                .ok_or_else(|| anyhow!("MFA 验证成功但未返回 continue URL"))?;
            return Ok(cont);
        }
        last_err = format!("{} ({status})", env.message);
    }

    bail!("轻爪 TOTP 2FA 验证失败: {last_err}")
}

async fn fetch_qzhua_csrf(http: &Client, auth_origin: &str) -> Result<QzhuaCsrf> {
    let resp = http
        .get(format!("{auth_origin}/api/auth/csrf"))
        .header("Origin", QZHUA_WEB_ORIGIN)
        .header("Referer", format!("{QZHUA_WEB_ORIGIN}/login"))
        .send()
        .await
        .context("GET Qzhua /api/auth/csrf")?;

    let status = resp.status();
    let env: QzhuaEnvelope<QzhuaCsrf> = resp.json().await.context("decode Qzhua /api/auth/csrf")?;
    if !status.is_success() || !env.success {
        bail!("获取轻爪 CSRF 失败 ({status}): {}", env.message);
    }
    env.data
        .ok_or_else(|| anyhow!("轻爪 /api/auth/csrf 未返回 data"))
}

fn redirect_location(resp: &reqwest::Response) -> Option<String> {
    resp.headers()
        .get(LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

fn origin_of(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    match parsed.port() {
        Some(port) => Some(format!("{}://{host}:{port}", parsed.scheme())),
        None => Some(format!("{}://{host}", parsed.scheme())),
    }
}

/// Extract `access_token`, `refresh_token`, `expires_in`, and `expires_at` from
/// the ChmlFrp `/sso/callback` redirect target fragment (or query fallback).
pub fn parse_sso_redirect_tokens(redirect_url: &str) -> Result<SsoTokenBundle> {
    let parsed = Url::parse(redirect_url).context("parse SSO callback redirect URL")?;
    let mut access_token = None;
    let mut refresh_token = None;
    let mut expires_in = 599_i64;
    let mut expires_at_ms = 0_i64;

    let parse_pairs = |raw: &str,
                       at: &mut Option<String>,
                       rt: &mut Option<String>,
                       ei: &mut i64,
                       ea: &mut i64| {
        for (k, v) in url_form_pairs(raw) {
            match k.as_str() {
                "access_token" if !v.is_empty() => *at = Some(v),
                "refresh_token" if !v.is_empty() => *rt = Some(v),
                "expires_in" => {
                    if let Ok(n) = v.parse::<i64>() {
                        *ei = n;
                    }
                }
                "expires_at" => {
                    if let Ok(n) = v.parse::<i64>() {
                        *ea = n;
                    }
                }
                _ => {}
            }
        }
    };

    if let Some(frag) = parsed.fragment() {
        parse_pairs(
            frag,
            &mut access_token,
            &mut refresh_token,
            &mut expires_in,
            &mut expires_at_ms,
        );
    }
    if access_token.is_none()
        && let Some(query) = parsed.query()
    {
        parse_pairs(
            query,
            &mut access_token,
            &mut refresh_token,
            &mut expires_in,
            &mut expires_at_ms,
        );
    }

    let access_token =
        access_token.ok_or_else(|| anyhow!("SSO 回调链接中未包含 access_token: {redirect_url}"))?;
    if expires_at_ms <= 0 {
        expires_at_ms = now_epoch_ms() + expires_in * 1000;
    }

    Ok(SsoTokenBundle {
        access_token,
        refresh_token,
        expires_in,
        expires_at_ms,
    })
}

fn url_form_pairs(input: &str) -> Vec<(String, String)> {
    // Parse via dummy URL query so percent-encoding is decoded accurately without adding url crate directly.
    Url::parse(&format!("http://localhost/?{input}"))
        .map(|u| {
            u.query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect()
        })
        .unwrap_or_default()
}

/// Generate a 6-digit RFC 6238 TOTP code from a Base32 secret.
pub fn generate_totp(secret_b32: &str, step_offset: i64) -> Result<String> {
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let step = (now_secs / 30).saturating_add(step_offset) as u64;
    generate_totp_for_counter(secret_b32, step)
}

pub fn generate_totp_for_counter(secret_b32: &str, counter: u64) -> Result<String> {
    let key = decode_base32(secret_b32)?;
    let msg = counter.to_be_bytes();
    let digest = hmac_sha1(&key, &msg);
    let offset = (digest[19] & 0x0f) as usize;
    let bin_code = ((digest[offset] as u32 & 0x7f) << 24)
        | ((digest[offset + 1] as u32) << 16)
        | ((digest[offset + 2] as u32) << 8)
        | (digest[offset + 3] as u32);
    Ok(format!("{:06}", bin_code % 1_000_000))
}

fn decode_base32(input: &str) -> Result<Vec<u8>> {
    let cleaned: String = input
        .chars()
        .filter(|c| !c.is_ascii_whitespace() && *c != '=')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if cleaned.is_empty() {
        bail!("TOTP 密钥为空");
    }

    let mut out = Vec::with_capacity(cleaned.len() * 5 / 8);
    let mut buffer: u32 = 0;
    let mut bits_left: u8 = 0;

    for ch in cleaned.bytes() {
        let val = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'2'..=b'7' => ch - b'2' + 26,
            _ => bail!("TOTP Base32 密钥包含非法字符: {}", ch as char),
        };
        buffer = (buffer << 5) | val as u32;
        bits_left += 5;
        if bits_left >= 8 {
            bits_left -= 8;
            out.push(((buffer >> bits_left) & 0xff) as u8);
        }
    }
    Ok(out)
}

fn hmac_sha1(key: &[u8], msg: &[u8]) -> [u8; 20] {
    const BLOCK_SIZE: usize = 64;
    let mut k_block = [0_u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let hashed = sha1(key);
        k_block[..20].copy_from_slice(&hashed);
    } else {
        k_block[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36_u8; BLOCK_SIZE];
    let mut opad = [0x5c_u8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        ipad[i] ^= k_block[i];
        opad[i] ^= k_block[i];
    }

    let mut inner = Vec::with_capacity(BLOCK_SIZE + msg.len());
    inner.extend_from_slice(&ipad);
    inner.extend_from_slice(msg);
    let inner_hash = sha1(&inner);

    let mut outer = Vec::with_capacity(BLOCK_SIZE + 20);
    outer.extend_from_slice(&opad);
    outer.extend_from_slice(&inner_hash);
    sha1(&outer)
}

#[allow(clippy::needless_range_loop)]
fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x6745_2301;
    let mut h1: u32 = 0xefcd_ab89;
    let mut h2: u32 = 0x98ba_dcfe;
    let mut h3: u32 = 0x1032_5476;
    let mut h4: u32 = 0xc3d2_e1f0;

    let bit_len = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut w = [0_u32; 80];
        for i in 0..16 {
            let j = i * 4;
            w[i] = u32::from_be_bytes([chunk[j], chunk[j + 1], chunk[j + 2], chunk[j + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);
        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5a82_7999_u32),
                20..=39 => (b ^ c ^ d, 0x6ed9_eba1_u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc_u32),
                _ => (b ^ c ^ d, 0xca62_c1d6_u32),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0_u8; 20];
    out[0..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6238_sha1_test_vectors() {
        // RFC 6238 Appendix B SHA1 secret "12345678901234567890" -> Base32 "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"
        let secret = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
        // t=59 => counter=1 => 94287082 mod 1_000_000 = 287082
        assert_eq!(generate_totp_for_counter(secret, 1).unwrap(), "287082");
        // t=1111111109 => counter=37037036 => 07081804 mod 1_000_000 = 081804
        assert_eq!(
            generate_totp_for_counter(secret, 37_037_036).unwrap(),
            "081804"
        );
    }

    #[test]
    fn parses_sso_fragment_tokens() {
        let url = "https://panel.chmlfrp.net/home#access_token=jwt_abc&refresh_token=rt_xyz&expires_in=599&expires_at=1795710439303";
        let bundle = parse_sso_redirect_tokens(url).unwrap();
        assert_eq!(bundle.access_token, "jwt_abc");
        assert_eq!(bundle.refresh_token.as_deref(), Some("rt_xyz"));
        assert_eq!(bundle.expires_in, 599);
        assert_eq!(bundle.expires_at_ms, 1795710439303);
    }
}
