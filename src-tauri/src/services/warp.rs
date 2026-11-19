//! Cloudflare WARP control through the official `warp-cli.exe`.
//!
//! Every invocation is an argument vector (never a shell string), and the JSON and
//! text output formats are both handled because WARP versions differ.

use std::{
    net::{Ipv4Addr, ToSocketAddrs},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use serde::Serialize;
use tokio::time::sleep;

use crate::{
    config::{EgressConfig, Paths, WarpScope, SERVICE_PROXY_PORT_DEFAULT},
    errors::{AppError, Result},
    services::networking::{self, run_hidden},
};

pub const KNOWN_CLEAN_ENDPOINTS: &[&str] = &[
    "162.159.192.1:2408",
    "162.159.192.2:2408",
    "162.159.193.1:2408",
    "162.159.193.5:2408",
    "162.159.195.1:2408",
    "188.114.96.1:2408",
    "188.114.97.1:2408",
    "188.114.98.1:2408",
    "188.114.99.1:2408",
    "162.159.192.1:500",
    "162.159.193.1:500",
    "188.114.96.1:500",
    "188.114.97.1:500",
    "162.159.192.1:4500",
    "162.159.193.1:4500",
    "188.114.96.1:4500",
    "188.114.97.1:4500",
    "162.159.192.1:1701",
    "162.159.193.1:1701",
    "162.159.192.1:854",
    "162.159.193.1:854",
    "162.159.192.1:880",
    "162.159.193.1:880",
];

fn warp_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(env_bin) = std::env::var("OUC_WARP_BIN") {
        if !env_bin.trim().is_empty() {
            out.push(PathBuf::from(env_bin));
        }
    }
    if let Ok(path) = which::which("warp-cli") {
        out.push(path);
    }
    out.push(PathBuf::from(
        r"C:\Program Files\Cloudflare\Cloudflare WARP\warp-cli.exe",
    ));
    out.push(PathBuf::from(
        r"C:\Program Files (x86)\Cloudflare\Cloudflare WARP\warp-cli.exe",
    ));
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        out.push(
            PathBuf::from(local)
                .join("Cloudflare")
                .join("Cloudflare WARP")
                .join("warp-cli.exe"),
        );
    }
    out
}

pub fn find_warp_cli() -> Option<PathBuf> {
    warp_candidates().into_iter().find(|p| p.is_file())
}

async fn run_warp_async(bin: &Path, args: &[&str], timeout_secs: u64) -> Result<(i32, String)> {
    let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    // Fresh installs need --accept-tos; shimmed boxes reject a duplicate flag.
    let mut with_flag = vec!["--accept-tos".to_string()];
    with_flag.extend(owned.iter().cloned());
    if let Ok(result) = run_hidden(&bin.to_string_lossy(), &with_flag, timeout_secs).await {
        if result.0 == 0 {
            return Ok(result);
        }
        let without = run_hidden(&bin.to_string_lossy(), &owned, timeout_secs).await?;
        return Ok(without);
    }
    run_hidden(&bin.to_string_lossy(), &owned, timeout_secs).await
}

/// Public wrapper for invoking `warp-cli.exe` with the accept-TOS fallback.
pub async fn run(bin: &Path, args: &[&str], timeout_secs: u64) -> Result<(i32, String)> {
    run_warp_async(bin, args, timeout_secs).await
}

#[derive(Debug, Clone)]
pub struct WarpStatus {
    pub connected: bool,
    pub mode: String,
    pub status_text: String,
}

