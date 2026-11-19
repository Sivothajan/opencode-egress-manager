//! Tauri application assembly and shared UI runtime state.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::{
    config::{AppSettings, Paths},
    events, platform,
};

pub const WINDOW_TITLE: &str = "Opencode Egress Manager";

#[derive(Debug, Clone, Serialize)]
pub struct ProbeState {
    pub ok: Option<bool>,
    pub code: String,
    pub ms: i64,
    pub ts: i64,
    #[serde(default)]
    pub checking: bool,
}

impl Default for ProbeState {
    fn default() -> Self {
        Self {
            ok: None,
            code: "unknown".into(),
            ms: 0,
            ts: 0,
            checking: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RepairActionEntry {
    pub ts: i64,
    pub action: String,
    pub reason: String,
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct UiState {
    pub egress: Option<String>,
    pub egress_ts: i64,
    pub egress_checking: bool,
    pub probe: ProbeState,
    pub probe_ts: i64,
    pub edge_pool: Vec<String>,
    pub repair_enabled: bool,
    pub repair_status: String,
    pub repair_actions: Vec<RepairActionEntry>,
    pub repair_last: HashMap<String, f64>,
    pub action_lock: Arc<tokio::sync::Mutex<()>>,
}

impl UiState {
    pub fn new(settings: &AppSettings) -> Self {
        Self {
            egress: None,
            egress_ts: 0,
            egress_checking: false,
            probe: ProbeState::default(),
            probe_ts: 0,
            edge_pool: Vec::new(),
            repair_enabled: settings.auto_repair,
            repair_status: "Watching for connection problems".into(),
            repair_actions: Vec::new(),
            repair_last: HashMap::new(),
            action_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub fn repair_snapshot(&self) -> RepairSnapshot {
        RepairSnapshot {
            enabled: self.repair_enabled,
            status: self.repair_status.clone(),
            actions: self.repair_actions.iter().rev().take(6).cloned().collect(),
            max_per_hour: 6,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RepairSnapshot {
    pub enabled: bool,
    pub status: String,
    pub actions: Vec<RepairActionEntry>,
    pub max_per_hour: u32,
}

pub struct AppState {
    pub paths: Paths,
    pub ui: tokio::sync::Mutex<UiState>,
}

impl AppState {
    pub fn new(paths: Paths, settings: &AppSettings) -> Self {
        Self {
            paths,
            ui: tokio::sync::Mutex::new(UiState::new(settings)),
        }
    }
}

fn init_tracing(_paths: &Paths) {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .try_init();
}

pub fn run() {
    let paths = Paths::discover();
    if let Err(err) = paths.ensure_dirs() {
        eprintln!("failed to create application directories: {err}");
    }
    let settings = AppSettings::load(&paths).unwrap_or_default();
    init_tracing(&paths);

    // Single-instance guard for the UI window.
    if !platform::acquire_mutex("OpencodeEgressManagerUI") {
        if let Some(hwnd) = platform::find_window_by_title(WINDOW_TITLE) {
            platform::restore_and_focus(hwnd);
        }
        std::process::exit(0);
    }

    let context = tauri::generate_context!();
    let builder = tauri::Builder::default()
        .manage(AppState::new(paths.clone(), &settings))
        .invoke_handler(tauri::generate_handler![
            crate::commands::ping,
            crate::commands::get_platform_info,
            crate::commands::get_system_accent_color,
            crate::commands::get_interfaces,
            crate::commands::get_state,
            crate::commands::refresh_probe,
            crate::commands::set_egress_mode,
            crate::commands::set_warp_scope,
            crate::commands::warp_connect,
            crate::commands::warp_disconnect,
            crate::commands::hop_now,
            crate::commands::repair_warp_connection,
            crate::commands::auto_heal_endpoint,
            crate::commands::set_clean_endpoint,
            crate::commands::reset_clean_endpoint,
            crate::commands::test_clean_endpoints,
            crate::commands::toggle_tor_service,
            crate::commands::rotate_tor_circuit,
            crate::commands::rotate_custom_proxy,
            crate::commands::toggle_worker,
            crate::commands::restart_workers,
            crate::commands::toggle_autostart,
            crate::commands::toggle_desktop_service_proxy,
            crate::commands::toggle_firewall,
            crate::commands::toggle_auto_resume,
            crate::commands::toggle_strict_mode,
            crate::commands::toggle_auto_repair,
            crate::commands::reroll_edge,
            crate::commands::set_edge_override,
            crate::commands::refresh_edge_pool,
            crate::commands::clear_blacklist,
            crate::commands::deep_reset_identity,
            crate::commands::fix_issue,
            crate::commands::fix_everything,
            crate::commands::launch_desktop,
            crate::commands::launch_cli,
            crate::commands::fix_opencode_service,
            crate::commands::install_or_repair,
            crate::commands::uninstall_all,
            crate::commands::read_logs,
            crate::commands::open_logs_folder,
            crate::commands::open_external_url,
            crate::commands::get_settings,
            crate::commands::save_settings,
        ])
        .setup({
            let bg_paths = paths.clone();
            move |app| {
                let handle = app.handle().clone();
                let paths_a = bg_paths.clone();
                tauri::async_runtime::spawn(async move {
                    background_probe_loop(handle, paths_a).await;
                });
                let handle2 = app.handle().clone();
                let paths_b = bg_paths.clone();
                tauri::async_runtime::spawn(async move {
                    background_repair_loop(handle2, paths_b).await;
                });
                Ok(())
            }
        });

    let app = builder
        .build(context)
        .expect("failed to build the Tauri application");

    app.run(move |app_handle, event| {
        if let tauri::RunEvent::WindowEvent { label, event, .. } = &event {
            if label == "main" {
                if matches!(event, tauri::WindowEvent::ThemeChanged(_)) {
                    apply_theme(app_handle, false);
                }
                if matches!(
                    event,
                    tauri::WindowEvent::Focused(true) | tauri::WindowEvent::Resized(_)
                ) {
                    apply_theme(app_handle, true);
                }
            }
        }
    });
}

fn apply_theme(app: &AppHandle, _force: bool) {
    let paths = Paths::discover();
    let settings = AppSettings::load(&paths).unwrap_or_default();
    if let Some(window) = app.get_webview_window("main") {
        let system_dark = window
            .theme()
            .map(|theme| theme == tauri::Theme::Dark)
            .unwrap_or(true);
        let dark = match settings.theme {
            crate::config::Theme::Dark => true,
            crate::config::Theme::Light => false,
            crate::config::Theme::System => system_dark,
        };
        if let Some(hwnd) = platform::native_hwnd(&window) {
            let _ = platform::set_dark_titlebar(hwnd, dark);
        }
    }
}

async fn background_probe_loop(app: AppHandle, _paths: Paths) {
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let state = app.state::<AppState>();
        let proxy_up = crate::services::networking::port_open(
            "127.0.0.1",
            crate::config::SERVICE_PROXY_PORT_DEFAULT,
            500,
        )
        .await;
        let started = Instant::now();
        let (ok, code) = if !proxy_up {
            (false, "proxy_down")
        } else {
            let proxy = format!(
                "http://127.0.0.1:{}",
                crate::config::SERVICE_PROXY_PORT_DEFAULT
            );
            match probe_once(&proxy).await {
                Some(true) => (true, "ok"),
                Some(false) => {
                    let direct = probe_once("").await.unwrap_or(false);
                    (false, if direct { "upstream" } else { "offline" })
                }
                None => (false, "timeout"),
            }
        };
        let mut ui = state.ui.lock().await;
        ui.probe = ProbeState {
            ok: Some(ok),
            code: code.into(),
            ms: started.elapsed().as_millis() as i64,
            ts: chrono::Utc::now().timestamp(),
            checking: false,
        };
        ui.probe_ts = chrono::Utc::now().timestamp();
        drop(ui);
        events::emit(&app, events::PROBE_CHANGED, ());
        tokio::time::sleep(Duration::from_secs(if ok { 120 } else { 30 })).await;
    }
}

async fn probe_once(proxy: &str) -> Option<bool> {
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(8))
        .user_agent("opencode-egress-manager-probe/3.0");
    if !proxy.is_empty() {
        if let Ok(p) = reqwest::Proxy::all(proxy) {
            builder = builder.proxy(p);
        }
    }
    let client = builder.build().ok()?;
    match client.get("https://opencode.ai/").send().await {
        Ok(resp) => {
            let code = resp.status();
            // Any HTTP answer proves the route works.
            Some(code.is_success() || code.as_u16() < 500)
        }
        Err(_) => None,
    }
}

const AUTO_REPAIR_TICK: Duration = Duration::from_secs(25);

async fn background_repair_loop(app: AppHandle, paths: Paths) {
    tokio::time::sleep(Duration::from_secs(20)).await;
    loop {
        let state = app.state::<AppState>().clone();
        let enabled = {
            let ui = state.ui.lock().await;
            ui.repair_enabled && ui.action_lock.try_lock().is_ok()
        };
        if enabled {
            let snapshot = crate::commands::collect_state_for_repair(&state, &paths).await;
            run_repair_tick(&app, &paths, &state, snapshot).await;
        }
        tokio::time::sleep(AUTO_REPAIR_TICK).await;
    }
}

pub struct RepairSnapshotData {
    pub warp_connected: bool,
    pub rotating: bool,
    pub mode: String,
    pub proxy_listening: bool,
    pub keeper_running: bool,
    pub keeper_heartbeat: Option<f64>,
    pub keeper_task_enabled: bool,
    pub fallback_active: bool,
    pub probe_code: String,
}

async fn run_repair_tick(
    app: &AppHandle,
    paths: &Paths,
    state: &State<'_, AppState>,
    data: RepairSnapshotData,
) {
    let cooldowns: HashMap<&str, f64> = HashMap::from([
        ("start_keeper", 120.0),
        ("restart_keeper", 600.0),
        ("connect_warp", 180.0),
        ("hop", 300.0),
        ("repair_warp", 900.0),
    ]);

    let mut action = "";
    let mut reason = String::new();
    if !data.proxy_listening && (data.keeper_running || data.keeper_task_enabled) {
        action = "start_keeper";
        reason = "the local proxy stopped while OpenCode is wired to it".into();
    } else if data.keeper_running
        && data
            .keeper_heartbeat
            .map(|age| age > 240.0)
            .unwrap_or(false)
    {
        action = "restart_keeper";
        reason = "the background service stopped responding".into();
    } else if (data.mode == "warp" || data.mode == "clean")
        && !data.warp_connected
        && !data.rotating
    {
        action = "connect_warp";
        reason = "WARP is disconnected".into();
    } else if (data.mode == "warp" || data.mode == "clean")
        && data.proxy_listening
        && !data.rotating
        && (data.fallback_active || data.probe_code == "upstream" || data.probe_code == "timeout")
    {
        action = "hop";
        reason = "the local proxy could not reach opencode.ai".into();
    }
    if action.is_empty() {
        let mut ui = state.ui.lock().await;
        ui.repair_status = "Watching for connection problems".into();
        return;
    }
    let mut ui = state.ui.lock().await;
    let last = ui.repair_last.get(action).copied().unwrap_or(0.0);
    let elapsed = chrono::Utc::now().timestamp() as f64 - last;
    if elapsed < *cooldowns.get(action).unwrap_or(&300.0) {
        return;
    }
    let recent = ui
        .repair_actions
        .iter()
        .filter(|a| chrono::Utc::now().timestamp() - a.ts < 3600)
        .count();
    if recent >= 6 {
        ui.repair_status =
            "Several repairs did not restore the route. Check the Diagnostics tab.".into();
        return;
    }
    ui.repair_last
        .insert(action.to_string(), chrono::Utc::now().timestamp() as f64);
    drop(ui);

    let result = crate::commands::run_repair_action(app, paths, action).await;
    let ok = result.ok;
    let detail = result.message.clone();
    let mut ui = state.ui.lock().await;
    ui.repair_actions.push(RepairActionEntry {
        ts: chrono::Utc::now().timestamp(),
        action: action.to_string(),
        reason,
        ok,
        message: result.message,
    });
    let len = ui.repair_actions.len();
    if len > 20 {
        ui.repair_actions.drain(0..len - 20);
    }
    ui.repair_status = if ok {
        format!("Repaired: {}.", action)
    } else {
        format!("Could not repair automatically: {detail}")
    };
}

/// Ensure the UI's egress cache is reasonably fresh without blocking a command.
pub async fn refresh_egress(state: &AppState, force: bool) -> Option<String> {
    let now = chrono::Utc::now().timestamp();
    {
        let ui = state.ui.lock().await;
        if !force && now - ui.egress_ts < 25 && ui.egress.is_some() {
            return ui.egress.clone();
        }
        if ui.egress_checking {
            return ui.egress.clone();
        }
    }
    {
        let mut ui = state.ui.lock().await;
        ui.egress_checking = true;
    }
    let paths = state.paths.clone();
    let cfg = crate::config::EgressConfig::load(&paths).unwrap_or_default();
    let result = crate::services::networking::get_egress_ip(&cfg, 5).await;
    let mut ui = state.ui.lock().await;
    ui.egress_checking = false;
    ui.egress_ts = chrono::Utc::now().timestamp();
    if let Some(ip) = &result {
        ui.egress = Some(ip.clone());
    }
    result
}
