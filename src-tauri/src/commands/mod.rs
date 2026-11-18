//! Typed Tauri command surface.
//!
//! Commands validate inputs, delegate to services, and return serializable DTOs.
//! Business logic lives in `services`; this module only shapes inputs/outputs.

use std::time::Duration;

use serde::Serialize;
use tauri::State;

use crate::{
    app::{AppState, ProbeState},
    config::{
        AppSettings, EgressConfig, EgressMode, GuardState, Paths, Theme,
        SERVICE_PROXY_PORT_DEFAULT, TASK_GUARD, TASK_KEEPER,
    },
    errors::{AppError, Result as AppResult},
    platform,
    services::{
        diagnostics::{build_health, HealthInput},
        firewall, install, networking, opencode, process,
        proxy::{self, chain},
        scheduler, tor, warp,
    },
};

type Res<T> = std::result::Result<T, String>;

fn err_string(err: AppError) -> String {
    err.user_message()
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct StatusResponse {
    pub ok: bool,
    pub message: String,
}

impl StatusResponse {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
        }
    }
    pub fn fail(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInfoDto {
    pub windows_version: String,
    pub is_win11: bool,
    pub is_win10_or_newer: bool,
    pub is_admin: bool,
    pub machine_name: String,
    pub app_version: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarpDto {
    pub installed: bool,
    pub bin: String,
    pub connected: bool,
    pub mode: String,
    pub egress_mode: String,
    pub status_text: String,
    pub adapter_v6: String,
    pub egress_v6: String,
    pub egress_prefix: String,
    pub is_cloudflare: bool,
    pub checking_egress: bool,
    pub rotating: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicesDto {
    pub keeper_running: bool,
    pub guard_running: bool,
    pub keeper_task_exists: bool,
    pub guard_task_exists: bool,
    pub keeper_task_enabled: bool,
    pub guard_task_enabled: bool,
    pub proxy_listening: bool,
    pub proxy_port: u16,
    pub firewall_installed: bool,
    pub is_admin: bool,
    pub auto_resume: bool,
    pub keeper_heartbeat: Option<f64>,
    pub guard_heartbeat: Option<f64>,
    pub autostart_registered: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaneDto {
    pub pinned_edge: String,
    pub edge_override: String,
    pub edge_pool_count: usize,
    pub strict_mode: bool,
    pub cli_path: String,
    pub desktop_path: String,
    pub path_wired: bool,
    pub desktop_service_wired: bool,
    pub shims_installed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsDto {
    pub hops: u64,
    pub blacklist_count: usize,
    pub last_hop: i64,
    pub failed_hops: u32,
    pub backoff_until: i64,
    pub free_wall_until: i64,
    pub projects: i64,
    pub sessions: i64,
    pub messages: i64,
    pub log_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogsDto {
    pub guard: Vec<String>,
    pub keeper: Vec<String>,
    pub opencode: Vec<String>,
    pub proxy: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyDto {
    pub errors_5m: usize,
    pub stream_resets_5m: usize,
    pub fallbacks_5m: usize,
    pub last_error: Option<String>,
    pub edge: Option<String>,
    pub last_bad_edge: Option<String>,
    pub fallback_age: Option<i64>,
    pub fallback_active: bool,
    pub recent: Vec<crate::config::ProxyEvent>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStateDto {
    pub platform: PlatformInfoDto,
    pub warp: WarpDto,
    pub services: ServicesDto,
    pub lane: LaneDto,
    pub stats: StatsDto,
    pub logs: LogsDto,
    pub egress: warp::EgressState,
    pub proxy: ProxyDto,
    pub auto_repair: crate::app::RepairSnapshot,
    pub health: crate::services::diagnostics::HealthSummary,
    pub probe: ProbeState,
    pub settings: AppSettings,
}

#[derive(Debug, Clone, Serialize)]
pub struct EndpointLatency {
    pub endpoint: String,
    pub latency: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogsResponse {
    pub kind: String,
    pub lines: Vec<String>,
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn task_name_for(name: &str) -> &'static str {
    if name == "keeper" {
        TASK_KEEPER
    } else {
        TASK_GUARD
    }
}

fn auto_resume_enabled(paths: &Paths) -> bool {
    std::fs::read_to_string(paths.auto_resume_flag())
        .map(|v| v.trim() != "0")
        .unwrap_or(true)
}

async fn wait_connected(bin: &std::path::Path, attempts: u32) -> bool {
    for _ in 0..attempts {
        tokio::time::sleep(Duration::from_millis(900)).await;
        if warp::warp_status(bin, true).await.connected {
            return true;
        }
    }
    false
}

fn recent_network_error(lines: Vec<String>) -> Option<String> {
    let re = regex::Regex::new(
        r"(?i)ECONNRESET|ECONNREFUSED|ECONNABORTED|ETIMEDOUT|ENETUNREACH|EHOSTUNREACH|ENOTFOUND|EAI_AGAIN|CF_HAPPY_EYEBALLS|UND_ERR_|Transport error|fetch failed|statusCode[=:\s]+503",
    )
    .ok()?;
    for line in lines.into_iter().rev() {
        if re.is_match(&line) {
            let cleaned = line.trim();
            if cleaned.is_empty() {
                continue;
            }
            return Some(cleaned.chars().take(240).collect());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// State assembly
// ---------------------------------------------------------------------------

pub async fn build_state(state: &AppState, force_egress: bool) -> AppResult<AppStateDto> {
    let paths = &state.paths;
    let cfg = EgressConfig::load(paths)?;
    let settings = AppSettings::load(paths).unwrap_or_default();

    let warp_bin = warp::find_warp_cli();
    let warp_status = match &warp_bin {
        Some(bin) => warp::warp_status(bin, false).await,
        None => warp::WarpStatus {
            connected: false,
            mode: String::new(),
            status_text: "Not Installed".into(),
        },
    };
    let local_v6 = networking::warp_local_v6().unwrap_or_default();
    let egress = crate::app::refresh_egress(state, force_egress).await;
    let (proxy_status, fallback_active) = proxy::server::proxy_status_snapshot(paths);
    let proxy_snap = proxy_status.snapshot();
    let guard_state = GuardState::load(paths).unwrap_or_default();
    let cli_path = opencode::find_opencode_cli();
    let desktop_path = opencode::find_opencode_desktop();
    let service = opencode::service_config_status(SERVICE_PROXY_PORT_DEFAULT);
    let db = opencode::db_stats();
    let keeper_hb = crate::config::heartbeat_age(&paths.keeper_heartbeat());
    let guard_hb = crate::config::heartbeat_age(&paths.guard_heartbeat());
    let firewall_installed = firewall::is_installed().await;
    let autostart =
        scheduler::task_exists(TASK_KEEPER).await || scheduler::task_exists(TASK_GUARD).await;

    let ui = state.ui.lock().await;
    let edge_pool_count = ui.edge_pool.len();
    let pinned_edge = crate::config::read_text(&paths.last_edge_file()).unwrap_or_default();
    let edge_override =
        crate::config::read_text(&paths.edge_override_file()).unwrap_or_else(|| "auto".into());
    let probe = ui.probe.clone();
    let repair = ui.repair_snapshot();
    let checking_egress = ui.egress_checking;
    drop(ui);

    let opencode_lines = crate::config::tail_lines(&Paths::opencode_log(), 160);
    let net_error_sample = recent_network_error(opencode_lines);

    let warp_dto = WarpDto {
        installed: warp_bin.is_some(),
        bin: warp_bin
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        connected: warp_status.connected,
        mode: if warp_status.mode.is_empty() {
            "unknown".into()
        } else {
            warp_status.mode.clone()
        },
        egress_mode: cfg.mode.as_str().to_string(),
        status_text: warp_status.status_text.clone(),
        adapter_v6: local_v6.clone(),
        egress_v6: egress.clone().unwrap_or_default(),
        egress_prefix: egress
            .as_deref()
            .map(networking::normalize_v6_64)
            .unwrap_or_default(),
        is_cloudflare: egress
            .as_deref()
            .map(networking::is_cloudflare_v6)
            .unwrap_or(false),
        checking_egress,
        rotating: crate::config::is_paused(paths),
    };

    let services_dto = ServicesDto {
        keeper_running: platform::mutex_held("OpencodeEgressManagerKeeper"),
        guard_running: platform::mutex_held("OpencodeEgressManagerGuard"),
        keeper_task_exists: scheduler::task_exists(TASK_KEEPER).await,
        guard_task_exists: scheduler::task_exists(TASK_GUARD).await,
        keeper_task_enabled: scheduler::task_enabled(TASK_KEEPER).await,
        guard_task_enabled: scheduler::task_enabled(TASK_GUARD).await,
        proxy_listening: networking::port_open("127.0.0.1", SERVICE_PROXY_PORT_DEFAULT, 500).await,
        proxy_port: SERVICE_PROXY_PORT_DEFAULT,
        firewall_installed,
        is_admin: platform::is_admin(),
        auto_resume: auto_resume_enabled(paths),
        keeper_heartbeat: keeper_hb,
        guard_heartbeat: guard_hb,
        autostart_registered: autostart,
    };

    let lane_dto = LaneDto {
        pinned_edge,
        edge_override,
        edge_pool_count,
        strict_mode: paths.strict_flag().is_file(),
        cli_path: cli_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        desktop_path: desktop_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        path_wired: install::user_path_has_bin(),
        desktop_service_wired: service.wired,
        shims_installed: paths.bin_dir.join("opencode.cmd").is_file(),
    };

    let stats_dto = StatsDto {
        hops: guard_state.hops,
        blacklist_count: guard_state.blacklist.len(),
        last_hop: guard_state.last_hop,
        failed_hops: guard_state.failed_hops,
        backoff_until: guard_state.backoff_until,
        free_wall_until: guard_state.free_wall_until,
        projects: db.projects,
        sessions: db.sessions,
        messages: db.messages,
        log_size_bytes: std::fs::metadata(Paths::opencode_log())
            .map(|m| m.len())
            .unwrap_or(0),
    };

    let logs_dto = LogsDto {
        guard: crate::config::tail_lines(&paths.guard_log_file(), 75),
        keeper: crate::config::tail_lines(&paths.keeper_log_file(), 75),
        opencode: crate::config::tail_lines(&Paths::opencode_log(), 55),
        proxy: crate::config::tail_lines(&paths.proxy_log_file(), 75),
    };

    let proxy_dto = ProxyDto {
        errors_5m: proxy_snap.errors_5m,
        stream_resets_5m: proxy_snap.stream_resets_5m,
        fallbacks_5m: proxy_snap.fallbacks_5m,
        last_error: proxy_snap.last_error.clone(),
        edge: proxy_snap.edge.clone(),
        last_bad_edge: proxy_snap.last_bad_edge.clone(),
        fallback_age: proxy_snap.fallback_age,
        fallback_active,
        recent: proxy_snap.recent,
    };

    let egress_dto = warp::egress_state(paths).await;

    let health = build_health(&HealthInput {
        warp_installed: warp_dto.installed,
        warp_connected: warp_dto.connected,
        rotating: warp_dto.rotating,
        mode: cfg.mode.as_str().to_string(),
        egress_tor_active: egress_dto.tor_active,
        keeper_running: services_dto.keeper_running,
        guard_running: services_dto.guard_running,
        proxy_listening: services_dto.proxy_listening,
        svc_wired: service.wired,
        path_wired: lane_dto.path_wired,
        installed: lane_dto.shims_installed,
        probe_ok: probe.ok,
        probe_code: probe.code.clone(),
        guard_backoff: guard_state.backoff_until,
        guard_free_wall: guard_state.free_wall_until,
        keeper_heartbeat: keeper_hb,
        guard_heartbeat: guard_hb,
        desktop_installed: desktop_path.is_some(),
        cli_installed: cli_path.is_some(),
        stream_resets_5m: proxy_snap.stream_resets_5m,
        fallback_active,
        fallback_age: proxy_snap.fallback_age,
        net_error_sample,
    });

    Ok(AppStateDto {
        platform: platform_info().await,
        warp: warp_dto,
        services: services_dto,
        lane: lane_dto,
        stats: stats_dto,
        logs: logs_dto,
        egress: egress_dto,
        proxy: proxy_dto,
        auto_repair: repair,
        health,
        probe,
        settings,
    })
}

async fn platform_info() -> PlatformInfoDto {
    let version = platform::version();
    PlatformInfoDto {
        windows_version: version.to_string(),
        is_win11: version.is_win11(),
        is_win10_or_newer: version.is_win10_or_newer(),
        is_admin: platform::is_admin(),
        machine_name: std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into()),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn ping() -> &'static str {
    "pong"
}

#[tauri::command]
pub async fn get_platform_info() -> PlatformInfoDto {
    platform_info().await
}

#[tauri::command]
pub async fn get_interfaces() -> Vec<networking::NetworkInterface> {
    tokio::task::spawn_blocking(networking::list_interfaces)
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub async fn get_state(state: State<'_, AppState>) -> Res<AppStateDto> {
    build_state(&state, false).await.map_err(err_string)
}

#[tauri::command]
pub async fn refresh_probe(state: State<'_, AppState>) -> Res<ProbeState> {
    let ui = state.ui.lock().await;
    Ok(ui.probe.clone())
}

#[tauri::command]
pub async fn set_egress_mode(
    state: State<'_, AppState>,
    mode: String,
    custom_proxy: Option<String>,
    custom_type: Option<String>,
) -> Res<StatusResponse> {
    let paths = &state.paths;
    let chosen = EgressMode::parse(&mode).map_err(err_string)?;
    let current = EgressConfig::load(paths).map_err(err_string)?;
    let raw = custom_proxy.unwrap_or_else(|| current.custom_proxy.clone());
    let ptype = match custom_type {
        Some(value) => chain::parse_proxy_type(&value).map_err(err_string)?,
        None => current.custom_type,
    };
    if chosen == EgressMode::Custom {
        if chain::parse_proxy_specs(&raw, ptype).is_empty() {
            return Err("no valid proxy entries found".into());
        }
        crate::services::secrets::store_proxy_credentials(paths, &raw).map_err(err_string)?;
    }
    let cfg = EgressConfig {
        mode: chosen,
        warp_scope: current.warp_scope,
        custom_proxy: chain::sanitize_proxy_string(&raw),
        custom_type: ptype,
        credentials_key: if chosen == EgressMode::Custom {
            Some("proxy-pool".into())
        } else {
            None
        },
    };
    cfg.save(paths).map_err(err_string)?;
    let _ = crate::config::write_text(&paths.lane_dir.join("custom_proxy_index.txt"), "0");
    if chosen == EgressMode::Tor {
        let _ = tor::start_portable_tor(paths).await;
    } else if chosen.is_warp_family() {
        if let Some(bin) = warp::find_warp_cli() {
            let _ = warp::connect(&bin, cfg.warp_scope).await;
        }
    }
    let _ = crate::app::refresh_egress(&state, true).await;
    Ok(StatusResponse::ok(format!(
        "Active egress switched to: {}",
        chosen.as_str().to_uppercase()
    )))
}

#[tauri::command]
pub async fn set_warp_scope(state: State<'_, AppState>, scope: String) -> Res<StatusResponse> {
    let clean = crate::config::WarpScope::parse(&scope).map_err(err_string)?;
    impl_set_scope(&state, clean).await
}

async fn impl_set_scope(state: &AppState, clean: crate::config::WarpScope) -> Res<StatusResponse> {
    let paths = &state.paths;
    let current = EgressConfig::load(paths).map_err(err_string)?;
    let changed = current.warp_scope != clean;
    let cfg = EgressConfig {
        mode: current.mode,
        warp_scope: clean,
        custom_proxy: current.custom_proxy,
        custom_type: current.custom_type,
        credentials_key: current.credentials_key,
    };
    cfg.save(paths).map_err(err_string)?;
    if let Some(bin) = warp::find_warp_cli() {
        if changed {
            let _ = warp::ensure_warp_protocol(&bin, "wireguard").await;
            let _ = warp::ensure_warp_mode(&bin, clean).await;
            let _ = warp::connect(&bin, clean).await;
        }
    }
    let _ = crate::app::refresh_egress(state, true).await;
    Ok(StatusResponse::ok(format!(
        "WARP scope set to: {}",
        clean.label()
    )))
}

#[tauri::command]
pub async fn warp_connect(state: State<'_, AppState>) -> Res<StatusResponse> {
    impl_warp_connect(&state).await
}

async fn impl_warp_connect(state: &AppState) -> Res<StatusResponse> {
    let paths = &state.paths;
    let bin = warp::find_warp_cli().ok_or_else(|| "warp-cli.exe not found".to_string())?;
    let cfg = EgressConfig::load(paths).map_err(err_string)?;
    let _ = warp::repair_registration(&bin, 3).await;
    let _ = warp::ensure_warp_mode(&bin, cfg.warp_scope).await;
    let _ = warp::run(&bin, &["connect"], 15).await;
    if !wait_connected(&bin, 10).await {
        let _ = warp::run(&bin, &["tunnel", "rotate-keys"], 15).await;
        let _ = warp::run(&bin, &["connect"], 15).await;
        if !wait_connected(&bin, 8).await {
            return Ok(StatusResponse::fail(
                "WARP could not connect. Restart the Cloudflare WARP client and retry.",
            ));
        }
    }
    let _ = crate::app::refresh_egress(state, true).await;
    Ok(StatusResponse::ok("WARP connected."))
}

#[tauri::command]
pub async fn warp_disconnect(state: State<'_, AppState>) -> Res<StatusResponse> {
    let bin = warp::find_warp_cli().ok_or_else(|| "warp-cli.exe not found".to_string())?;
    warp::disconnect(&bin).await.map_err(err_string)?;
    let mut ui = state.ui.lock().await;
    ui.egress = None;
    ui.egress_ts = chrono::Utc::now().timestamp();
    Ok(StatusResponse::ok("WARP disconnected."))
}

#[tauri::command]
pub async fn hop_now(state: State<'_, AppState>) -> Res<StatusResponse> {
    impl_hop_now(&state, true).await
}

async fn impl_hop_now(state: &AppState, toast: bool) -> Res<StatusResponse> {
    let _guard = state
        .ui
        .lock()
        .await
        .action_lock
        .clone()
        .try_lock_owned()
        .map_err(|_| "Another rotation or setup action is already running.".to_string())?;
    let paths = &state.paths;
    let cfg = EgressConfig::load(paths).map_err(err_string)?;
    let old_v6 = state.ui.lock().await.egress.clone().unwrap_or_default();
    match cfg.mode {
        EgressMode::Tor => Ok(StatusResponse::ok(
            tor::rotate_circuit(paths).await.map_err(err_string)?,
        )),
        EgressMode::Custom => {
            let (ok, msg) = chain::rotate_custom_proxy(paths)
                .await
                .map_err(err_string)?;
            let _ = crate::app::refresh_egress(state, true).await;
            Ok(if ok {
                StatusResponse::ok(msg)
            } else {
                StatusResponse::fail(msg)
            })
        }
        EgressMode::Warp | EgressMode::Clean => {
            let bin = warp::find_warp_cli().ok_or_else(|| "warp-cli.exe not found".to_string())?;
            let guard = GuardState::load(paths).map_err(err_string)?;
            let avoid = guard.blacklist.clone();
            let pause = crate::config::PauseGuard::acquire(paths, "ui-hop", 300);
            let new_v6 = warp::hop_once(&bin, paths, 5, &avoid, &old_v6).await;
            drop(pause);
            let Some(new_v6) = new_v6 else {
                return Ok(StatusResponse::fail(
                    "Rotation failed - the keeper will retry automatically.",
                ));
            };
            {
                let mut ui = state.ui.lock().await;
                ui.egress = Some(new_v6.clone());
                ui.egress_ts = chrono::Utc::now().timestamp();
            }
            let mut updated = guard;
            updated.hops += 1;
            updated.last_hop = chrono::Utc::now().timestamp();
            updated.last_ok_hop = updated.last_hop;
            updated.current = new_v6.clone();
            updated.failed_hops = 0;
            updated.backoff_until = 0;
            let _ = updated.save(paths);
            let _ = crate::config::write_text(
                &paths.edge_reroll_file(),
                &chrono::Utc::now().timestamp().to_string(),
            );
            let _ = toast;
            Ok(StatusResponse::ok(format!(
                "Rotated /64: {} -> {}",
                networking::normalize_v6_64(&old_v6),
                networking::normalize_v6_64(&new_v6)
            )))
        }
    }
}

#[tauri::command]
pub async fn repair_warp_connection(state: State<'_, AppState>) -> Res<StatusResponse> {
    impl_repair_warp(&state).await
}

async fn impl_repair_warp(state: &AppState) -> Res<StatusResponse> {
    let paths = &state.paths;
    let bin = warp::find_warp_cli().ok_or_else(|| "warp-cli.exe not found".to_string())?;
    let cfg = EgressConfig::load(paths).map_err(err_string)?;
    let _ = warp::ensure_warp_protocol(&bin, "wireguard").await;
    if !warp::registration_healthy(&bin).await && !warp::repair_registration(&bin, 3).await {
        return Ok(StatusResponse::fail(
            "WARP registration could not be created. Check the WARP client and retry.",
        ));
    }
    let _ = warp::run(&bin, &["tunnel", "rotate-keys"], 15).await;
    let _ = warp::ensure_warp_mode(&bin, cfg.warp_scope).await;
    let _ = warp::run(&bin, &["connect"], 15).await;
    if wait_connected(&bin, 8).await {
        let _ = crate::app::refresh_egress(state, true).await;
        Ok(StatusResponse::ok("WARP session renewed and connected."))
    } else {
        Ok(StatusResponse::fail(
            "WARP still cannot connect. Restart the Cloudflare WARP client and retry.",
        ))
    }
}

#[tauri::command]
pub async fn auto_heal_endpoint(state: State<'_, AppState>) -> Res<StatusResponse> {
    let bin = warp::find_warp_cli().ok_or_else(|| "warp-cli.exe not found".to_string())?;
    let msg = warp::auto_heal_endpoint(&bin, 900)
        .await
        .map_err(err_string)?;
    let _ = crate::app::refresh_egress(&state, true).await;
    Ok(StatusResponse::ok(msg))
}

#[tauri::command]
pub async fn set_clean_endpoint(
    state: State<'_, AppState>,
    endpoint: String,
) -> Res<StatusResponse> {
    let bin = warp::find_warp_cli().ok_or_else(|| "warp-cli.exe not found".to_string())?;
    warp::set_custom_endpoint(&bin, &endpoint)
        .await
        .map_err(err_string)?;
    let _ = crate::app::refresh_egress(&state, true).await;
    Ok(StatusResponse::ok(format!(
        "Cloudflare WARP endpoint set to {endpoint}."
    )))
}

#[tauri::command]
pub async fn reset_clean_endpoint(state: State<'_, AppState>) -> Res<StatusResponse> {
    let bin = warp::find_warp_cli().ok_or_else(|| "warp-cli.exe not found".to_string())?;
    warp::reset_custom_endpoint(&bin)
        .await
        .map_err(err_string)?;
    let _ = crate::app::refresh_egress(&state, true).await;
    Ok(StatusResponse::ok(
        "Cloudflare WARP endpoint reset to the default (WireGuard).",
    ))
}

#[tauri::command]
pub async fn test_clean_endpoints(timeout_ms: Option<u64>) -> Res<Vec<EndpointLatency>> {
    let timeout = timeout_ms.unwrap_or(900).clamp(200, 5000);
    let mut out = Vec::new();
    for endpoint in warp::KNOWN_CLEAN_ENDPOINTS {
        out.push(EndpointLatency {
            endpoint: (*endpoint).to_string(),
            latency: warp::test_endpoint_latency(endpoint, timeout).await,
        });
    }
    out.sort_by(|a, b| {
        b.latency
            .partial_cmp(&a.latency)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

#[tauri::command]
pub async fn toggle_tor_service(enable: bool) -> Res<StatusResponse> {
    let paths = Paths::discover();
    let msg = if enable {
        tor::start_portable_tor(&paths).await.map_err(err_string)?
    } else {
        tor::stop_portable_tor(&paths).await.map_err(err_string)?
    };
    Ok(StatusResponse::ok(msg))
}

#[tauri::command]
pub async fn rotate_tor_circuit() -> Res<StatusResponse> {
    let paths = Paths::discover();
    let msg = tor::rotate_circuit(&paths).await.map_err(err_string)?;
    Ok(StatusResponse::ok(msg))
}

#[tauri::command]
pub async fn rotate_custom_proxy(state: State<'_, AppState>) -> Res<StatusResponse> {
    let (ok, msg) = chain::rotate_custom_proxy(&state.paths)
        .await
        .map_err(err_string)?;
    let _ = crate::app::refresh_egress(&state, true).await;
    Ok(if ok {
        StatusResponse::ok(msg)
    } else {
        StatusResponse::fail(msg)
    })
}

#[tauri::command]
pub async fn toggle_worker(
    state: State<'_, AppState>,
    name: String,
    enable: bool,
) -> Res<StatusResponse> {
    if name != "keeper" && name != "guard" {
        return Ok(StatusResponse::fail("Unknown worker."));
    }
    let _paths = &state.paths;
    if !enable {
        let task = task_name_for(&name);
        if scheduler::task_exists(task).await {
            let _ = scheduler::disable_task(task).await;
        }
        let (killed, error) = process::stop_worker_processes(&name).await;
        if !error.is_empty() {
            return Ok(StatusResponse::fail(format!(
                "Stopped {killed} {name} process(es), but {error}."
            )));
        }
        return Ok(StatusResponse::ok(format!(
            "Stopped {name} ({killed} process(es))."
        )));
    }
    let app_exe = platform::current_exe();
    let task = task_name_for(&name);
    if scheduler::task_exists(task).await {
        let _ = scheduler::enable_task(task).await;
        scheduler::run_task(task)
            .await
            .map_err(|e| e.user_message())?;
        return Ok(StatusResponse::ok(format!(
            "Started {name} via Scheduled Task."
        )));
    }
    process::spawn_worker(&app_exe, &name).map_err(err_string)?;
    tokio::time::sleep(Duration::from_millis(800)).await;
    Ok(StatusResponse::ok(format!(
        "Started {name} background daemon."
    )))
}

#[tauri::command]
pub async fn restart_workers(state: State<'_, AppState>) -> Res<StatusResponse> {
    impl_restart_workers(&state).await
}

async fn impl_restart_workers(_state: &AppState) -> Res<StatusResponse> {
    let app_exe = platform::current_exe();
    for role in ["guard", "keeper"] {
        process::stop_worker_processes(role).await;
    }
    tokio::time::sleep(Duration::from_millis(700)).await;
    process::spawn_worker(&app_exe, "guard").map_err(err_string)?;
    process::spawn_worker(&app_exe, "keeper").map_err(err_string)?;
    if process::wait_for_port(SERVICE_PROXY_PORT_DEFAULT, 6).await {
        Ok(StatusResponse::ok(
            "Background service restarted with the current code.",
        ))
    } else {
        Ok(StatusResponse::fail(
            "Workers restarted, but the local proxy is not listening yet.",
        ))
    }
}

#[tauri::command]
pub async fn toggle_autostart(enable: bool) -> Res<StatusResponse> {
    let app_exe = platform::current_exe();
    if enable {
        scheduler::register_autostart(&app_exe)
            .await
            .map_err(err_string)?;
        Ok(StatusResponse::ok(
            "Registered logon auto-start tasks for Keeper & Guard.",
        ))
    } else {
        scheduler::clear_autostart().await.map_err(err_string)?;
        Ok(StatusResponse::ok("Removed logon Scheduled Tasks."))
    }
}

#[tauri::command]
pub async fn toggle_desktop_service_proxy(enable: bool) -> Res<StatusResponse> {
    impl_wire_service(enable).await
}

async fn impl_wire_service(enable: bool) -> Res<StatusResponse> {
    let paths = Paths::discover();
    if enable && !networking::port_open("127.0.0.1", SERVICE_PROXY_PORT_DEFAULT, 400).await {
        return Ok(StatusResponse::fail(format!(
            "Proxy port {SERVICE_PROXY_PORT_DEFAULT} is not listening. Start WARP Keeper first."
        )));
    }
    let msg = opencode::wire_service_proxy(&paths, enable)
        .await
        .map_err(err_string)?;
    Ok(StatusResponse::ok(msg))
}

#[tauri::command]
pub async fn toggle_firewall(enable: bool) -> Res<StatusResponse> {
    let helper = platform::find_sidecar("egress-manager-admin.exe")
        .ok_or_else(|| "privileged helper binary not found".to_string())?;
    let args = if enable {
        let cli = opencode::find_opencode_cli()
            .ok_or_else(|| "opencode CLI not found; cannot target a firewall rule".to_string())?;
        format!("firewall-install \"{}\"", cli.to_string_lossy())
    } else {
        "firewall-remove".to_string()
    };
    platform::run_elevated(&helper.to_string_lossy(), &args).map_err(err_string)?;
    Ok(StatusResponse::ok(
        "Firewall change requested. Accept the UAC prompt to apply it.",
    ))
}

#[tauri::command]
pub async fn toggle_auto_resume(enable: bool) -> Res<StatusResponse> {
    let paths = Paths::discover();
    crate::config::write_flag(&paths.auto_resume_flag(), enable).map_err(err_string)?;
    Ok(StatusResponse::ok(if enable {
        "Auto-resume enabled. A 'continue' prompt is sent after a rotation."
    } else {
        "Auto-resume disabled."
    }))
}

#[tauri::command]
pub async fn toggle_strict_mode(enable: bool) -> Res<StatusResponse> {
    let paths = Paths::discover();
    crate::config::write_flag(&paths.strict_flag(), enable).map_err(err_string)?;
    Ok(StatusResponse::ok(if enable {
        "Strict fail-closed mode enabled: requests fail when the configured egress is unavailable."
    } else {
        "Resilient fallback enabled: requests fall back to a direct connection when the route drops."
    }))
}

#[tauri::command]
pub async fn toggle_auto_repair(state: State<'_, AppState>, enable: bool) -> Res<StatusResponse> {
    let mut ui = state.ui.lock().await;
    ui.repair_enabled = enable;
    ui.repair_status = if enable {
        "Watching for connection problems".into()
    } else {
        "Auto-repair is off".into()
    };
    Ok(StatusResponse::ok(if enable {
        "Auto-repair is on. It only reconnects the proxy and route; your settings stay as chosen."
    } else {
        "Auto-repair is off."
    }))
}

#[tauri::command]
pub async fn reroll_edge(state: State<'_, AppState>) -> Res<StatusResponse> {
    let paths = &state.paths;
    crate::config::write_text(&paths.edge_override_file(), "auto\n").map_err(err_string)?;
    crate::config::write_text(
        &paths.edge_reroll_file(),
        &chrono::Utc::now().timestamp().to_string(),
    )
    .map_err(err_string)?;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let edge = crate::config::read_text(&paths.last_edge_file()).unwrap_or_default();
    Ok(StatusResponse::ok(format!(
        "Re-rolled Cloudflare edge: {}",
        if edge.is_empty() {
            "pending next request".into()
        } else {
            edge
        }
    )))
}

#[tauri::command]
pub async fn set_edge_override(state: State<'_, AppState>, edge: String) -> Res<StatusResponse> {
    let clean = edge.trim().to_ascii_lowercase();
    if clean != "auto" {
        if !clean.contains(':') || clean.len() > 64 {
            return Ok(StatusResponse::fail(
                "Edge override must be 'auto' or an IPv6 address.",
            ));
        }
        if clean.parse::<std::net::Ipv6Addr>().is_err() {
            return Ok(StatusResponse::fail(
                "Edge override is not a valid IPv6 address.",
            ));
        }
    }
    let paths = &state.paths;
    crate::config::write_text(&paths.edge_override_file(), &(clean.clone() + "\n"))
        .map_err(err_string)?;
    tokio::time::sleep(Duration::from_millis(900)).await;
    Ok(StatusResponse::ok(format!("Edge pinning set to: {clean}")))
}

#[tauri::command]
pub async fn refresh_edge_pool(state: State<'_, AppState>) -> Res<StatusResponse> {
    let pool = networking::fetch_opencode_aaaa_pool().await;
    let mut ui = state.ui.lock().await;
    ui.edge_pool = pool.clone();
    Ok(StatusResponse::ok(format!(
        "Found {} live opencode.ai IPv6 edges.",
        pool.len()
    )))
}

#[tauri::command]
pub async fn clear_blacklist(state: State<'_, AppState>) -> Res<StatusResponse> {
    impl_clear_blacklist(&state).await
}

async fn impl_clear_blacklist(state: &AppState) -> Res<StatusResponse> {
    let paths = &state.paths;
    let mut guard = GuardState::load(paths).map_err(err_string)?;
    guard.blacklist.clear();
    guard.save(paths).map_err(err_string)?;
    Ok(StatusResponse::ok("Cleared /64 blacklist."))
}

#[tauri::command]
pub async fn deep_reset_identity(state: State<'_, AppState>) -> Res<StatusResponse> {
    impl_deep_reset(&state).await
}

async fn impl_deep_reset(state: &AppState) -> Res<StatusResponse> {
    let paths = &state.paths;
    let cleared = opencode::clear_free_limit();
    impl_clear_blacklist(state).await?;
    let _ = set_edge_override_value(paths, "auto").await;
    let cfg = EgressConfig::load(paths).map_err(err_string)?;
    let rotation = match cfg.mode {
        EgressMode::Tor => tor::rotate_circuit(paths)
            .await
            .unwrap_or_else(|e| e.user_message()),
        EgressMode::Custom => chain::rotate_custom_proxy(paths)
            .await
            .map(|(_, msg)| msg)
            .unwrap_or_default(),
        _ => {
            if let Some(bin) = warp::find_warp_cli() {
                let pause = crate::config::PauseGuard::acquire(paths, "ui-deep-reset", 300);
                let new_ip = warp::hop_once(&bin, paths, 3, &[], "").await;
                drop(pause);
                match new_ip {
                    Some(ip) => format!("Rotated WARP address: {ip}"),
                    None => "WARP reconnect failed".into(),
                }
            } else {
                "WARP is not installed".into()
            }
        }
    };
    let mut message = format!("Deep reset complete. Sessions preserved. {rotation}");
    if cleared > 0 {
        message.push_str(&format!(" Cleared {cleared} limit entries."));
    }
    Ok(StatusResponse::ok(message))
}

async fn set_edge_override_value(paths: &Paths, value: &str) -> Res<()> {
    crate::config::write_text(&paths.edge_override_file(), &(value.to_string() + "\n"))
        .map_err(err_string)
}

#[tauri::command]
pub async fn fix_issue(state: State<'_, AppState>, action: String) -> Res<StatusResponse> {
    run_fix_action(&state, &action).await
}

async fn run_fix_action(state: &AppState, action: &str) -> Res<StatusResponse> {
    match action {
        "start_keeper" => {
            let app_exe = platform::current_exe();
            process::spawn_worker(&app_exe, "keeper").map_err(err_string)?;
            if process::wait_for_port(SERVICE_PROXY_PORT_DEFAULT, 8).await {
                Ok(StatusResponse::ok(
                    "Background service is running. OpenCode can connect again.",
                ))
            } else {
                Ok(StatusResponse::fail(
                    "Keeper started, but the local proxy did not bind yet.",
                ))
            }
        }
        "start_guard" => {
            let app_exe = platform::current_exe();
            process::spawn_worker(&app_exe, "guard").map_err(err_string)?;
            Ok(StatusResponse::ok("Auto-rotation started."))
        }
        "restart_workers" => impl_restart_workers(state).await,
        "connect_warp" => impl_warp_connect(state).await,
        "repair_warp" => impl_repair_warp(state).await,
        "hop" => impl_hop_now(state, false).await,
        "start_tor" => toggle_tor_service_impl(true).await,
        "install" => impl_install(state).await,
        "connect_opencode" => impl_wire_service(true).await,
        "fix_port" => impl_fix_opencode_service().await,
        "install_warp" => {
            platform::open_external_url("https://one.one.one.one/").map_err(err_string)?;
            Ok(StatusResponse::ok(
                "Opened the Cloudflare WARP download page. Install it, then press Refresh.",
            ))
        }
        other => Ok(StatusResponse::fail(format!("Unknown fix action: {other}"))),
    }
}

async fn toggle_tor_service_impl(enable: bool) -> Res<StatusResponse> {
    let paths = Paths::discover();
    let msg = if enable {
        tor::start_portable_tor(&paths).await.map_err(err_string)?
    } else {
        tor::stop_portable_tor(&paths).await.map_err(err_string)?
    };
    Ok(StatusResponse::ok(msg))
}

#[tauri::command]
pub async fn fix_everything(state: State<'_, AppState>) -> Res<StatusResponse> {
    let order = [
        "install",
        "connect_opencode",
        "start_keeper",
        "restart_workers",
        "connect_warp",
        "start_tor",
        "start_guard",
        "fix_port",
    ];
    let mut done = Vec::new();
    let mut failed = Vec::new();
    let mut manual = Vec::new();
    let mut tried: Vec<String> = Vec::new();
    for _round in 0..2 {
        let dto = build_state(&state, false).await.map_err(err_string)?;
        let mut issues: Vec<_> = dto
            .health
            .issues
            .iter()
            .filter(|i| i.fix.is_some() && !tried.contains(&i.fix.clone().unwrap()))
            .collect();
        issues.sort_by_key(|i| {
            i.fix
                .as_deref()
                .and_then(|f| order.iter().position(|o| *o == f))
                .unwrap_or(99)
        });
        if issues.is_empty() {
            break;
        }
        for issue in issues {
            let fix = issue.fix.clone().unwrap_or_default();
            tried.push(fix.clone());
            if fix == "install_warp" {
                manual.push(
                    "Install Cloudflare WARP (free) from one.one.one.one, then press Fix everything again.".into(),
                );
                continue;
            }
            match run_fix_action(&state, &fix).await {
                Ok(res) if res.ok => done.push(issue.title.clone()),
                Ok(res) => failed.push(format!("{}: {}", issue.title, res.message)),
                Err(err) => failed.push(format!("{}: {err}", issue.title)),
            }
        }
    }
    let final_state = build_state(&state, false).await.map_err(err_string)?;
    let remaining: Vec<String> = final_state
        .health
        .issues
        .iter()
        .filter(|i| i.level == "bad")
        .map(|i| i.title.clone())
        .collect();
    let mut parts = Vec::new();
    if !done.is_empty() {
        parts.push(format!("Fixed: {}.", done.join(", ")));
    }
    if !failed.is_empty() {
        parts.push(format!("Couldn't fix: {}.", failed.join("; ")));
    }
    if !remaining.is_empty() {
        parts.push(format!("Still not working: {}.", remaining.join(", ")));
    }
    parts.extend(manual);
    if remaining.is_empty() && !done.is_empty() {
        parts.push("Everything is working.".into());
    }
    Ok(if remaining.is_empty() && failed.is_empty() {
        StatusResponse::ok(parts.join(" "))
    } else {
        StatusResponse::fail(parts.join(" "))
    })
}

#[tauri::command]
pub async fn launch_desktop() -> Res<StatusResponse> {
    let paths = Paths::discover();
    let msg = install::launch_desktop(&paths).await.map_err(err_string)?;
    Ok(StatusResponse::ok(msg))
}

#[tauri::command]
pub async fn launch_cli() -> Res<StatusResponse> {
    let paths = Paths::discover();
    let msg = install::launch_cli(&paths).await.map_err(err_string)?;
    Ok(StatusResponse::ok(msg))
}

#[tauri::command]
pub async fn fix_opencode_service() -> Res<StatusResponse> {
    impl_fix_opencode_service().await
}

async fn impl_fix_opencode_service() -> Res<StatusResponse> {
    let paths = Paths::discover();
    let cli = opencode::find_opencode_cli()
        .ok_or_else(|| "OpenCode CLI not found. Install OpenCode first.".to_string())?;
    let cleared = opencode::clear_free_limit();
    let _ = opencode::run_hidden_oc(&cli, &["service", "stop"], 10).await;
    let healthy = opencode::run_hidden_oc(&cli, &["service", "status"], 8)
        .await
        .map(|(_, text)| text.contains("127.0.0.1:"))
        .unwrap_or(false);
    if !healthy {
        let _ = opencode::run_hidden_oc(&cli, &["service", "restart"], 20).await;
        tokio::time::sleep(Duration::from_millis(700)).await;
    }
    let healthy = opencode::run_hidden_oc(&cli, &["service", "status"], 8)
        .await
        .map(|(_, text)| text.contains("127.0.0.1:"))
        .unwrap_or(false);
    let wiring = if networking::port_open("127.0.0.1", SERVICE_PROXY_PORT_DEFAULT, 500).await {
        opencode::wire_service_proxy(&paths, true).await
    } else {
        opencode::wire_service_proxy(&paths, false).await
    };
    let _ = wiring;
    let extra = if cleared > 0 {
        format!(" Cleared {cleared} limit rows.")
    } else {
        String::new()
    };
    if healthy {
        Ok(StatusResponse::ok(format!(
            "OpenCode service restarted.{extra}"
        )))
    } else {
        Ok(StatusResponse::fail(format!(
            "OpenCode service did not report a listening address. Close OpenCode, then retry.{extra}"
        )))
    }
}

#[tauri::command]
pub async fn install_or_repair(state: State<'_, AppState>) -> Res<StatusResponse> {
    impl_install(&state).await
}

async fn impl_install(state: &AppState) -> Res<StatusResponse> {
    let paths = &state.paths;
    let app_exe = platform::current_exe();
    let msg = install::install_or_repair(paths, &app_exe)
        .await
        .map_err(err_string)?;
    Ok(StatusResponse::ok(msg))
}

#[tauri::command]
pub async fn uninstall_all(purge_state: Option<bool>) -> Res<StatusResponse> {
    let paths = Paths::discover();
    let _ = crate::services::secrets::clear_credentials(&paths);
    let msg = install::uninstall_all(&paths, purge_state.unwrap_or(false))
        .await
        .map_err(err_string)?;
    Ok(StatusResponse::ok(msg))
}

#[tauri::command]
pub async fn read_logs(kind: String) -> Res<LogsResponse> {
    let paths = Paths::discover();
    let lines = match kind.as_str() {
        "guard" => crate::config::tail_lines(&paths.guard_log_file(), 200),
        "keeper" => crate::config::tail_lines(&paths.keeper_log_file(), 200),
        "opencode" => crate::config::tail_lines(&Paths::opencode_log(), 200),
        "proxy" => crate::config::tail_lines(&paths.proxy_log_file(), 200),
        "tor" => crate::config::tail_lines(&paths.logs_dir.join("tor.log"), 200),
        _ => return Err("unknown log kind".into()),
    };
    Ok(LogsResponse { kind, lines })
}

#[tauri::command]
pub async fn open_logs_folder() -> Res<StatusResponse> {
    let paths = Paths::discover();
    paths.ensure_dirs().map_err(err_string)?;
    use std::os::windows::process::CommandExt;
    std::process::Command::new("explorer.exe")
        .arg(&paths.data_root)
        .creation_flags(0x0800_0000)
        .spawn()
        .map_err(|_| "could not open the logs folder".to_string())?;
    Ok(StatusResponse::ok(format!(
        "Opened {}",
        paths.data_root.display()
    )))
}

#[tauri::command]
pub async fn open_external_url(url: String) -> Res<StatusResponse> {
    platform::open_external_url(&url).map_err(err_string)?;
    Ok(StatusResponse::ok(url))
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Res<AppSettings> {
    Ok(AppSettings::load(&state.paths).unwrap_or_default())
}

#[tauri::command]
pub async fn save_settings(
    state: State<'_, AppState>,
    auto_resume: Option<bool>,
    strict_mode: Option<bool>,
    auto_repair: Option<bool>,
    autostart: Option<bool>,
    theme: Option<String>,
) -> Res<AppSettings> {
    let mut settings = AppSettings::load(&state.paths).unwrap_or_default();
    if let Some(value) = auto_resume {
        settings.auto_resume = value;
    }
    if let Some(value) = strict_mode {
        settings.strict_mode = value;
    }
    if let Some(value) = auto_repair {
        settings.auto_repair = value;
    }
    if let Some(value) = autostart {
        settings.autostart = value;
    }
    if let Some(value) = theme {
        settings.theme = match value.to_ascii_lowercase().as_str() {
            "light" => Theme::Light,
            "dark" => Theme::Dark,
            "system" => Theme::System,
            _ => return Err("theme must be light, dark, or system".into()),
        };
    }
    settings.save(&state.paths).map_err(err_string)?;
    Ok(settings)
}

// ---------------------------------------------------------------------------
// Background repair loop support
// ---------------------------------------------------------------------------

pub struct RepairActionResult {
    pub ok: bool,
    pub message: String,
}

pub async fn collect_state_for_repair(
    state: &AppState,
    paths: &Paths,
) -> crate::app::RepairSnapshotData {
    let cfg = EgressConfig::load(paths).unwrap_or_default();
    let warp_bin = warp::find_warp_cli();
    let warp_connected = match &warp_bin {
        Some(bin) => warp::warp_status(bin, false).await.connected,
        None => false,
    };
    let proxy_listening = networking::port_open("127.0.0.1", SERVICE_PROXY_PORT_DEFAULT, 400).await;
    let keeper_hb = crate::config::heartbeat_age(&paths.keeper_heartbeat());
    let keeper_running = platform::mutex_held("OpencodeEgressManagerKeeper");
    let keeper_task_enabled = scheduler::task_enabled(TASK_KEEPER).await;
    let (_, fallback_active) = proxy::server::proxy_status_snapshot(paths);
    let probe = state.ui.lock().await.probe.clone();
    crate::app::RepairSnapshotData {
        warp_connected,
        rotating: crate::config::is_paused(paths),
        mode: cfg.mode.as_str().to_string(),
        proxy_listening,
        keeper_running,
        keeper_heartbeat: keeper_hb,
        keeper_task_enabled,
        fallback_active,
        probe_code: probe.code,
    }
}

pub async fn run_repair_action(
    _app: &tauri::AppHandle,
    paths: &Paths,
    action: &str,
) -> RepairActionResult {
    let result = match action {
        "start_keeper" => {
            let app_exe = platform::current_exe();
            match process::spawn_worker(&app_exe, "keeper") {
                Ok(()) if process::wait_for_port(SERVICE_PROXY_PORT_DEFAULT, 8).await => {
                    StatusResponse::ok("Background service restarted.")
                }
                Ok(()) => StatusResponse::fail("Keeper started, but the proxy did not bind."),
                Err(err) => StatusResponse::fail(err.user_message()),
            }
        }
        "restart_keeper" => {
            let _ = process::stop_worker_processes("keeper").await;
            tokio::time::sleep(Duration::from_millis(800)).await;
            let app_exe = platform::current_exe();
            match process::spawn_worker(&app_exe, "keeper") {
                Ok(()) => StatusResponse::ok("Keeper restarted."),
                Err(err) => StatusResponse::fail(err.user_message()),
            }
        }
        "connect_warp" => match warp::find_warp_cli() {
            Some(bin) => {
                let scope = EgressConfig::load(paths).unwrap_or_default().warp_scope;
                if warp::connect(&bin, scope).await {
                    StatusResponse::ok("WARP reconnected.")
                } else {
                    StatusResponse::fail("WARP could not reconnect.")
                }
            }
            None => StatusResponse::fail("warp-cli.exe not found."),
        },
        "hop" => match warp::find_warp_cli() {
            Some(bin) => {
                let guard = GuardState::load(paths).unwrap_or_default();
                let pause = crate::config::PauseGuard::acquire(paths, "auto-repair-hop", 300);
                let new_v6 = warp::hop_once(&bin, paths, 3, &guard.blacklist, "").await;
                drop(pause);
                match new_v6 {
                    Some(ip) => StatusResponse::ok(format!("Rotated /64: {ip}")),
                    None => StatusResponse::fail("No fresh IPv6 egress was confirmed."),
                }
            }
            None => StatusResponse::fail("warp-cli.exe not found."),
        },
        "repair_warp" => match warp::find_warp_cli() {
            Some(bin) => {
                let cfg = EgressConfig::load(paths).unwrap_or_default();
                let _ = warp::ensure_warp_protocol(&bin, "wireguard").await;
                let _ = warp::run(&bin, &["tunnel", "rotate-keys"], 15).await;
                let _ = warp::ensure_warp_mode(&bin, cfg.warp_scope).await;
                let _ = warp::run(&bin, &["connect"], 15).await;
                if wait_connected(&bin, 8).await {
                    StatusResponse::ok("WARP session renewed.")
                } else {
                    StatusResponse::fail("WARP still cannot connect.")
                }
            }
            None => StatusResponse::fail("warp-cli.exe not found."),
        },
        other => StatusResponse::fail(format!("Unknown repair action: {other}")),
    };
    RepairActionResult {
        ok: result.ok,
        message: result.message,
    }
}