/// Query WARP status. Parses JSON when available, otherwise the text form.
pub async fn warp_status(bin: &Path, force: bool) -> WarpStatus {
    let _ = force;
    let (code, out) = run_warp_async(bin, &["-j", "status"], 6)
        .await
        .unwrap_or((1, String::new()));
    if code == 0 {
        if let Some(start) = out.find('{') {
            if let Some(end) = out.rfind('}') {
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&out[start..=end]) {
                    let status_text = data
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Disconnected")
                        .to_string();
                    let connected = status_text.eq_ignore_ascii_case("connected");
                    let mut mode = String::new();
                    if let Some(op) = data.pointer("/reason/SettingsChanged/current/operation_mode")
                    {
                        mode = match op {
                            serde_json::Value::String(s) => s.clone(),
                            serde_json::Value::Object(map) => {
                                map.keys().cloned().collect::<Vec<_>>().join(",")
                            }
                            _ => String::new(),
                        };
                    }
                    let mut detail = status_text.clone();
                    if !connected {
                        if let Some(reason) = data.get("reason") {
                            let rendered = if reason.is_string() {
                                reason.as_str().unwrap_or("").to_string()
                            } else {
                                serde_json::to_string(reason).unwrap_or_default()
                            };
                            if !rendered.is_empty() {
                                detail = format!("{status_text}: {rendered}");
                            }
                        }
                    }
                    if mode.is_empty() {
                        mode = warp_mode(bin).await;
                    }
                    return WarpStatus {
                        connected,
                        mode,
                        status_text: detail,
                    };
                }
            }
        }
    }
    // Text fallback.
    let (code, out) = run_warp_async(bin, &["status"], 6)
        .await
        .unwrap_or((1, String::new()));
    let mut connected = false;
    let mut first = String::new();
    if code == 0 {
        for line in out.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let low = trimmed.to_ascii_lowercase();
            if low.contains("status") || low.contains("connected") {
                if first.is_empty() {
                    first = trimmed.to_string();
                }
                if low.contains("connected") && !low.contains("disconnected") {
                    connected = true;
                    first = trimmed.to_string();
                    break;
                }
            }
        }
    }
    WarpStatus {
        connected,
        mode: warp_mode(bin).await,
        status_text: if first.is_empty() {
            "Disconnected".into()
        } else {
            first
        },
    }
}

pub async fn warp_mode(bin: &Path) -> String {
    let (code, out) = run_warp_async(bin, &["-j", "settings", "list"], 6)
        .await
        .unwrap_or((1, String::new()));
    if code == 0 {
        if let Some(start) = out.find('{') {
            if let Some(end) = out.rfind('}') {
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&out[start..=end]) {
                    let op = data
                        .pointer("/settings/operation_mode")
                        .or_else(|| data.get("operation_mode"));
                    let value = match op {
                        Some(serde_json::Value::String(s)) => Some(s.clone()),
                        Some(serde_json::Value::Object(map)) => {
                            Some(map.keys().cloned().collect::<Vec<_>>().join(","))
                        }
                        _ => None,
                    };
                    if let Some(v) = value {
                        if !v.is_empty() {
                            return v;
                        }
                    }
                }
            }
        }
    }
    let (code, out) = run_warp_async(bin, &["settings"], 5)
        .await
        .unwrap_or((1, String::new()));
    if code == 0 {
        for line in out.lines() {
            if line.to_ascii_lowercase().contains("mode") && line.contains(':') {
                let value = line
                    .rsplit(':')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase();
                if ["warp", "proxy", "dns_only", "tunnel_only"].contains(&value.as_str()) {
                    return value;
                }
            }
        }
    }
    String::new()
}

pub async fn warp_protocol(bin: &Path) -> String {
    let (code, out) = run_warp_async(bin, &["-j", "settings"], 5)
        .await
        .unwrap_or((1, String::new()));
    if code == 0 {
        if let Some(start) = out.find('{') {
            if let Some(end) = out.rfind('}') {
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&out[start..=end]) {
                    if let Some(proto) = data.pointer("/settings/warp_tunnel_protocol") {
                        if let Some(s) = proto.as_str() {
                            return s.to_ascii_lowercase();
                        }
                    }
                }
            }
        }
    }
    let (code, out) = run_warp_async(bin, &["settings"], 5)
        .await
        .unwrap_or((1, String::new()));
    if code == 0 {
        for line in out.lines() {
            let low = line.to_ascii_lowercase();
            if low.contains("protocol") {
                if low.contains("wireguard") {
                    return "wireguard".into();
                }
                if low.contains("masque") {
                    return "masque".into();
                }
            }
        }
    }
    String::new()
}

pub async fn ensure_warp_protocol(bin: &Path, desired: &str) -> bool {
    let want = desired.to_ascii_lowercase();
    if want != "wireguard" && want != "masque" {
        return false;
    }
    if warp_protocol(bin).await == want {
        return true;
    }
    let label = if want == "wireguard" {
        "WireGuard"
    } else {
        "MASQUE"
    };
    matches!(
        run_warp_async(bin, &["tunnel", "protocol", "set", label], 10).await,
        Ok((0, _))
    )
}

