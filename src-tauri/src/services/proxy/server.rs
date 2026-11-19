//! Local HTTP/CONNECT proxy bound to 127.0.0.1.
//!
//! Routes traffic through WARP (TUN or isolated SOCKS), Tor, or a custom pool, with
//! pinned-edge failover, stream-reset accounting, and explicit fallback recording.

use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use rand::RngExt;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Mutex,
};
use tokio_util::sync::CancellationToken;

use crate::{
    config::{EgressConfig, Paths, ProxyStatus},
    errors::{redact, AppError, Result},
    services::{
        networking,
        proxy::{chain, edge, edge::EdgeState, socks},
        tor,
    },
};

const PROBE_PINNED_HOSTS: &[&str] = &[
    "opencode.ai",
    "www.opencode.ai",
    "api.opencode.ai",
    "opencode.com",
    "www.opencode.com",
    "api.opencode.com",
    "inference.opencode.ai",
];

/// Handle for a running local proxy listener.
pub struct RunningProxy {
    /// Actual bound port (useful when started with port 0).
    pub port: u16,
    cancel: CancellationToken,
    join: tokio::task::JoinHandle<()>,
}

impl RunningProxy {
    /// Cancel the accept loop and wait briefly for it to finish.
    pub async fn stop(self) {
        self.cancel.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(3), self.join).await;
    }
}

struct ProxyCtx {
    paths: Paths,
    edge: EdgeState,
    status: Mutex<ProxyStatus>,
    active_tunnels: AtomicUsize,
    cfg_cache: Mutex<EgressConfig>,
}

impl ProxyCtx {
    async fn config(&self) -> EgressConfig {
        let mut cache = self.cfg_cache.lock().await;
        if let Ok(cfg) = EgressConfig::load(&self.paths) {
            *cache = cfg;
        }
        cache.clone()
    }

    fn record(&self, kind: &str, detail: &str, host: &str, edge: &str) {
        let mut status = match self.status.try_lock() {
            Ok(status) => status,
            Err(_) => return,
        };
        status.record(&self.paths, kind, detail, host, edge);
    }

    async fn mark_stream_reset(&self, host: &str, detail: &str, edge: &str) {
        let replacement = self.edge.note_stream_reset(edge).await;
        let detail = if let Some(new_edge) = replacement {
            format!("{detail}; moved to {new_edge}")
        } else {
            detail.to_string()
        };
        self.record("stream_reset", &detail, host, edge);
    }
}

/// Start the proxy on `port` (0 = ephemeral). Runs until cancelled.
pub async fn start_proxy_server(paths: &Paths, port: u16) -> Result<RunningProxy> {
    paths.ensure_dirs()?;
    let scope = EgressConfig::load(paths).unwrap_or_default().warp_scope;
    let initial_edge = std::env::var("OUC_PIN_EDGE")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_default();

    let pool = if scope == crate::config::WarpScope::Isolated && initial_edge.is_empty() {
        Vec::new()
    } else {
        networking::fetch_opencode_aaaa_pool().await
    };
    let chosen_edge = if !initial_edge.is_empty() {
        initial_edge
    } else {
        pool.choose_random().unwrap_or_default()
    };
    if !chosen_edge.is_empty() {
        let _ = crate::config::write_text(&paths.last_edge_file(), &(chosen_edge.clone() + "\n"));
    }

    let listener = TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| {
        let mut status = ProxyStatus::default();
        status.record(
            &paths.clone(),
            "bind_fail",
            &format!("127.0.0.1:{port}: {e}"),
            "",
            "",
        );
        AppError::Process(format!("proxy bind failed on 127.0.0.1:{port}: {e}"))
    })?;
    let actual_port = listener.local_addr()?.port();

    let ctx = Arc::new(ProxyCtx {
        paths: paths.clone(),
        edge: EdgeState::new(pool, chosen_edge.clone()),
        status: Mutex::new(ProxyStatus::load(paths)),
        active_tunnels: AtomicUsize::new(0),
        cfg_cache: Mutex::new(EgressConfig::default()),
    });
    ctx.record(
        "start",
        &format!(
            "listening on 127.0.0.1:{actual_port} (edge={})",
            if chosen_edge.is_empty() {
                "isolated".to_string()
            } else {
                chosen_edge.clone()
            }
        ),
        "",
        "",
    );

    let cancel = CancellationToken::new();
    let loop_cancel = cancel.clone();
    let watch_paths = paths.clone();
    let watch_ctx = ctx.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = loop_cancel.cancelled() => break,
                _ = ticker.tick() => {
                    watch_ctx.edge.poll_controls(&watch_paths).await;
                }
            }
        }
    });

    let accept_ctx = ctx.clone();
    let accept_cancel = cancel.clone();
    let join = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = accept_cancel.cancelled() => break,
                accepted = listener.accept() => {
                    let Ok((stream, _peer)) = accepted else { continue };
                    let ctx = accept_ctx.clone();
                    tokio::spawn(async move {
                        let _ = stream.set_nodelay(true);
                        if let Err(err) = handle_connection(ctx, stream).await {
                            tracing::debug!(error = %redact(&err.user_message()), "proxy connection ended");
                        }
                    });
                }
            }
        }
    });

    Ok(RunningProxy {
        port: actual_port,
        cancel,
        join,
    })
}

