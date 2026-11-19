//! Cloudflare edge selection and failure accounting.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use rand::RngExt;
use tokio::sync::Mutex;

use crate::{config::Paths, services::networking};

const EDGE_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const EDGE_FAILURE_COOLDOWN: Duration = Duration::from_secs(120);
const CACHE_TTL: Duration = Duration::from_millis(1500);
const STREAM_RESET_WINDOW: Duration = Duration::from_secs(300);

/// Tracks the active Cloudflare edge, the candidate pool, and failure cooldowns.
pub struct EdgeState {
    inner: Mutex<Inner>,
}

struct Inner {
    pool: Vec<String>,
    edge: String,
    reroll_mtime: f64,
    bad_edges: HashMap<String, Instant>,
    stream_fails: HashMap<String, Vec<Instant>>,
    last_prefix: String,
    fetched_at: Instant,
    cached_v6: Option<String>,
}

impl EdgeState {
    /// Create edge state from a candidate pool and the initially selected edge.
    pub fn new(pool: Vec<String>, initial_edge: String) -> Self {
        Self {
            inner: Mutex::new(Inner {
                pool,
                edge: initial_edge,
                reroll_mtime: 0.0,
                bad_edges: HashMap::new(),
                stream_fails: HashMap::new(),
                last_prefix: String::new(),
                fetched_at: Instant::now() - CACHE_TTL - Duration::from_secs(1),
                cached_v6: None,
            }),
        }
    }

    /// Return the currently selected edge address.
    pub async fn current_edge(&self) -> String {
        self.inner.lock().await.edge.clone()
    }

    /// Refresh pinned-edge control files (called by the server loop).
    pub async fn poll_controls(&self, paths: &Paths) {
        let mut inner = self.inner.lock().await;
        if let Some(value) = crate::config::read_text(&paths.edge_override_file()) {
            let value = value.to_ascii_lowercase();
            if value != "auto" && value.contains(':') {
                inner.edge = value.clone();
                let _ = crate::config::write_text(&paths.last_edge_file(), &(value + "\n"));
                return;
            }
        }
        let reroll_path = paths.edge_reroll_file();
        if let Ok(meta) = std::fs::metadata(&reroll_path) {
            if let Ok(modified) = meta.modified() {
                let mtime = modified
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0);
                if mtime > inner.reroll_mtime {
                    inner.reroll_mtime = mtime;
                    let fresh = networking::fetch_opencode_aaaa_pool().await;
                    if !fresh.is_empty() {
                        inner.pool = fresh;
                        let choices: Vec<&String> =
                            inner.pool.iter().filter(|e| **e != inner.edge).collect();
                        if let Some(choice) = choices.choose() {
                            inner.edge = (*choice).clone();
                            let _ = crate::config::write_text(
                                &paths.last_edge_file(),
                                &(inner.edge.clone() + "\n"),
                            );
                        }
                    }
                }
            }
        }
    }

    fn pick_locked(inner: &Inner, exclude: &str) -> String {
        let now = Instant::now();
        let is_bad = |edge: &str| {
            inner
                .bad_edges
                .iter()
                .any(|(bad, at)| bad == edge && now.duration_since(*at) < EDGE_FAILURE_COOLDOWN)
        };
        let choices: Vec<&String> = inner
            .pool
            .iter()
            .filter(|e| e.as_str() != exclude && !is_bad(e))
            .collect();
        let candidates = if choices.is_empty() {
            inner
                .pool
                .iter()
                .filter(|e| e.as_str() != exclude)
                .collect::<Vec<_>>()
        } else {
            choices
        };
        if candidates.is_empty() {
            return inner.edge.clone();
        }
        let mut rng = rand::rng();
        candidates[rng.random_range(0..candidates.len())].clone()
    }

    /// Edges to attempt for a new connection, current edge first.
    pub async fn edges_to_try(&self) -> Vec<String> {
        let inner = self.inner.lock().await;
        let now = Instant::now();
        let is_bad = |edge: &str| {
            inner
                .bad_edges
                .iter()
                .any(|(bad, at)| bad == edge && now.duration_since(*at) < EDGE_FAILURE_COOLDOWN)
        };
        let primary = inner.edge.clone();
        let mut healthy: Vec<String> = inner
            .pool
            .iter()
            .filter(|e| **e != primary && !is_bad(e))
            .cloned()
            .collect();
        let mut choices = if primary.is_empty() {
            healthy
        } else if is_bad(&primary) && !healthy.is_empty() {
            std::mem::take(&mut healthy)
        } else {
            let mut c = vec![primary.clone()];
            c.append(&mut healthy);
            c
        };
        if choices.is_empty() {
            choices = inner
                .pool
                .iter()
                .filter(|e| **e != primary)
                .cloned()
                .collect();
            if choices.is_empty() {
                choices.push(primary);
            }
        }
        choices.truncate(3);
        choices
    }

    /// Record a successful connection, adopting `edge` as current when it differs.
    pub async fn mark_ok(&self, edge: &str, paths: &Paths) {
        let mut inner = self.inner.lock().await;
        inner.bad_edges.remove(edge);
        if !edge.is_empty() && edge != inner.edge {
            inner.edge = edge.to_string();
            let _ = crate::config::write_text(&paths.last_edge_file(), &(edge.to_string() + "\n"));
        }
    }

    /// Record a failed connection, picking a replacement if the current edge failed.
    pub async fn mark_failed(&self, edge: &str, paths: &Paths) {
        let mut inner = self.inner.lock().await;
        inner.bad_edges.insert(edge.to_string(), Instant::now());
        if edge == inner.edge {
            inner.edge = Self::pick_locked(&inner, edge);
            let _ =
                crate::config::write_text(&paths.last_edge_file(), &(inner.edge.clone() + "\n"));
        }
    }

    /// Retire an edge after a second live-stream reset inside five minutes.
    pub async fn note_stream_reset(&self, edge: &str) -> Option<String> {
        if edge.is_empty() {
            return None;
        }
        let mut inner = self.inner.lock().await;
        let now = Instant::now();
        let stamps: Vec<Instant> = inner
            .stream_fails
            .get(edge)
            .map(|v| {
                v.iter()
                    .filter(|t| now.duration_since(**t) < STREAM_RESET_WINDOW)
                    .copied()
                    .collect()
            })
            .unwrap_or_default();
        let mut stamps = stamps;
        stamps.push(now);
        inner.stream_fails.insert(edge.to_string(), stamps.clone());
        if stamps.len() < 2 || edge != inner.edge {
            return None;
        }
        let replacement = Self::pick_locked(&inner, edge);
        if replacement.is_empty() || replacement == edge {
            return None;
        }
        inner.edge = replacement.clone();
        inner.bad_edges.insert(edge.to_string(), now);
        Some(replacement)
    }

    /// Force the next [`EdgeState::get`] to refresh the cached IPv6 and pool.
    pub async fn invalidate(&self) {
        let mut inner = self.inner.lock().await;
        inner.fetched_at = Instant::now() - CACHE_TTL - Duration::from_secs(1);
        inner.cached_v6 = None;
    }

    /// Cached WARP IPv6 + current edge. Waits briefly while a rotation is pending.
    pub async fn get(&self, paths: &Paths, wait_if_rotating: bool) -> (Option<String>, String) {
        let deadline = Instant::now()
            + if wait_if_rotating {
                Duration::from_secs(18)
            } else {
                Duration::ZERO
            };
        loop {
            {
                let inner = self.inner.lock().await;
                if inner.cached_v6.is_some() && inner.fetched_at.elapsed() < CACHE_TTL {
                    let v6 = inner.cached_v6.clone();
                    let edge = inner.edge.clone();
                    drop(inner);
                    return (v6, edge);
                }
            }
            let v6 = networking::warp_local_v6();
            let mut refresh_pool = false;
            {
                let mut inner = self.inner.lock().await;
                inner.fetched_at = Instant::now();
                inner.cached_v6 = v6.clone();
                if let Some(addr) = &v6 {
                    let prefix = networking::normalize_v6_64(addr);
                    if !prefix.is_empty()
                        && !inner.last_prefix.is_empty()
                        && prefix != inner.last_prefix
                    {
                        inner.last_prefix = prefix;
                        refresh_pool = true;
                    } else if !prefix.is_empty() {
                        inner.last_prefix = prefix;
                    }
                }
            }
            if refresh_pool {
                // Refresh outside the lock: this is a network call.
                let fresh = networking::fetch_opencode_aaaa_pool().await;
                if !fresh.is_empty() {
                    let mut inner = self.inner.lock().await;
                    inner.pool = fresh;
                    if let Some(choice) = inner.pool.choose() {
                        inner.edge = choice.clone();
                        let _ = crate::config::write_text(
                            &paths.last_edge_file(),
                            &(inner.edge.clone() + "\n"),
                        );
                    }
                }
            }
            self.poll_controls(paths).await;
            let edge = self.current_edge().await;
            if v6.is_some() {
                return (v6, edge);
            }
            if !wait_if_rotating || !crate::config::is_paused(paths) || Instant::now() >= deadline {
                return (None, edge);
            }
            tokio::time::sleep(Duration::from_millis(750)).await;
        }
    }
}