pub async fn ensure_warp_mode(bin: &Path, scope: WarpScope) -> bool {
    let desired = if scope == WarpScope::Isolated {
        "proxy"
    } else {
        "warp"
    };
    if warp_mode(bin).await == desired {
        return true;
    }
    if desired == "proxy" {
        let _ = run_warp_async(
            bin,
            &["proxy", "port", &SERVICE_PROXY_PORT_DEFAULT.to_string()],
            10,
        )
        .await;
    }
    matches!(
        run_warp_async(bin, &["mode", desired], 10).await,
        Ok((0, _))
    )
}

pub async fn registration_healthy(bin: &Path) -> bool {
    matches!(
        run_warp_async(bin, &["registration", "show"], 8).await,
        Ok((0, _))
    )
}

pub async fn repair_registration(bin: &Path, max_tries: u32) -> bool {
    if registration_healthy(bin).await {
        return true;
    }
    for attempt in 0..max_tries.max(1) {
        match run_warp_async(bin, &["registration", "new"], 30).await {
            Ok((0, _)) => return true,
            Ok((_, out)) => {
                if out.to_ascii_lowercase().contains("old registration") {
                    let _ = run_warp_async(bin, &["registration", "delete"], 20).await;
                    continue;
                }
            }
            Err(_) => {}
        }
        if attempt + 1 < max_tries.max(1) {
            sleep(Duration::from_secs(4)).await;
        }
    }
    false
}

pub async fn connect(bin: &Path, scope: WarpScope) -> bool {
    ensure_warp_mode(bin, scope).await;
    ensure_warp_protocol(bin, "wireguard").await;
    let _ = run_warp_async(bin, &["connect"], 15).await;
    for _ in 0..10 {
        sleep(Duration::from_millis(900)).await;
        let status = warp_status(bin, true).await;
        if status.connected {
            return true;
        }
    }
    false
}

pub async fn disconnect(bin: &Path) -> Result<()> {
    run_warp_async(bin, &["disconnect"], 10).await?;
    Ok(())
}

async fn wait_connected(bin: &Path, attempts: u32) -> bool {
    for _ in 0..attempts {
        sleep(Duration::from_secs(1)).await;
        if warp_status(bin, true).await.connected {
            return true;
        }
    }
    false
}

fn registration_missing(raw: &str) -> bool {
    let low = raw.to_ascii_lowercase();
    ["registration missing", "not registered", "no registration"]
        .iter()
        .any(|marker| low.contains(marker))
}

/// Keep the tunnel connected without resetting a healthy session.
pub async fn ensure_connected(bin: &Path, paths: &Paths) -> bool {
    let cfg = EgressConfig::load(paths).unwrap_or_default();
    let scope = cfg.warp_scope;
    let desired_mode = if scope == WarpScope::Isolated {
        "proxy"
    } else {
        "warp"
    };
    let status = warp_status(bin, true).await;
    if status.connected {
        let current = warp_mode(bin).await;
        if current.is_empty() || current == desired_mode {
            return true;
        }
        crate::config::append_log(
            &paths.keeper_log_file(),
            &format!(
                "operation mode drift ({current} -> {desired_mode}) - applying selected scope"
            ),
            1_048_576,
        );
        ensure_warp_mode(bin, scope).await;
        if wait_connected(bin, 8).await {
            return true;
        }
    }
    let raw = warp_status(bin, true).await.status_text;
    if raw.to_ascii_lowercase().contains("connecting") && wait_connected(bin, 8).await {
        return true;
    }
    if registration_missing(&raw) {
        crate::config::append_log(
            &paths.keeper_log_file(),
            "registration missing - creating a new one",
            1_048_576,
        );
        if !repair_registration(bin, 3).await {
            return false;
        }
    }
    if connect(bin, scope).await {
        return true;
    }
    crate::config::append_log(
        &paths.keeper_log_file(),
        "tunnel disconnected - renewing keys and reconnecting",
        1_048_576,
    );
    let _ = run_warp_async(bin, &["tunnel", "rotate-keys"], 15).await;
    if connect(bin, scope).await {
        return true;
    }
    let raw = warp_status(bin, true).await.status_text;
    if registration_missing(&raw) && repair_registration(bin, 3).await {
        return connect(bin, scope).await;
    }
    false
}

