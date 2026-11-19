//! Background worker roles (keeper and guard), run by a detached process.
//!
//! These are the durable counterparts of the old pythonw workers: they survive the
//! UI closing because Scheduled Tasks launch them at logon. IPC is file/heartbeat/
//! mutex based, exactly like the audited design, so no privileged channel is added.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio_util::sync::CancellationToken;

use crate::{
    config::{EgressConfig, GuardState, Paths, SERVICE_PROXY_PORT_DEFAULT},
    errors::{redact, Result},
    platform,
    services::{networking, opencode, proxy, tor, warp},
};

const KEEPER_TICK: Duration = Duration::from_secs(45);
const GUARD_POLL: Duration = Duration::from_secs(10);
const GUARD_COOLDOWN: i64 = 75;
const MAX_HOP_TRIES: u32 = 4;
const REWALL_SECONDS: i64 = 180;
const FAILED_HOPS_MAX: u32 = 2;
const BACKOFF_SECONDS: i64 = 120;
const ROTATION_RETRY_SECONDS: i64 = 600;
const FREE_WALL_HOPS_MAX: u32 = 2;
const MAX_READ_BYTES: usize = 4_000_000;

/// Entry point for `--worker keeper|guard`.
pub async fn run_role(role: &str) -> i32 {
    let paths = Paths::discover();
    if let Err(err) = paths.ensure_dirs() {
        eprintln!("worker cannot create state directories: {err}");
        return 1;
    }
    let code = match role {
        "keeper" => keeper_main(&paths).await,
        "guard" => guard_main(&paths).await,
        other => {
            eprintln!("unknown worker role: {other}");
            2
        }
    };
    code
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Keeper
// ---------------------------------------------------------------------------

async fn keeper_main(paths: &Paths) -> i32 {
    let Some(_mutex) = platform::lock_mutex("OpencodeEgressManagerKeeper") else {
        return 0;
    };
    let cancel = CancellationToken::new();
    let mut ticker = tokio::time::interval(KEEPER_TICK);
    ticker.tick().await; // consume the immediate first tick

    let warp_bin = warp::find_warp_cli();
    crate::config::append_log(
        &paths.keeper_log_file(),
        &format!(
            "keeper up (tick=45s, warp={})",
            if warp_bin.is_some() { "found" } else { "none" }
        ),
        1_048_576,
    );
    crate::config::heartbeat(paths, true);

    // Host the shared service proxy used by OpenCode Desktop/CLI.
    let mut service_proxy =
        if networking::port_open("127.0.0.1", SERVICE_PROXY_PORT_DEFAULT, 400).await {
            None
        } else {
            match proxy::server::start_proxy_server(paths, SERVICE_PROXY_PORT_DEFAULT).await {
                Ok(server) => Some(server),
                Err(err) => {
                    crate::config::append_log(
                        &paths.keeper_log_file(),
                        &format!("service proxy bind failed: {}", err.user_message()),
                        1_048_576,
                    );
                    None
                }
            }
        };

    let watchdog_cancel = cancel.clone();
    let watchdog_paths = paths.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(3));
        loop {
            tokio::select! {
                _ = watchdog_cancel.cancelled() => break,
                _ = interval.tick() => {
                    if !networking::port_open("127.0.0.1", SERVICE_PROXY_PORT_DEFAULT, 400).await {
                        if let Ok(_server) = proxy::server::start_proxy_server(&watchdog_paths, SERVICE_PROXY_PORT_DEFAULT).await { crate::config::append_log(
                            &watchdog_paths.keeper_log_file(),
                            "service proxy rebound",
                            1_048_576,
                        ) }
                    }
                }
            }
        }
    });

    // Allow interface initialization after boot.
    if let Some(bin) = &warp_bin {
        for _ in 0..6 {
            if warp::run(bin, &["status"], 8)
                .await
                .map(|(code, _)| code == 0)
                .unwrap_or(false)
            {
                break;
            }
            tokio::time::sleep(Duration::from_secs(8)).await;
        }
    }

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::signal::ctrl_c() => break,
            _ = ticker.tick() => {
                crate::config::heartbeat(paths, true);
                if service_proxy.is_none()
                    && !networking::port_open("127.0.0.1", SERVICE_PROXY_PORT_DEFAULT, 400).await
                {
                    service_proxy = proxy::server::start_proxy_server(paths, SERVICE_PROXY_PORT_DEFAULT).await.ok();
                }
                // The legacy Python keeper owns the shared WARP tunnel and Tor
                // daemon while it runs; in that case this keeper only hosts the
                // proxy and leaves WARP/Tor management to it.
                if !platform::mutex_held("OpencodeUnlimitedKeeper") {
                    let cfg = EgressConfig::load(paths).unwrap_or_default();
                    match cfg.mode {
                        crate::config::EgressMode::Warp | crate::config::EgressMode::Clean => {
                            if let Some(bin) = &warp_bin {
                                if !crate::config::is_paused(paths) {
                                    warp::ensure_connected(bin, paths).await;
                                }
                            }
                        }
                        crate::config::EgressMode::Tor => {
                            maintain_tor(paths).await;
                        }
                        crate::config::EgressMode::Custom => {}
                    }
                }
            }
        }
    }

    if let Some(proxy) = service_proxy {
        proxy.stop().await;
    }
    crate::config::append_log(&paths.keeper_log_file(), "keeper stopped", 1_048_576);
    0
}