/// Load the persisted proxy status and whether the fallback flag is set.
pub fn proxy_status_snapshot(paths: &Paths) -> (ProxyStatus, bool) {
    let status = ProxyStatus::load(paths);
    let fallback = paths.fallback_flag().is_file();
    (status, fallback)
}

fn is_pinned_host(host: &str) -> bool {
    PROBE_PINNED_HOSTS.contains(&host)
        || host.ends_with(".opencode.ai")
        || host.ends_with(".opencode.com")
}

async fn handle_connection(ctx: Arc<ProxyCtx>, mut client: TcpStream) -> Result<()> {
    let (head, leftover) = read_headers(&mut client, 65536).await?;
    let mut lines = head.split("\r\n").flat_map(|l| l.split('\n'));
    let request_line = lines.next().unwrap_or("").trim().to_string();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_ascii_uppercase();
    let target = parts.next().unwrap_or("").to_string();
    let proto = parts.next().unwrap_or("HTTP/1.1").to_string();
    if method.is_empty() || target.is_empty() {
        send_error(&mut client, 400, "Bad Request", "").await;
        return Ok(());
    }

    let mut headers: Vec<String> = Vec::new();
    let mut content_length: usize = 0;
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("proxy-connection:") || lower.starts_with("proxy-authorization:") {
            continue;
        }
        if let Some(value) = lower.strip_prefix("content-length:") {
            content_length = value.trim().parse().unwrap_or(0);
        }
        headers.push(line.trim().to_string());
    }

    if method == "CONNECT" {
        let (host, port) = parse_connect_target(&target)?;
        let upstream = match open_upstream(ctx.clone(), &host, port).await {
            Ok(sock) => sock,
            Err(err) => {
                if chain::strict_mode(&ctx.paths) {
                    ctx.record(
                        "error",
                        &format!("CONNECT {host}:{port}: {}", err.user_message()),
                        &host,
                        "",
                    );
                    send_error(
                        &mut client,
                        503,
                        "Service Unavailable",
                        &format!("Strict mode blocked the route to {host}:{port}."),
                    )
                    .await;
                    return Ok(());
                }
                match direct_connect(&host, port).await {
                    Ok(sock) => {
                        ctx.record(
                            "fallback",
                            &format!(
                                "CONNECT {host}:{port} used the direct route ({})",
                                err.user_message()
                            ),
                            &host,
                            "",
                        );
                        let _ = sock.set_nodelay(true);
                        sock
                    }
                    Err(err2) => {
                        ctx.record(
                            "error",
                            &format!("CONNECT {host}:{port}: {}", err2.user_message()),
                            &host,
                            "",
                        );
                        send_error(
                            &mut client,
                            503,
                            "Service Unavailable",
                            &format!(
                                "Opencode Egress Manager could not reach {host}:{port}. {}",
                                err2.user_message()
                            ),
                        )
                        .await;
                        return Ok(());
                    }
                }
            }
        };
        let edge = ctx.edge.current_edge().await;
        let _ = client
            .write_all(format!("{proto} 200 Connection Established\r\n\r\n").as_bytes())
            .await;
        ctx.active_tunnels.fetch_add(1, Ordering::SeqCst);
        relay(ctx.clone(), client, upstream, &host, &edge).await;
        ctx.active_tunnels.fetch_sub(1, Ordering::SeqCst);
        return Ok(());
    }

    let (host, port, path) = parse_absolute_target(&target)?;
    let mut upstream = match open_upstream(ctx.clone(), &host, port).await {
        Ok(sock) => sock,
        Err(err) => {
            if chain::strict_mode(&ctx.paths) {
                ctx.record(
                    "error",
                    &format!("HTTP {host}:{port}: {}", err.user_message()),
                    &host,
                    "",
                );
                send_error(
                    &mut client,
                    503,
                    "Service Unavailable",
                    &format!("Strict mode blocked the route to {host}:{port}."),
                )
                .await;
                return Ok(());
            }
            match direct_connect(&host, port).await {
                Ok(sock) => {
                    ctx.record(
                        "fallback",
                        &format!(
                            "HTTP {host}:{port} used the direct route ({})",
                            err.user_message()
                        ),
                        &host,
                        "",
                    );
                    sock
                }
                Err(err2) => {
                    ctx.record(
                        "error",
                        &format!("HTTP {host}:{port}: {}", err2.user_message()),
                        &host,
                        "",
                    );
                    send_error(
                        &mut client,
                        503,
                        "Service Unavailable",
                        &format!(
                            "Opencode Egress Manager could not reach {host}:{port}. {}",
                            err2.user_message()
                        ),
                    )
                    .await;
                    return Ok(());
                }
            }
        }
    };
    let edge = ctx.edge.current_edge().await;
    let mut request = format!("{method} {path} {proto}\r\n");
    for header in &headers {
        request.push_str(header);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    upstream.write_all(request.as_bytes()).await?;
    if content_length > 0 {
        let mut remaining = content_length;
        let mut buffer = leftover;
        while remaining > 0 {
            if !buffer.is_empty() {
                let take = buffer.len().min(remaining);
                upstream.write_all(&buffer[..take]).await?;
                remaining -= take;
                buffer.drain(..take);
                continue;
            }
            let mut chunk = vec![0u8; remaining.min(65536)];
            let n = client.read(&mut chunk).await?;
            if n == 0 {
                break;
            }
            upstream.write_all(&chunk[..n]).await?;
            remaining = remaining.saturating_sub(n);
        }
    }
    ctx.active_tunnels.fetch_add(1, Ordering::SeqCst);
    relay(ctx.clone(), client, upstream, &host, &edge).await;
    ctx.active_tunnels.fetch_sub(1, Ordering::SeqCst);
    Ok(())
}

