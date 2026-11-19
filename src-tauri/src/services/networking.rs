//! Network interface discovery, IPv6 helpers, and public egress probing.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::time::timeout;
use windows::core::PWSTR;
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_INFO, GAA_FLAG_SKIP_MULTICAST,
    IP_ADAPTER_ADDRESSES_LH, IP_ADAPTER_UNICAST_ADDRESS_LH,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6, SOCKADDR_IN, SOCKADDR_IN6};

use crate::config::{EgressConfig, EgressMode, Paths, WarpScope};
use crate::errors::{AppError, Result};

/// Fallback OpenCode AAAA records (kept from the audited application).
const FALLBACK_OPENCODE_AAAA: &[&str] = &[
    "2606:4700::6810:7f60",
    "2606:4700::6810:8060",
    "2606:4700::6810:84e5",
    "2606:4700::6810:85e5",
    "2606:4700:11:d209",
    "2606:4700:12:a42",
    "2606:4700:12:b42",
    "2606:4700:10::6814:2011",
    "2606:4700:10::6814:2111",
    "2606:4700:10::6814:2211",
    "2606:4700:10::6814:2311",
    "2606:4700:10::ac42:ad95",
    "2606:4700:10::ac42:ad96",
    "2606:4700:10::ac42:ad97",
    "2606:4700:10::ac42:ad98",
    "2606:4700:20::681a:641",
    "2606:4700:20::681a:741",
    "2606:4700:20::681a:841",
    "2606:4700:20::681a:941",
    "2606:4700:30::6818:101",
    "2606:4700:30::6818:201",
    "2606:4700:30::6818:301",
    "2606:4700:30::6818:401",
    "2606:4700:30::ac43:8001",
    "2606:4700:30::ac43:8002",
    "2606:4700:30::ac43:9001",
    "2606:4700:30::ac43:9002",
    "2606:4700:30::ac43:a001",
    "2606:4700:30::ac43:a002",
    "2606:4700:31::a29f:8001",
    "2606:4700:31::a29f:8002",
    "2606:4700:31::a29f:9001",
    "2606:4700:31::a29f:9002",
    "2606:4700:e0::ac40:9001",
    "2606:4700:e0::ac40:9002",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInterface {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub mac: String,
    #[serde(default)]
    pub ipv4: Vec<String>,
    #[serde(default)]
    pub ipv6: Vec<String>,
    pub is_up: bool,
    #[serde(default)]
    pub is_loopback: bool,
}

fn pwidestr_to_string(ptr: PWSTR) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { ptr.to_string().unwrap_or_default() }
}