async fn maintain_tor(paths: &Paths) {
    if tor::active_target().await.is_some() {
        return;
    }
    if tor::portable_running(paths) {
        // A live process that has not opened SOCKS after a generous window is stuck.
        if let Some(_pid) = tor::portable_pid(paths) {
            let stale = std::fs::metadata(paths.tor_dir.join("tor.pid"))
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|m| {
                    SystemTime::now()
                        .duration_since(m)
                        .map(|d| d.as_secs() > 240)
                        .unwrap_or(false)
                })
                .unwrap_or(false);
            if stale {
                crate::config::append_log(
                    &paths.keeper_log_file(),
                    "Tor SOCKS port did not open within 240s - restarting Tor",
                    1_048_576,
                );
                let _ = tor::stop_portable_tor(paths).await;
                let _ = tor::start_portable_tor(paths).await;
            }
        }
        return;
    }
    let _ = tor::start_portable_tor(paths).await;
}

// ---------------------------------------------------------------------------
// Guard
// ---------------------------------------------------------------------------

async fn guard_main(paths: &Paths) -> i32 {
    // Two quota-wall guards would both rotate WARP and churn the tunnel. If the
    // legacy Python guard is running, stand down and let it own rotation.
    if platform::mutex_held("OpencodeUnlimitedGuard") {
        crate::config::append_log(
            &paths.guard_log_file(),
            "Opencode Unlimited guard is running - standing down",
            1_048_576,
        );
        return 0;
    }
    let Some(_mutex) = platform::lock_mutex("OpencodeEgressManagerGuard") else {
        return 0;
    };
    let cancel = CancellationToken::new();
    let mut state = GuardState::load(paths).unwrap_or_else(|_| {
        let offset = std::fs::metadata(Paths::opencode_log())
            .map(|m| m.len())
            .unwrap_or(0);
        GuardState {
            offset,
            ..Default::default()
        }
    });
    crate::config::append_log(
        &paths.guard_log_file(),
        &format!(
            "guard up (poll=10s, cooldown={GUARD_COOLDOWN}s, warp={})",
            if warp::find_warp_cli().is_some() {
                "found"
            } else {
                "none"
            }
        ),
        1_048_576,
    );
    crate::config::heartbeat(paths, false);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::signal::ctrl_c() => break,
            _ = tokio::time::sleep(GUARD_POLL) => {
                crate::config::heartbeat(paths, false);
                if let Err(err) = guard_tick(paths, &mut state).await {
                    crate::config::append_log(
                        &paths.guard_log_file(),
                        &format!("guard error: {}", redact(&err.to_string())),
                        1_048_576,
                    );
                }
            }
        }
    }
    crate::config::append_log(&paths.guard_log_file(), "guard stopped", 1_048_576);
    0
}

fn wall_patterns() -> Vec<&'static str> {
    vec![
        "quotaexceeded",
        "quota exceeded",
        "quota_exceeded",
        "free usage limit",
        "free usage exceeded",
        "free-tier usage limit",
        "usage limit reached",
        "exceeded your current quota",
        "credits exhausted",
        "rate limit exceeded",
        "rate_limit_exceeded",
        "rate limited",
        "ratelimited",
        "resource_exhausted",
        "429 too many requests",
        "status=429",
        "status: 429",
        "status 429",
        "code=429",
    ]
}

fn session_id_in(line: &str) -> String {
    let re = regex::Regex::new(r"\bsessionID=(ses_[A-Za-z0-9]+)").unwrap();
    re.captures(line)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_default()
}