async fn read_headers(stream: &mut TcpStream, max: usize) -> Result<(String, Vec<u8>)> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_header_end(&buf) {
            let leftover = buf[pos..].to_vec();
            let head = String::from_utf8_lossy(&buf[..pos]).to_string();
            return Ok((head.trim_end_matches(['\r', '\n']).to_string(), leftover));
        }
        if buf.len() > max {
            return Err(AppError::Process("request headers too large".into()));
        }
    }
    Err(AppError::Process(
        "client closed before request completed".into(),
    ))
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn parse_connect_target(target: &str) -> Result<(String, u16)> {
    if let Some((host, port)) = target.rsplit_once(':') {
        let port: u16 = port
            .parse()
            .map_err(|_| AppError::validation("invalid CONNECT port"))?;
        Ok((host.trim_matches(['[', ']']).to_string(), port))
    } else {
        Ok((target.to_string(), 443))
    }
}

fn parse_absolute_target(target: &str) -> Result<(String, u16, String)> {
    let parsed =
        url::Url::parse(target).map_err(|_| AppError::validation("invalid absolute target URL"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| AppError::validation("target URL has no host"))?
        .to_ascii_lowercase();
    let port = parsed.port_or_known_default().unwrap_or(80);
    let mut path = parsed.path().to_string();
    if let Some(query) = parsed.query() {
        path.push('?');
        path.push_str(query);
    }
    Ok((host, port, path))
}

async fn direct_connect(host: &str, port: u16) -> Result<TcpStream> {
    let targets = resolve_targets(host, port).await;
    let mut last = AppError::Process(format!("could not resolve {host}:{port}"));
    for addr in targets {
        match tokio::time::timeout(Duration::from_secs(8), TcpStream::connect(addr)).await {
            Ok(Ok(sock)) => return Ok(sock),
            Ok(Err(err)) => last = AppError::Process(err.to_string()),
            Err(_) => last = AppError::Process("direct connect timed out".into()),
        }
    }
    Err(last)
}

async fn resolve_targets(host: &str, port: u16) -> Vec<SocketAddr> {
    use std::net::ToSocketAddrs;
    let mut out = Vec::new();
    if let Ok(addrs) = (host, port).to_socket_addrs() {
        out.extend(addrs);
    }
    out
}

async fn open_upstream(ctx: Arc<ProxyCtx>, host: &str, port: u16) -> Result<TcpStream> {
    let clean = host.trim_matches(['[', ']']).to_ascii_lowercase();
    if clean == "127.0.0.1" || clean == "localhost" || clean == "::1" {
        return direct_connect(&clean, port).await;
    }
    let cfg = ctx.config().await;
    if matches!(
        cfg.mode,
        crate::config::EgressMode::Tor | crate::config::EgressMode::Custom
    ) {
        let ctx_inner = ctx.clone();
        let result = chain::open_multi_egress_upstream(&ctx.paths, &clean, port, move |h, p| {
            let ctx = ctx_inner.clone();
            async move { open_warp_direct(ctx, &h, p).await }
        })
        .await;
        return match result {
            Ok(sock) => Ok(sock),
            Err(err) => {
                if chain::strict_mode(&ctx.paths) {
                    return Err(err);
                }
                open_warp_direct(ctx.clone(), &clean, port).await
            }
        };
    }
    open_warp_direct(ctx, &clean, port).await
}

async fn open_warp_direct(ctx: Arc<ProxyCtx>, host: &str, port: u16) -> Result<TcpStream> {
    let cfg = ctx.config().await;
    let scope = cfg.warp_scope;
    let strict = chain::strict_mode(&ctx.paths);

    if scope == crate::config::WarpScope::Isolated {
        if let Ok(sock) = socks::open_socks5_tunnel(
            host,
            port,
            "127.0.0.1",
            crate::config::WARP_ISOLATED_SOCKS_PORT,
            Duration::from_secs(8),
            "",
            "",
        )
        .await
        {
            return Ok(sock);
        }
    }

    if is_pinned_host(host) {
        match open_pinned_any(ctx.clone(), host, port).await {
            Ok(sock) => return Ok(sock),
            Err(err) => {
                ctx.record(
                    "connect_fail",
                    &format!("pinned route failed: {}", err.user_message()),
                    host,
                    "",
                );
            }
        }
        if let Ok(sock) = socks::open_socks5_tunnel(
            host,
            port,
            "127.0.0.1",
            crate::config::WARP_ISOLATED_SOCKS_PORT,
            Duration::from_secs(6),
            "",
            "",
        )
        .await
        {
            return Ok(sock);
        }
    }

    if let Some(local_v6) = networking::warp_local_v6() {
        let addrs = networking::resolve_v6(host, port);
        if let Some(dest) = addrs.first() {
            if let Ok(sock) = tokio::time::timeout(
                Duration::from_secs(12),
                edge::open_pinned_v6(&dest.to_string(), port, &local_v6),
            )
            .await
            .unwrap_or_else(|_| Err(std::io::Error::other("pinned connect timed out")))
            {
                return Ok(sock);
            }
        }
    }

    if strict {
        return Err(AppError::Process(
            "Cloudflare WARP tunnel unavailable (fail-closed strict mode)".into(),
        ));
    }

    if let Some(target) = tor::active_target().await {
        if let Ok(sock) = socks::open_socks5_tunnel(
            host,
            port,
            &target.0,
            target.1,
            Duration::from_secs(10),
            "",
            "",
        )
        .await
        {
            if is_pinned_host(host) {
                ctx.record(
                    "fallback",
                    "WARP route unavailable; OpenCode used the Tor fallback",
                    host,
                    "",
                );
            }
            return Ok(sock);
        }
    }

    if is_pinned_host(host) {
        let edge = ctx.edge.current_edge().await;
        if edge.contains(':') {
            if let Ok(dest) = edge.parse::<std::net::Ipv6Addr>() {
                if let Ok(sock) = tokio::time::timeout(
                    Duration::from_secs(3),
                    TcpStream::connect(SocketAddr::from((dest, port))),
                )
                .await
                .unwrap_or_else(|_| Err(std::io::Error::other("edge connect timed out")))
                {
                    return Ok(sock);
                }
            }
        }
        ctx.record(
            "fallback",
            &format!(
                "pinned IPv6 edge unavailable; OpenCode used the fallback route ({})",
                scope.as_str()
            ),
            host,
            "",
        );
    }

    let sock = direct_connect(host, port).await?;
    Ok(sock)
}

async fn open_pinned_any(ctx: Arc<ProxyCtx>, host: &str, port: u16) -> Result<TcpStream> {
    let (mut local_v6, _) = ctx.edge.get(&ctx.paths, true).await;
    let mut last_err = AppError::Process("no Cloudflare edge answered".into());
    for attempt in 0..2 {
        let mut stale = false;
        for edge in ctx.edge.edges_to_try().await {
            match tokio::time::timeout(
                edge::edge_connect_timeout(),
                edge::open_pinned_v6(&edge, port, local_v6.as_deref().unwrap_or("")),
            )
            .await
            {
                Ok(Ok(sock)) => {
                    ctx.edge.mark_ok(&edge, &ctx.paths).await;
                    return Ok(sock);
                }
                Ok(Err(err)) => {
                    last_err = AppError::Process(err.to_string());
                    if is_stale_bind(&err) {
                        stale = true;
                        break;
                    }
                    ctx.edge.mark_failed(&edge, &ctx.paths).await;
                    ctx.record(
                        "edge_fail",
                        &format!("{}: {}", kind_name(&err), redact(&err.to_string())),
                        host,
                        &edge,
                    );
                }
                Err(_) => {
                    last_err = AppError::Process("edge connect timed out".into());
                    ctx.edge.mark_failed(&edge, &ctx.paths).await;
                }
            }
        }
        if !stale || attempt == 1 {
            break;
        }
        ctx.edge.invalidate().await;
        let (fresh, _) = ctx.edge.get(&ctx.paths, true).await;
        match fresh {
            Some(v) if Some(&v) != local_v6.as_ref() => {
                local_v6 = Some(v);
                ctx.record("repair", "local WARP address changed; retrying", host, "");
            }
            _ => break,
        }
    }
    Err(last_err)
}

fn is_stale_bind(err: &std::io::Error) -> bool {
    if err.raw_os_error() == Some(10049) {
        return true;
    }
    err.kind() == std::io::ErrorKind::AddrNotAvailable
}

fn kind_name(err: &std::io::Error) -> &'static str {
    match err.kind() {
        std::io::ErrorKind::ConnectionRefused => "ConnectionRefused",
        std::io::ErrorKind::TimedOut => "TimedOut",
        std::io::ErrorKind::AddrNotAvailable => "AddrNotAvailable",
        _ => "IoError",
    }
}