/// Connect to an edge explicitly bound to the given local IPv6 address.
pub async fn open_pinned_v6(
    dest_ip: &str,
    port: u16,
    local_v6: &str,
) -> std::io::Result<tokio::net::TcpStream> {
    use std::net::SocketAddrV6;
    let local: std::net::Ipv6Addr = local_v6
        .split('%')
        .next()
        .unwrap_or(local_v6)
        .parse()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "bad local IPv6"))?;
    let dest: std::net::Ipv6Addr = dest_ip
        .split('%')
        .next()
        .unwrap_or(dest_ip)
        .parse()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "bad edge IPv6"))?;
    let socket = tokio::net::TcpSocket::new_v6()?;
    socket.bind(SocketAddrV6::new(local, 0, 0, 0).into())?;
    socket
        .connect(SocketAddrV6::new(dest, port, 0, 0).into())
        .await
}

trait ChooseRandom<T: Clone> {
    fn choose(&self) -> Option<T>;
}
impl<T: Clone> ChooseRandom<T> for Vec<T> {
    fn choose(&self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        let mut rng = rand::rng();
        Some(self[rng.random_range(0..self.len())].clone())
    }
}

/// Timeout applied to each pinned edge connect attempt.
pub fn edge_connect_timeout() -> Duration {
    EDGE_CONNECT_TIMEOUT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_avoids_excluded_and_bad() {
        let mut inner = Inner {
            pool: vec!["a".into(), "b".into(), "c".into()],
            edge: "a".into(),
            reroll_mtime: 0.0,
            bad_edges: HashMap::new(),
            stream_fails: HashMap::new(),
            last_prefix: String::new(),
            fetched_at: Instant::now(),
            cached_v6: None,
        };
        inner.bad_edges.insert("a".into(), Instant::now());
        let picked = EdgeState::pick_locked(&inner, "a");
        assert_ne!(picked, "a");
        assert!(inner.pool.contains(&picked));
    }
}