/// Read newly appended log bytes and detect quota walls.
fn read_new_bytes(state: &mut GuardState, paths: &Paths) -> (bool, Vec<String>) {
    let path = Paths::opencode_log();
    let Ok(meta) = std::fs::metadata(&path) else {
        return (false, Vec::new());
    };
    let size = meta.len();
    let mut offset = state.offset;
    if size < offset {
        offset = 0;
    }
    let start = offset.max(size.saturating_sub(MAX_READ_BYTES as u64));
    let delta = size - start;
    if delta == 0 {
        return (false, Vec::new());
    }
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut fh) = std::fs::File::open(&path) else {
        return (false, Vec::new());
    };
    if fh.seek(SeekFrom::Start(start)).is_err() {
        return (false, Vec::new());
    }
    let mut buf = Vec::new();
    if fh.read_to_end(&mut buf).is_err() {
        return (false, Vec::new());
    }
    state.offset = size;
    let text = String::from_utf8_lossy(&buf).to_string();
    let lower = text.to_ascii_lowercase();
    let patterns = wall_patterns();
    let wall = patterns.iter().any(|p| lower.contains(p));
    let mut sessions = Vec::new();
    if wall {
        for line in text.lines() {
            let low = line.to_ascii_lowercase();
            if patterns.iter().any(|p| low.contains(p)) {
                let sid = session_id_in(line);
                if !sid.is_empty() && !sessions.contains(&sid) {
                    sessions.push(sid);
                }
            }
        }
    }
    if wall {
        state.wall_pending = true;
        state.wall_seen = now_epoch();
        if lower.contains("free usage limit") {
            state.last_free_wall = state.wall_seen;
        }
    }
    let _ = state.save(paths);
    (wall, sessions)
}

fn next_utc_midnight(now: i64) -> i64 {
    let secs = now.rem_euclid(86400);
    now - secs + 86400
}

/// Update the rotation-pause state machine. Returns true while paused.
fn note_wall(state: &mut GuardState, paths: &Paths, now: i64) -> bool {
    if state.backoff_until > 0 && now >= state.backoff_until {
        state.backoff_until = 0;
        state.failed_hops = 0;
        state.free_wall_hops = 0;
        state.free_wall_until = 0;
        crate::config::append_log(
            &paths.guard_log_file(),
            "rotation pause is over - trying again on the next limit",
            1_048_576,
        );
    }
    if now < state.backoff_until {
        if now - state.backoff_logged >= 60 {
            state.backoff_logged = now;
            let left = (state.backoff_until - now).max(0) / 60;
            crate::config::append_log(
                &paths.guard_log_file(),
                &format!("limit seen again, but auto-rotation is paused for ~{left} more min"),
                1_048_576,
            );
        }
        return true;
    }

    let last_hop = state.last_ok_hop;
    if last_hop > 0 && now - last_hop <= REWALL_SECONDS {
        if state.hop_counted != last_hop {
            state.hop_counted = last_hop;
            state.failed_hops += 1;
        }
    } else if last_hop > 0 {
        state.failed_hops = 0;
    }

    let free_wall = state.last_free_wall;
    let fresh_free =
        last_hop > 0 && free_wall > 0 && (0..=REWALL_SECONDS).contains(&(free_wall - last_hop));
    if fresh_free {
        if state.free_wall_counted != last_hop {
            state.free_wall_counted = last_hop;
            state.free_wall_hops += 1;
        }
    } else {
        state.free_wall_hops = 0;
    }

    if state.free_wall_hops >= FREE_WALL_HOPS_MAX {
        let until = next_utc_midnight(now);
        state.free_wall_until = until;
        state.backoff_until = until;
        state.backoff_logged = now;
        state.hop_counted = last_hop;
        crate::config::append_log(
            &paths.guard_log_file(),
            &format!(
                "free limit is not following the IP: {FREE_WALL_HOPS_MAX} confirmed new IPv6 \
                 addresses hit it again. Pausing rotation until 00:00 UTC."
            ),
            1_048_576,
        );
        return true;
    }

    if state.failed_hops >= FAILED_HOPS_MAX {
        state.backoff_until = now + BACKOFF_SECONDS;
        state.backoff_logged = now;
        crate::config::append_log(
            &paths.guard_log_file(),
            &format!(
                "pausing auto-rotation for {} min: the last {FAILED_HOPS_MAX} new addresses hit \
                 the limit again immediately.",
                BACKOFF_SECONDS / 60
            ),
            1_048_576,
        );
    }
    let _ = state.save(paths);
    now < state.backoff_until
}