enum RelayEnd {
    Eof,
    Reset(bool), // true = saw data first
    Failed,
}

async fn copy_direction(
    reader: &mut tokio::net::tcp::OwnedReadHalf,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
) -> RelayEnd {
    use tokio::io::AsyncWriteExt;
    let mut buf = vec![0u8; 65536];
    let mut saw_data = false;
    loop {
        match reader.read(&mut buf).await {
            Ok(0) => return RelayEnd::Eof,
            Ok(n) => {
                saw_data = true;
                if writer.write_all(&buf[..n]).await.is_err() {
                    return RelayEnd::Failed;
                }
            }
            Err(err) => {
                return if is_reset(&err) {
                    RelayEnd::Reset(saw_data)
                } else {
                    RelayEnd::Failed
                };
            }
        }
    }
}

fn is_reset(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
    ) || err
        .raw_os_error()
        .is_some_and(|code| code == 10053 || code == 10054)
}

async fn relay(ctx: Arc<ProxyCtx>, client: TcpStream, upstream: TcpStream, host: &str, edge: &str) {
    let (mut cr, mut cw) = client.into_split();
    let (mut ur, mut uw) = upstream.into_split();
    let up = copy_direction(&mut ur, &mut cw);
    let down = copy_direction(&mut cr, &mut uw);
    let (up_end, down_end) = tokio::join!(up, down);
    match (&up_end, &down_end) {
        (RelayEnd::Reset(true), _) => {
            ctx.mark_stream_reset(host, "upstream reset after response", edge)
                .await;
        }
        (_, RelayEnd::Reset(true)) if matches!(up_end, RelayEnd::Reset(true) | RelayEnd::Eof) => {
            ctx.mark_stream_reset(host, "client reset after response", edge)
                .await;
        }
        (RelayEnd::Reset(false), _) => {
            ctx.record("reset", "upstream reset before response", host, edge);
        }
        (_, RelayEnd::Reset(false)) => {
            ctx.record("reset", "connection reset before response", host, edge);
        }
        _ => {}
    }
}