/// Enumerate adapters with unicast addresses using the IpHelper API.
pub fn list_interfaces() -> Vec<NetworkInterface> {
    let flags = GAA_FLAG_SKIP_ANYCAST.0 | GAA_FLAG_SKIP_MULTICAST.0 | GAA_FLAG_SKIP_DNS_INFO;
    let mut size: u32 = 0;
    unsafe {
        let status = GetAdaptersAddresses(
            0u32,
            windows::Win32::NetworkManagement::IpHelper::GET_ADAPTERS_ADDRESSES_FLAGS(flags),
            None,
            None,
            &mut size,
        );
        if status != 0 && size == 0 {
            return Vec::new();
        }
    }
    let mut buf: Vec<u8> = vec![0u8; size as usize];
    unsafe {
        let ptr = buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
        let status = GetAdaptersAddresses(
            0u32,
            windows::Win32::NetworkManagement::IpHelper::GET_ADAPTERS_ADDRESSES_FLAGS(flags),
            None,
            Some(ptr),
            &mut size,
        );
        if status != 0 {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut current: *mut IP_ADAPTER_ADDRESSES_LH = ptr;
        while !current.is_null() {
            let adapter = &*current;
            let name = pwidestr_to_string(adapter.FriendlyName);
            let description = pwidestr_to_string(adapter.Description);
            let mut ipv4 = Vec::new();
            let mut ipv6 = Vec::new();
            let mut unicast: *mut IP_ADAPTER_UNICAST_ADDRESS_LH = adapter.FirstUnicastAddress;
            while !unicast.is_null() {
                let entry = &*unicast;
                let sa = entry.Address.lpSockaddr;
                if !sa.is_null() {
                    let family = (*sa).sa_family;
                    if family == AF_INET {
                        let sin = sa as *const SOCKADDR_IN;
                        let bytes = (*sin).sin_addr.S_un.S_addr.to_ne_bytes();
                        ipv4.push(Ipv4Addr::from(bytes).to_string());
                    } else if family == AF_INET6 {
                        let sin6 = sa as *const SOCKADDR_IN6;
                        let bytes = (*sin6).sin6_addr.u.Byte;
                        let ip = Ipv6Addr::from(bytes);
                        if !ip.is_multicast() {
                            ipv6.push(ip.to_string());
                        }
                    }
                }
                unicast = entry.Next;
            }
            let is_up = adapter.OperStatus == IfOperStatusUp;
            let mac = adapter.PhysicalAddress[..adapter.PhysicalAddressLength.min(8) as usize]
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(":");
            out.push(NetworkInterface {
                name,
                description,
                mac,
                ipv4,
                ipv6,
                is_up,
                is_loopback: false,
            });
            current = adapter.Next;
        }
        out
    }
}

/// Return the active WARP adapter global IPv6 address, if any.
pub fn warp_local_v6() -> Option<String> {
    for iface in list_interfaces() {
        let haystack = format!("{} {}", iface.name, iface.description).to_ascii_lowercase();
        let warp_named = haystack.contains("warp") || haystack.contains("cloudflare");
        for addr in &iface.ipv6 {
            let clean = addr.split('%').next().unwrap_or(addr);
            if let Ok(ip) = clean.parse::<Ipv6Addr>() {
                if ip.is_loopback() || is_link_local(&ip) || ip.is_multicast() {
                    continue;
                }
                if warp_named || is_cloudflare_v6(clean) {
                    return Some(ip.to_string());
                }
            }
        }
    }
    None
}

fn is_link_local(ip: &Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xffc0) == 0xfe80
}

/// Normalize an IPv6 address to its `/64` network prefix string.
pub fn normalize_v6_64(addr: &str) -> String {
    let clean = addr.trim().split('%').next().unwrap_or("");
    if clean.is_empty() || !clean.contains(':') {
        return String::new();
    }
    match clean.parse::<Ipv6Addr>() {
        Ok(ip) => {
            let segments = ip.segments();
            Ipv6Addr::new(
                segments[0],
                segments[1],
                segments[2],
                segments[3],
                0,
                0,
                0,
                0,
            )
            .to_string()
        }
        Err(_) => String::new(),
    }
}

/// True when the address belongs to Cloudflare/WARP global ranges.
pub fn is_cloudflare_v6(addr: &str) -> bool {
    let clean = addr.split('%').next().unwrap_or("");
    let Ok(ip) = clean.parse::<Ipv6Addr>() else {
        return false;
    };
    if is_link_local(&ip) || ip.is_loopback() || ip.is_multicast() || ip.is_unspecified() {
        return false;
    }
    let octets = ip.octets();
    let hex = format!(
        "{:02x}{:02x}:{:02x}{:02x}",
        octets[0], octets[1], octets[2], octets[3]
    );
    hex.starts_with("2606:") || hex.starts_with("2a09:")
}

/// Validate a candidate public IP string strictly (no whitespace or markup).
pub fn valid_public_ip(text: &str) -> Option<IpAddr> {
    let cand = text.trim().split('%').next().unwrap_or("").trim();
    if cand.is_empty() || cand.len() > 64 || cand.contains([' ', '<', '>', '"', '\'']) {
        return None;
    }
    let ip: IpAddr = cand.parse().ok()?;
    match ip {
        IpAddr::V4(v4) => {
            if v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_unspecified()
                || v4.octets() == [255, 255, 255, 255]
            {
                None
            } else {
                Some(ip)
            }
        }
        IpAddr::V6(v6) => {
            if v6.is_loopback() || v6.is_multicast() || v6.is_unspecified() || is_link_local(&v6) {
                None
            } else {
                Some(ip)
            }
        }
    }
}