async fn guard_tick(paths: &Paths, state: &mut GuardState) -> Result<()> {
    // Fail-safe against a check/use race: if the legacy Python guard appears
    // after startup, stop rotating rather than fight it for WARP.
    if platform::mutex_held("OpencodeUnlimitedGuard") {
        return Ok(());
    }
    let now = now_epoch();
    if state.backoff_until > now {
        // Still paused: leave queued work alone.
        let _ = state.save(paths);
        return Ok(());
    }

    let (wall, sessions) = read_new_bytes(state, paths);
    if !wall && !state.wall_pending {
        return Ok(());
    }
    if wall {
        for sid in sessions {
            state.pending.insert(sid, now);
        }
        let _ = state.save(paths);
        if note_wall(state, paths, now) {
            return Ok(());
        }
    }

    let last_hop = state.last_hop;
    let last_ok = state.last_ok_hop;
    let seen = state.wall_seen;
    if seen > 0 && seen <= last_ok {
        let ids: Vec<String> = state.pending.keys().cloned().collect();
        for sid in ids {
            state.resumed.insert(sid.clone(), last_ok);
            state.pending.remove(&sid);
        }
        state.prune();
        state.wall_pending = false;
        let _ = state.save(paths);
        return Ok(());
    }

    let age = now - last_hop;
    if age >= GUARD_COOLDOWN {
        state.wall_pending = false;
        let _ = state.save(paths);
        if !rotate(paths, state).await? {
            state.wall_pending = true;
            let _ = state.save(paths);
        }
    } else {
        let fresh_ip = wall && last_ok >= last_hop;
        let resumable: Vec<String> = state
            .pending
            .keys()
            .filter(|sid| {
                let resumed_at = state.resumed.get(*sid).copied().unwrap_or(0);
                fresh_ip && resumed_at < last_ok
            })
            .cloned()
            .collect();
        if !resumable.is_empty() {
            crate::config::append_log(
                &paths.guard_log_file(),
                &format!(
                    "{} more chat(s) hit the limit around the last IP change - continuing them",
                    resumable.len()
                ),
                1_048_576,
            );
            dispatch_auto_resume(paths, state, Some(resumable)).await?;
        }
    }
    Ok(())
}