async fn send_error(client: &mut TcpStream, code: u16, reason: &str, detail: &str) {
    let reason = reason.replace(['\r', '\n'], " ");
    let detail = detail.replace(['\r', '\n'], " ");
    let text = if detail.is_empty() {
        reason.clone()
    } else {
        format!("{reason}\n{detail}")
    };
    let body = text.as_bytes();
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = client.write_all(head.as_bytes()).await;
    let _ = client.write_all(body).await;
    let _ = client.flush().await;
}

trait ChooseRandom<T: Clone> {
    fn choose_random(&self) -> Option<String>;
}
impl ChooseRandom<String> for Vec<String> {
    fn choose_random(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut rng = rand::rng();
        Some(self[rng.random_range(0..self.len())].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_connect_targets() {
        assert_eq!(
            parse_connect_target("opencode.ai:443").unwrap(),
            ("opencode.ai".into(), 443)
        );
        assert_eq!(
            parse_connect_target("[2606:4700::1]:8443").unwrap(),
            ("2606:4700::1".into(), 8443)
        );
        assert_eq!(
            parse_connect_target("example.com").unwrap(),
            ("example.com".into(), 443)
        );
    }

    #[test]
    fn finds_header_end() {
        assert_eq!(
            find_header_end(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n"),
            Some(27)
        );
        assert_eq!(find_header_end(b"partial"), None);
    }

    #[test]
    fn identifies_resets() {
        let err = std::io::Error::from_raw_os_error(10054);
        assert!(is_reset(&err));
        assert!(!is_reset(&std::io::Error::other("nope")));
    }
}