fn parse_egress_body(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("ip=") {
            if let Some(found) = valid_public_ip(value) {
                return Some(found.to_string());
            }
        }
    }
    valid_public_ip(text).map(|ip| ip.to_string())
}

const EGRESS_ENDPOINTS: &[&str] = &[
    "https://api64.ipify.org",
    "https://cloudflare.com/cdn-cgi/trace",
    "https://1.1.1.1/cdn-cgi/trace",
];

/// Probe the public egress address, optionally through an explicit proxy URL.
pub async fn probe_public_ip(proxy_url: Option<&str>, timeout_secs: u64) -> Option<String> {
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(timeout_secs))
        .user_agent("opencode-egress-manager/3.0");
    if let Some(url) = proxy_url {
        // reqwest only understands socks5://; remote DNS resolution is a property
        // of the proxy server, so the wire format is identical.
        let normalized = url.replacen("socks5h://", "socks5://", 1);
        if let Ok(proxy) = reqwest::Proxy::all(&normalized) {
            builder = builder.proxy(proxy);
        }
    }
    let client = match builder.build() {
        Ok(client) => client,
        Err(err) => {
            tracing::debug!(error = %crate::errors::redact(&err.to_string()), "egress client build failed");
            return None;
        }
    };
    for endpoint in EGRESS_ENDPOINTS {
        match timeout(
            Duration::from_secs(timeout_secs),
            client.get(*endpoint).send(),
        )
        .await
        {
            Ok(Ok(resp)) => {
                if let Ok(body) = resp.text().await {
                    if let Some(found) = parse_egress_body(&body) {
                        return Some(found);
                    }
                }
            }
            _ => continue,
        }
    }
    None
}

/// Return the public egress address for the active route.
pub async fn get_egress_ip(cfg: &EgressConfig, timeout_secs: u64) -> Option<String> {
    match cfg.mode {
        EgressMode::Tor => {
            for port in [9050u16, 9150u16] {
                if let Some(found) =
                    probe_public_ip(Some(&format!("socks5h://127.0.0.1:{port}")), timeout_secs)
                        .await
                {
                    return Some(found);
                }
            }
            None
        }
        EgressMode::Custom => {
            if let Some(spec) =
                crate::services::proxy::chain::get_current_proxy_spec(&Paths::discover(), cfg)
            {
                let url = spec.proxy_url();
                if let Some(found) = probe_public_ip(Some(&url), timeout_secs).await {
                    return Some(found);
                }
            }
            None
        }
        EgressMode::Warp | EgressMode::Clean => {
            if cfg.warp_scope == WarpScope::Isolated {
                if let Some(found) = probe_public_ip(
                    Some(&format!(
                        "socks5h://127.0.0.1:{}",
                        crate::config::WARP_ISOLATED_SOCKS_PORT
                    )),
                    timeout_secs,
                )
                .await
                {
                    return Some(found);
                }
            }
            probe_public_ip(None, timeout_secs).await
        }
    }
}

/// The public IPv6 egress address, verified externally (never the adapter address).
pub async fn get_public_v6(cfg: &EgressConfig, timeout_secs: u64) -> Option<String> {
    let egress = get_egress_ip(cfg, timeout_secs).await?;
    if egress.contains(':') {
        Some(egress)
    } else {
        None
    }
}

#[derive(Debug, Deserialize)]
struct DohAnswer {
    #[serde(default)]
    answer: Vec<DohRecord>,
}

#[derive(Debug, Deserialize)]
struct DohRecord {
    #[serde(rename = "type")]
    record_type: u16,
    #[serde(default)]
    data: String,
}