/// One confirmed WARP `/64` rotation attempt: returns a fresh public IPv6 address.
pub async fn hop_once(
    bin: &Path,
    paths: &Paths,
    max_reg_tries: u32,
    avoid_prefixes: &[String],
    previous: &str,
) -> Option<String> {
    let cfg = EgressConfig::load(paths).unwrap_or_default();
    let scope = cfg.warp_scope;
    let mut avoid: Vec<String> = avoid_prefixes
        .iter()
        .map(|v| networking::normalize_v6_64(v))
        .filter(|v| !v.is_empty())
        .collect();
    let old_prefix = networking::normalize_v6_64(previous);
    if !old_prefix.is_empty() {
        avoid.push(old_prefix.clone());
    }

    ensure_warp_mode(bin, scope).await;
    ensure_warp_protocol(bin, "wireguard").await;

    if !warp_status(bin, true).await.connected {
        let _ = run_warp_async(bin, &["connect"], 15).await;
        if !wait_connected(bin, 6).await {
            return None;
        }
    }

    for _rotation in 0..3 {
        let _ = run_warp_async(bin, &["tunnel", "rotate-keys"], 15).await;
        let _ = run_warp_async(bin, &["disconnect"], 10).await;
        sleep(Duration::from_millis(800)).await;
        let _ = run_warp_async(bin, &["connect"], 15).await;
        for _ in 0..6 {
            sleep(Duration::from_millis(1500)).await;
            let status = warp_status(bin, true).await;
            if !status.connected {
                break;
            }
            if let Some(candidate) = networking::get_public_v6(&cfg, 2).await {
                let prefix = networking::normalize_v6_64(&candidate);
                if !prefix.is_empty() && !avoid.contains(&prefix) {
                    return Some(candidate);
                }
            }
        }
        if !warp_status(bin, true).await.connected && repair_registration(bin, max_reg_tries).await
        {
            ensure_warp_mode(bin, scope).await;
            let _ = run_warp_async(bin, &["connect"], 15).await;
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Clean endpoints
// ---------------------------------------------------------------------------

/// Measure round-trip latency to a Cloudflare endpoint using TCP connect.
pub async fn test_endpoint_latency(endpoint: &str, timeout_ms: u64) -> Option<f64> {
    let (host, port) = split_endpoint(endpoint)?;
    let addr: std::net::SocketAddr = format!("{host}:{port}")
        .to_socket_addrs()
        .ok()?
        .find(|a| a.is_ipv4())
        .or_else(|| format!("{host}:{port}").to_socket_addrs().ok()?.next())?;
    let start = Instant::now();
    let ok = networking::port_open(&addr.ip().to_string(), addr.port(), timeout_ms).await;
    if ok {
        return Some(start.elapsed().as_secs_f64() * 1000.0);
    }
    None
}

fn split_endpoint(endpoint: &str) -> Option<(String, u16)> {
    let endpoint = endpoint.trim();
    let (host, port) = endpoint.rsplit_once(':')?;
    let port: u16 = port.parse().ok()?;
    if host.is_empty() || host.parse::<Ipv4Addr>().is_err() {
        // Only numeric IPv4 endpoints are accepted here (defense in depth).
        return None;
    }
    Some((host.to_string(), port))
}

pub async fn set_custom_endpoint(bin: &Path, endpoint: &str) -> Result<()> {
    validate_endpoint(endpoint)?;
    ensure_warp_protocol(bin, "wireguard").await;
    let (code, msg) =
        run_warp_async(bin, &["tunnel", "endpoint", "set", endpoint.trim()], 15).await?;
    if code != 0 {
        return Err(AppError::Process(msg));
    }
    let _ = run_warp_async(bin, &["tunnel", "rotate-keys"], 15).await;
    let _ = run_warp_async(bin, &["connect"], 15).await;
    for _ in 0..10 {
        sleep(Duration::from_millis(800)).await;
        let status = warp_status(bin, true).await;
        let low = status.status_text.to_ascii_lowercase();
        if status.connected && !low.contains("unable") && !low.contains("mitm") {
            return Ok(());
        }
    }
    Err(AppError::Process(format!(
        "endpoint {endpoint} was set but WARP did not connect"
    )))
}

pub async fn reset_custom_endpoint(bin: &Path) -> Result<()> {
    let (code, msg) = run_warp_async(bin, &["tunnel", "endpoint", "reset"], 15).await?;
    if code != 0 {
        return Err(AppError::Process(msg));
    }
    ensure_warp_protocol(bin, "wireguard").await;
    let _ = run_warp_async(bin, &["tunnel", "rotate-keys"], 15).await;
    let _ = run_warp_async(bin, &["connect"], 15).await;
    for _ in 0..10 {
        sleep(Duration::from_millis(800)).await;
        if warp_status(bin, true).await.connected {
            return Ok(());
        }
    }
    Err(AppError::Process(
        "endpoint reset, but WARP did not reconnect".into(),
    ))
}

pub async fn auto_heal_endpoint(bin: &Path, timeout_ms: u64) -> Result<String> {
    let mut best: Option<(f64, String)> = None;
    for endpoint in KNOWN_CLEAN_ENDPOINTS {
        if let Some(lat) = test_endpoint_latency(endpoint, timeout_ms).await {
            if best
                .as_ref()
                .map(|(best_lat, _)| lat < *best_lat)
                .unwrap_or(true)
            {
                best = Some((lat, (*endpoint).to_string()));
            }
        }
    }
    let (latency, endpoint) =
        best.ok_or_else(|| AppError::Process("no responsive clean endpoints found".into()))?;
    set_custom_endpoint(bin, &endpoint).await?;
    Ok(format!(
        "auto-healed connection using {endpoint} ({latency:.1}ms)"
    ))
}

fn validate_endpoint(endpoint: &str) -> Result<()> {
    let trimmed = endpoint.trim();
    if trimmed.len() > 253 {
        return Err(AppError::validation("endpoint is too long"));
    }
    if trimmed.contains(char::is_whitespace) || trimmed.contains(['\r', '\n']) {
        return Err(AppError::validation(
            "endpoint must be a single IP:port value",
        ));
    }
    split_endpoint(trimmed).ok_or_else(|| {
        AppError::validation(format!("invalid endpoint (expected IPv4:port): {trimmed}"))
    })?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EgressState {
    pub mode: String,
    pub warp_scope: String,
    pub custom_proxy: String,
    pub custom_type: String,
    pub tor_active: bool,
    pub tor_target: String,
    pub tor_running: bool,
    pub custom_pool_count: usize,
    pub custom_current_node: String,
    pub clean_endpoints: Vec<String>,
}

pub async fn egress_state(paths: &Paths) -> EgressState {
    let cfg = EgressConfig::load(paths).unwrap_or_default();
    let tor = crate::services::tor::status_for(paths).await;
    let pool = crate::services::proxy::chain::parse_proxy_specs(&cfg.custom_proxy, cfg.custom_type);
    let index = crate::services::proxy::chain::proxy_index_from_paths(paths, pool.len());
    let current = pool.get(index);
    EgressState {
        mode: cfg.mode.as_str().to_string(),
        warp_scope: cfg.warp_scope.as_str().to_string(),
        custom_proxy: cfg.custom_proxy.clone(),
        custom_type: cfg.custom_type.as_str().to_string(),
        tor_active: tor.active,
        tor_target: tor
            .target
            .as_ref()
            .map(|(host, port)| format!("{host}:{port}"))
            .unwrap_or_else(|| "Not running".into()),
        tor_running: tor.portable_running,
        custom_pool_count: pool.len(),
        custom_current_node: current
            .map(|spec| format!("{}:{}", spec.host, spec.port))
            .unwrap_or_default(),
        clean_endpoints: KNOWN_CLEAN_ENDPOINTS
            .iter()
            .map(|s| s.to_string())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn egress_state_serializes_camel_case() {
        let state = EgressState {
            mode: "warp".into(),
            warp_scope: "tun".into(),
            custom_proxy: "127.0.0.1:10808".into(),
            custom_type: "socks5".into(),
            tor_active: false,
            tor_target: "Not running".into(),
            tor_running: false,
            custom_pool_count: 1,
            custom_current_node: "127.0.0.1:10808".into(),
            clean_endpoints: vec!["162.159.192.1:2408".into()],
        };
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["warpScope"], "tun");
        assert_eq!(json["customProxy"], "127.0.0.1:10808");
        assert_eq!(json["customType"], "socks5");
        assert_eq!(json["torActive"], false);
        assert_eq!(json["torTarget"], "Not running");
        assert_eq!(json["torRunning"], false);
        assert_eq!(json["customPoolCount"], 1);
        assert_eq!(json["customCurrentNode"], "127.0.0.1:10808");
        assert!(json["cleanEndpoints"].is_array());
        assert!(json.get("clean_endpoints").is_none());
    }
}
