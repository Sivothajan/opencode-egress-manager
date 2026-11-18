//! Egress chain: proxy pool parsing, current-node selection, and rotation.

use serde::{Deserialize, Serialize};

use crate::{
    config::{Paths, ProxyType},
    errors::{redact, AppError, Result},
    services::networking,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxySpec {
    pub host: String,
    pub port: u16,
    #[serde(rename = "type")]
    pub proxy_type: ProxyType,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
}

impl ProxySpec {
    /// Proxy URL with credentials included. Only for in-process use; never log it.
    pub fn proxy_url(&self) -> String {
        let scheme = match self.proxy_type {
            ProxyType::Socks5 => "socks5h",
            ProxyType::Http => "http",
        };
        if self.username.is_empty() {
            format!("{scheme}://{}:{}", self.host, self.port)
        } else {
            let user = urlencoding(self.username.as_str());
            let pass = urlencoding(self.password.as_str());
            format!("{scheme}://{user}:{pass}@{}:{}", self.host, self.port)
        }
    }

    pub fn describe(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

fn urlencoding(value: &str) -> String {
    percent_encoding::utf8_percent_encode(value, percent_encoding::NON_ALPHANUMERIC).to_string()
}

/// Strip credentials from a proxy string for safe persistence/logging.
pub fn sanitize_proxy_string(raw: &str) -> String {
    raw.lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return String::new();
            }
            if let Some((scheme, rest)) = trimmed.split_once("://") {
                if let Some((_userinfo, hostport)) = rest.rsplit_once('@') {
                    return format!("{scheme}://{hostport}");
                }
            } else if let Some((_userinfo, hostport)) = trimmed.rsplit_once('@') {
                return hostport.to_string();
            }
            trimmed.to_string()
        })
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extract credentials from a proxy string, if present.
pub fn extract_credentials(raw: &str) -> Option<(String, String)> {
    let mut result = None;
    for line in raw.lines() {
        let trimmed = line.trim();
        let text = trimmed.split_once("://").map(|(_, r)| r).unwrap_or(trimmed);
        if let Some((userinfo, _)) = text.rsplit_once('@') {
            let (user, pass) = match userinfo.split_once(':') {
                Some((u, p)) => (urldecode(u), urldecode(p)),
                None => (urldecode(userinfo), String::new()),
            };
            if !user.is_empty() {
                result = Some((user, pass));
            }
        }
    }
    result
}

fn urldecode(value: &str) -> String {
    percent_encoding::percent_decode_str(value)
        .decode_utf8_lossy()
        .to_string()
}

/// Parse a single `[scheme://][user:pass@]host:port` entry.
pub fn parse_proxy_entry(raw: &str, default_type: ProxyType) -> Option<ProxySpec> {
    let text = raw.trim();
    if text.is_empty() || text.len() > 512 {
        return None;
    }
    let (scheme, mut rest) = match text.split_once("://") {
        Some((s, r)) => (Some(s.trim().to_ascii_lowercase()), r.trim().to_string()),
        None => (None, text.to_string()),
    };
    rest = rest.trim_end_matches('/').to_string();
    let mut username = String::new();
    let mut password = String::new();
    if let Some((userinfo, hostport)) = rest.rsplit_once('@') {
        let (u, p) = match userinfo.split_once(':') {
            Some((u, p)) => (u, p),
            None => (userinfo, ""),
        };
        username = urldecode(u);
        password = urldecode(p);
        rest = hostport.to_string();
    }
    let (host, port) = if let Some(stripped) = rest.strip_prefix('[') {
        let end = stripped.find(']')?;
        let host = stripped[..end].to_string();
        let tail = stripped[end + 1..].trim_start_matches(':');
        let port: u16 = tail.parse().ok()?;
        (host, port)
    } else {
        let (phost, pport) = rest.rsplit_once(':')?;
        let port: u16 = pport.parse().ok()?;
        (phost.trim_matches(['[', ']']).to_string(), port)
    };
    if host.is_empty() || port == 0 {
        return None;
    }
    let proxy_type = match scheme.as_deref() {
        Some("socks5") | Some("socks5h") | Some("socks") => ProxyType::Socks5,
        Some("http") | Some("https") => ProxyType::Http,
        Some(_) => default_type,
        None => default_type,
    };
    Some(ProxySpec {
        host,
        port,
        proxy_type,
        username,
        password,
    })
}

pub fn parse_proxy_type(value: &str) -> Result<ProxyType> {
    ProxyType::parse(value)
}

/// Split a comma/semicolon/whitespace/newline delimited proxy pool.
pub fn parse_proxy_specs(raw: &str, default_type: ProxyType) -> Vec<ProxySpec> {
    let entries: Vec<&str> = raw
        .split([',', ';', '\n', '\r', ' ', '\t'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let paths = Paths::discover();
    let mut specs = Vec::new();
    for entry in entries {
        if let Some(mut spec) = parse_proxy_entry(entry, default_type) {
            // Attach DPAPI-protected credentials when they exist for this node.
            if let Some((user, pass)) =
                crate::services::secrets::credentials_for(&paths, &spec.host, spec.port)
            {
                spec.username = user;
                spec.password = pass;
            }
            specs.push(spec);
        }
    }
    if specs.is_empty() {
        specs.push(ProxySpec {
            host: "127.0.0.1".into(),
            port: 10808,
            proxy_type: default_type,
            username: String::new(),
            password: String::new(),
        });
    }
    specs
}

fn index_file(paths: &Paths) -> std::path::PathBuf {
    paths.lane_dir.join("custom_proxy_index.txt")
}

pub fn proxy_index_from_paths(paths: &Paths, total: usize) -> usize {
    let path = index_file(paths);
    let idx: i64 = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    if total == 0 {
        0
    } else {
        (idx.rem_euclid(total as i64)) as usize
    }
}

pub fn write_proxy_index(paths: &Paths, idx: usize) -> Result<()> {
    crate::config::write_text(&index_file(paths), &idx.to_string())
}

pub fn get_current_proxy_spec(
    paths: &Paths,
    cfg: &crate::config::EgressConfig,
) -> Option<ProxySpec> {
    let specs = parse_proxy_specs(&cfg.custom_proxy, cfg.custom_type);
    if specs.is_empty() {
        return None;
    }
    let idx = proxy_index_from_paths(paths, specs.len());
    specs.into_iter().nth(idx)
}

/// Rotate to the next node that answers, verifying the exit address when possible.
pub async fn rotate_custom_proxy(paths: &Paths) -> Result<(bool, String)> {
    let cfg = crate::config::EgressConfig::load(paths)?;
    let specs = parse_proxy_specs(&cfg.custom_proxy, cfg.custom_type);
    if specs.len() <= 1 {
        let spec = &specs[0];
        return Ok((
            false,
            format!(
                "Only one proxy is configured ({}); add another node to rotate exits.",
                spec.describe()
            ),
        ));
    }
    let original = proxy_index_from_paths(paths, specs.len());
    if cfg.mode != crate::config::EgressMode::Custom {
        let new_idx = (original + 1) % specs.len();
        write_proxy_index(paths, new_idx)?;
        let spec = &specs[new_idx];
        return Ok((
            true,
            format!(
                "Switched to proxy #{}/{}: {}",
                new_idx + 1,
                specs.len(),
                spec.describe()
            ),
        ));
    }
    for step in 1..specs.len() {
        let new_idx = (original + step) % specs.len();
        write_proxy_index(paths, new_idx)?;
        let spec = &specs[new_idx];
        if let Some(exit_ip) = networking::probe_public_ip(Some(&spec.proxy_url()), 5).await {
            return Ok((
                true,
                format!(
                    "Switched to proxy #{}/{}: {} (exit {})",
                    new_idx + 1,
                    specs.len(),
                    spec.describe(),
                    exit_ip
                ),
            ));
        }
    }
    write_proxy_index(paths, original)?;
    let spec = &specs[original];
    Ok((
        false,
        format!(
            "None of the other proxy nodes answered; kept {} (#{}/{}).",
            spec.describe(),
            original + 1,
            specs.len()
        ),
    ))
}

/// Move to the next node without a blocking verification (fast request-path use).
pub fn advance_custom_proxy(paths: &Paths) -> Result<ProxySpec> {
    let cfg = crate::config::EgressConfig::load(paths)?;
    let specs = parse_proxy_specs(&cfg.custom_proxy, cfg.custom_type);
    let idx = proxy_index_from_paths(paths, specs.len());
    let new_idx = (idx + 1) % specs.len().max(1);
    write_proxy_index(paths, new_idx)?;
    Ok(specs[new_idx.min(specs.len().saturating_sub(1))].clone())
}

pub async fn open_http_proxy_tunnel(
    dest_host: &str,
    dest_port: u16,
    proxy_host: &str,
    proxy_port: u16,
    timeout: std::time::Duration,
    username: &str,
    password: &str,
) -> Result<tokio::net::TcpStream> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut sock = tokio::time::timeout(
        timeout,
        tokio::net::TcpStream::connect((proxy_host, proxy_port)),
    )
    .await
    .map_err(|_| AppError::Process("upstream HTTP proxy connect timed out".into()))?
    .map_err(|e| AppError::Process(format!("upstream HTTP proxy connect failed: {e}")))?;
    let mut req = format!(
        "CONNECT {dest_host}:{dest_port} HTTP/1.1\r\nHost: {dest_host}:{dest_port}\r\nProxy-Connection: Keep-Alive\r\n"
    );
    if !username.is_empty() {
        use base64::Engine;
        let token = base64::engine::general_purpose::STANDARD
            .encode(format!("{username}:{password}").as_bytes());
        req.push_str(&format!("Proxy-Authorization: Basic {token}\r\n"));
    }
    req.push_str("\r\n");
    sock.write_all(req.as_bytes()).await?;
    let mut resp = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = sock.read(&mut buf).await?;
        if n == 0 {
            return Err(AppError::Process(
                "upstream HTTP proxy closed connection".into(),
            ));
        }
        resp.extend_from_slice(&buf[..n]);
        if resp.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if resp.len() > 65536 {
            return Err(AppError::Process(
                "upstream HTTP proxy reply is too large".into(),
            ));
        }
    }
    let status_line = resp
        .split(|b| *b == b'\r' || *b == b'\n')
        .next()
        .map(|s| String::from_utf8_lossy(s).to_string())
        .unwrap_or_default();
    if !status_line.contains(" 200 ") {
        return Err(AppError::Process(format!(
            "upstream HTTP proxy rejected CONNECT: {}",
            redact(&status_line)
        )));
    }
    Ok(sock)
}

/// Route an outbound connection through the active egress mode.
pub async fn open_multi_egress_upstream<F, Fut>(
    paths: &Paths,
    host: &str,
    port: u16,
    warp_fallback: F,
) -> Result<tokio::net::TcpStream>
where
    F: FnOnce(String, u16) -> Fut,
    Fut: std::future::Future<Output = Result<tokio::net::TcpStream>>,
{
    let cfg = crate::config::EgressConfig::load(paths)?;
    match cfg.mode {
        crate::config::EgressMode::Tor => {
            if let Some(target) = crate::services::tor::active_target().await {
                match super::socks::open_socks5_tunnel(
                    host,
                    port,
                    &target.0,
                    target.1,
                    std::time::Duration::from_secs(10),
                    "",
                    "",
                )
                .await
                {
                    Ok(sock) => return Ok(sock),
                    Err(err) => {
                        tracing::debug!(error = %redact(&err.user_message()), "tor tunnel failed");
                    }
                }
            } else {
                let _ = crate::services::tor::start_portable_tor(paths).await;
                if let Some(target) = crate::services::tor::active_target().await {
                    if let Ok(sock) = super::socks::open_socks5_tunnel(
                        host,
                        port,
                        &target.0,
                        target.1,
                        std::time::Duration::from_secs(10),
                        "",
                        "",
                    )
                    .await
                    {
                        return Ok(sock);
                    }
                }
            }
            if strict_mode(paths) {
                return Err(AppError::Process(
                    "Tor route unavailable (strict mode)".into(),
                ));
            }
            warp_fallback(host.to_string(), port).await
        }
        crate::config::EgressMode::Custom => {
            let specs = parse_proxy_specs(&cfg.custom_proxy, cfg.custom_type);
            let mut last_err = None;
            for _ in 0..specs.len().max(1) {
                let Some(spec) = get_current_proxy_spec(paths, &cfg) else {
                    break;
                };
                let result = match spec.proxy_type {
                    ProxyType::Http => {
                        open_http_proxy_tunnel(
                            host,
                            port,
                            &spec.host,
                            spec.port,
                            std::time::Duration::from_secs(8),
                            &spec.username,
                            &spec.password,
                        )
                        .await
                    }
                    ProxyType::Socks5 => {
                        super::socks::open_socks5_tunnel(
                            host,
                            port,
                            &spec.host,
                            spec.port,
                            std::time::Duration::from_secs(6),
                            &spec.username,
                            &spec.password,
                        )
                        .await
                    }
                };
                match result {
                    Ok(sock) => return Ok(sock),
                    Err(err) => {
                        last_err = Some(err);
                        let _ = advance_custom_proxy(paths);
                    }
                }
            }
            if strict_mode(paths) {
                return Err(AppError::Process(redact(&format!(
                    "all custom proxies failed (strict mode): {}",
                    last_err.map(|e| e.user_message()).unwrap_or_default()
                ))));
            }
            if let Ok(sock) = warp_fallback(host.to_string(), port).await {
                return Ok(sock);
            }
            Err(AppError::Process(redact(&format!(
                "all proxies in pool failed (last error: {})",
                last_err.map(|e| e.user_message()).unwrap_or_default()
            ))))
        }
        crate::config::EgressMode::Warp | crate::config::EgressMode::Clean => {
            warp_fallback(host.to_string(), port).await
        }
    }
}

pub fn strict_mode(paths: &Paths) -> bool {
    std::env::var("OUC_STRICT_PROXY").ok().as_deref() == Some("1") || paths.strict_flag().is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_entries() {
        let specs = parse_proxy_specs("127.0.0.1:10808", ProxyType::Socks5);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].host, "127.0.0.1");
        assert_eq!(specs[0].port, 10808);
        assert_eq!(specs[0].proxy_type, ProxyType::Socks5);
    }

    #[test]
    fn parses_mixed_pools_and_credentials() {
        let raw = "socks5://user:pass@1.2.3.4:1080, http://5.6.7.8:8080\n9.9.9.9:7890";
        let specs = parse_proxy_specs(raw, ProxyType::Socks5);
        assert_eq!(specs.len(), 3);
        assert_eq!(specs[0].username, "user");
        assert_eq!(specs[0].password, "pass");
        assert_eq!(specs[1].proxy_type, ProxyType::Http);
        assert_eq!(specs[2].host, "9.9.9.9");
    }

    #[test]
    fn sanitizes_credentials() {
        let clean = sanitize_proxy_string("socks5://user:secret@1.2.3.4:1080");
        assert!(!clean.contains("secret"));
        assert!(clean.contains("1.2.3.4:1080"));
        assert_eq!(
            extract_credentials("socks5://user:secret@1.2.3.4:1080"),
            Some(("user".into(), "secret".into()))
        );
    }

    #[test]
    fn rejects_invalid_entries() {
        assert!(parse_proxy_entry("not a proxy", ProxyType::Socks5).is_none());
        assert!(parse_proxy_entry("1.2.3.4:0", ProxyType::Socks5).is_none());
        assert!(parse_proxy_entry("1.2.3.4:70000", ProxyType::Socks5).is_none());
        assert!(parse_proxy_entry("", ProxyType::Socks5).is_none());
    }

    #[test]
    fn normalizes_ipv6_literal() {
        let spec = parse_proxy_entry("[2001:db8::1]:1080", ProxyType::Socks5).unwrap();
        assert_eq!(spec.host, "2001:db8::1");
        assert_eq!(spec.port, 1080);
    }
}