/// Gather live AAAA records for opencode.ai via DoH, system DNS, then fallbacks.
pub async fn fetch_opencode_aaaa_pool() -> Vec<String> {
    let mut pool: std::collections::BTreeSet<String> = Default::default();
    let client = match reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(4))
        .build()
    {
        Ok(client) => client,
        Err(_) => {
            return FALLBACK_OPENCODE_AAAA
                .iter()
                .map(|s| s.to_string())
                .collect()
        }
    };
    for host in [
        "https://1.1.1.1/dns-query?name=opencode.ai&type=AAAA",
        "https://1.0.0.1/dns-query?name=opencode.ai&type=AAAA",
        "https://dns.google/resolve?name=opencode.ai&type=AAAA",
    ] {
        if let Ok(resp) = client
            .get(host)
            .header(reqwest::header::ACCEPT, "application/dns-json")
            .send()
            .await
        {
            if let Ok(body) = resp.json::<DohAnswer>().await {
                for ans in body.answer {
                    if ans.record_type == 28 && ans.data.parse::<Ipv6Addr>().is_ok() {
                        pool.insert(ans.data);
                    }
                }
            }
        }
    }
    for host in ["opencode.ai", "api.opencode.ai"] {
        if let Ok(addrs) = tokio::net::lookup_host((host, 443)).await {
            for addr in addrs {
                if let IpAddr::V6(v6) = addr.ip() {
                    if (v6.segments()[0] & 0xffc0) != 0xfe80 {
                        pool.insert(v6.to_string());
                    }
                }
            }
        }
    }
    for fallback in FALLBACK_OPENCODE_AAAA {
        pool.insert((*fallback).to_string());
    }
    pool.into_iter().collect()
}

/// Look up whether a TCP port is open on loopback.
pub async fn port_open(host: &str, port: u16, timeout_ms: u64) -> bool {
    timeout(
        Duration::from_millis(timeout_ms),
        tokio::net::TcpStream::connect((host, port)),
    )
    .await
    .map(|r| r.is_ok())
    .unwrap_or(false)
}

/// Run a Windows command with a hidden console and fixed arguments.
pub async fn run_hidden(
    program: &str,
    args: &[String],
    timeout_secs: u64,
) -> Result<(i32, String)> {
    use tokio::process::Command;

    let mut cmd = Command::new(program);
    cmd.args(args)
        .creation_flags(0x0800_0000)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null());
    let output = timeout(Duration::from_secs(timeout_secs), cmd.output())
        .await
        .map_err(|_| AppError::Process(format!("{program} timed out")))?
        .map_err(|e| AppError::Process(format!("{program} failed to start: {e}")))?;
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok((output.status.code().unwrap_or(-1), text))
}

/// Resolve a host:port pair, returning only IPv6 addresses.
pub fn resolve_v6(host: &str, port: u16) -> Vec<Ipv6Addr> {
    let mut out = Vec::new();
    if let Ok(addrs) = (host, port).to_socket_addrs() {
        for addr in addrs {
            if let IpAddr::V6(v6) = addr.ip() {
                out.push(v6);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interfaces_serialize_camel_case() {
        let iface = NetworkInterface {
            name: "Ethernet".into(),
            description: "Ethernet".into(),
            mac: "AA:BB:CC:DD:EE:FF".into(),
            ipv4: vec!["192.168.1.2".into()],
            ipv6: vec!["fe80::1".into()],
            is_up: true,
            is_loopback: false,
        };
        let json = serde_json::to_value(&iface).unwrap();
        assert_eq!(json["isUp"], true);
        assert_eq!(json["isLoopback"], false);
        assert!(json.get("is_up").is_none());
    }

    #[test]
    fn normalizes_prefixes() {
        assert_eq!(
            normalize_v6_64("2606:4700:1234:5678:9abc:def0:1234:5678"),
            "2606:4700:1234:5678::"
        );
        assert_eq!(normalize_v6_64("not-an-ip"), "");
        assert_eq!(normalize_v6_64("fe80::1%5"), "fe80::");
    }

    #[test]
    fn detects_cloudflare_v6() {
        assert!(is_cloudflare_v6("2606:4700::6810:7f60"));
        assert!(!is_cloudflare_v6("fe80::1"));
        assert!(!is_cloudflare_v6("2001:db8::1"));
    }

    #[test]
    fn rejects_non_public_addresses() {
        assert!(valid_public_ip("127.0.0.1").is_none());
        assert!(valid_public_ip("10.0.0.1").is_none());
        assert!(valid_public_ip("192.168.1.1").is_none());
        assert!(valid_public_ip("1.1.1.1").is_some());
        assert!(valid_public_ip("2606:4700::6810:7f60").is_some());
        assert!(valid_public_ip("<html>").is_none());
        assert!(valid_public_ip("1.1.1.1 extra").is_none());
    }
}