async fn rotate(paths: &Paths, state: &mut GuardState) -> Result<bool> {
    let now = now_epoch();
    let cfg = EgressConfig::load(paths)?;
    match cfg.mode {
        crate::config::EgressMode::Tor => {
            crate::config::append_log(
                &paths.guard_log_file(),
                "wall detected in Tor mode - rotating circuit",
                1_048_576,
            );
            let msg = tor::rotate_circuit(paths).await?;
            state.last_hop = now;
            state.hops += 1;
            if !msg.is_empty() {
                state.last_ok_hop = now;
            }
            let _ = state.save(paths);
            crate::config::append_log(
                &paths.guard_log_file(),
                &format!("Tor rotated: {msg}"),
                1_048_576,
            );
            if !msg.is_empty() {
                dispatch_auto_resume(paths, state, None).await?;
                return Ok(true);
            }
            Ok(false)
        }
        crate::config::EgressMode::Custom => {
            crate::config::append_log(
                &paths.guard_log_file(),
                "wall detected in Custom Proxy mode - cycling node",
                1_048_576,
            );
            let (ok, msg) = proxy::chain::rotate_custom_proxy(paths).await?;
            state.last_hop = now;
            state.hops += 1;
            if ok {
                state.last_ok_hop = now;
            }
            let _ = state.save(paths);
            crate::config::append_log(
                &paths.guard_log_file(),
                &format!("custom rotated: {msg}"),
                1_048_576,
            );
            if ok {
                dispatch_auto_resume(paths, state, None).await?;
            }
            Ok(ok)
        }
        crate::config::EgressMode::Warp | crate::config::EgressMode::Clean => {
            let Some(bin) = warp::find_warp_cli() else {
                crate::config::append_log(
                    &paths.guard_log_file(),
                    "wall detected but warp-cli is not installed",
                    1_048_576,
                );
                return Ok(false);
            };
            let old_v6 = networking::get_egress_ip(&cfg, 8)
                .await
                .or_else(networking::warp_local_v6)
                .unwrap_or_else(|| state.current.clone());
            let old_display = if old_v6.is_empty() {
                "?".to_string()
            } else {
                old_v6.clone()
            };
            let old_prefix = networking::normalize_v6_64(&old_v6);
            crate::config::append_log(
                &paths.guard_log_file(),
                &format!("wall detected - rotating WARP /64 (was: {old_display})"),
                1_048_576,
            );
            state.blacklist_prefix(&old_v6);
            let _ = state.save(paths);

            let avoid = state.blacklist.clone();
            let mut new_v6 = String::new();
            {
                let pause = crate::config::PauseGuard::acquire(paths, "zen-guard", 600);
                for attempt in 1..=MAX_HOP_TRIES {
                    let candidate = warp::hop_once(&bin, paths, 3, &avoid, &old_v6).await;
                    match candidate {
                        Some(cand) if networking::normalize_v6_64(&cand) != old_prefix => {
                            if state.is_burnt(&cand) {
                                crate::config::append_log(
                                    &paths.guard_log_file(),
                                    &format!(
                                        "try {attempt}: Cloudflare returned a burnt /64 again"
                                    ),
                                    1_048_576,
                                );
                                continue;
                            }
                            new_v6 = cand;
                            break;
                        }
                        Some(cand) => {
                            crate::config::append_log(
                                &paths.guard_log_file(),
                                &format!("try {attempt}: WARP returned the same /64 ({cand})"),
                                1_048_576,
                            );
                        }
                        None => {
                            crate::config::append_log(
                                &paths.guard_log_file(),
                                &format!("try {attempt}: rotation produced no fresh /64"),
                                1_048_576,
                            );
                        }
                    }
                }
                if new_v6.is_empty() {
                    if let Some(candidate) = warp::hop_once(&bin, paths, 3, &[], &old_v6).await {
                        new_v6 = candidate;
                    }
                }
                drop(pause);
            }

            state.last_hop = now;
            let same_address = !old_prefix.is_empty()
                && !new_v6.is_empty()
                && networking::normalize_v6_64(&new_v6) == old_prefix;
            if !new_v6.is_empty() && !same_address {
                state.current = new_v6.clone();
                state.hops += 1;
                state.last_ok_hop = now;
                state.failed_hops = 0;
                state.backoff_until = 0;
                state.hop_counted = 0;
                let _ = state.save(paths);
                crate::config::append_log(
                    &paths.guard_log_file(),
                    &format!("rotated #{}: {old_v6} -> {new_v6}", state.hops),
                    1_048_576,
                );
                let _ = crate::config::write_text(&paths.edge_reroll_file(), &now.to_string());
                dispatch_auto_resume(paths, state, None).await?;
                return Ok(true);
            }
            if same_address {
                state.failed_hops += 1;
                if state.failed_hops >= FAILED_HOPS_MAX {
                    state.backoff_until = now + BACKOFF_SECONDS;
                    state.backoff_logged = now;
                    crate::config::append_log(
                        &paths.guard_log_file(),
                        &format!(
                            "pausing auto-rotation for {} min: WARP kept returning the same /64.",
                            BACKOFF_SECONDS / 60
                        ),
                        1_048_576,
                    );
                }
                let _ = state.save(paths);
                return Ok(false);
            }
            state.backoff_until = now + ROTATION_RETRY_SECONDS;
            state.backoff_logged = now;
            let _ = state.save(paths);
            crate::config::append_log(
                &paths.guard_log_file(),
                &format!(
                    "rotation failed - no confirmed new IPv6 egress. Pausing retries for {} min.",
                    ROTATION_RETRY_SECONDS / 60
                ),
                1_048_576,
            );
            Ok(false)
        }
    }
}

async fn dispatch_auto_resume(
    paths: &Paths,
    state: &mut GuardState,
    session_ids: Option<Vec<String>>,
) -> Result<()> {
    let sids = session_ids.unwrap_or_else(|| {
        let last_ok = state.last_ok_hop;
        state
            .pending
            .keys()
            .filter(|sid| state.resumed.get(*sid).copied().unwrap_or(0) < last_ok)
            .cloned()
            .collect::<Vec<_>>()
    });
    let now = now_epoch();
    let result = opencode::resume_active_session(paths, Some(sids.clone())).await;
    match &result {
        Ok(msg) => crate::config::append_log(
            &paths.guard_log_file(),
            &format!("[auto-resume] {msg}"),
            1_048_576,
        ),
        Err(err) => crate::config::append_log(
            &paths.guard_log_file(),
            &format!("[auto-resume] note: {}", err.user_message()),
            1_048_576,
        ),
    }
    for sid in sids {
        state.resumed.insert(sid.clone(), now);
        state.pending.remove(&sid);
    }
    state.prune();
    let _ = state.save(paths);
    Ok(())
}
