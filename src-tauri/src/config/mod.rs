//! Configuration, persistence paths, and guard/proxy state.
//!
//! All writes are atomic (temp file + rename) and keep a single `.bak` snapshot so
//! a failed write can be rolled back. Values are validated before they reach disk.

use std::{
    fmt, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::errors::{AppError, Result};

/// Brand name used for the data directory and log prefixes.
pub const BRAND: &str = "opencode-egress-manager";
/// Local port the Tauri service proxy listens on.
pub const SERVICE_PROXY_PORT_DEFAULT: u16 = 41988;
/// Proxy port owned by the legacy Python "Opencode Unlimited" app.
pub const PYTHON_SERVICE_PROXY_PORT: u16 = 41987;
/// SOCKS port used when WARP runs in isolated (proxy) mode.
pub const WARP_ISOLATED_SOCKS_PORT: u16 = 40000;
/// SOCKS port served by the bundled portable Tor client.
pub const TOR_SOCKS_PORT: u16 = 9050;
/// SOCKS port exposed by the Tor Browser bundle.
pub const TOR_BROWSER_SOCKS_PORT: u16 = 9150;
/// Tor control port used to request `NEWNYM` circuit rotation.
pub const TOR_CONTROL_PORT: u16 = 9051;
/// Scheduled-task name that keeps the shared tunnel alive.
pub const TASK_KEEPER: &str = "OpencodeEgressManager-Keeper";
/// Scheduled-task name that rotates egress when a quota wall is hit.
pub const TASK_GUARD: &str = "OpencodeEgressManager-Guard";
/// Windows Firewall rule name for the CLI kill-switch.
pub const FIREWALL_RULE_NAME: &str = "OUC_LEAKGUARD_CLI_EGRESS";
/// Named mutexes that elect the single live keeper/guard per implementation.
pub const MUTEX_KEEPER: &str = "OpencodeEgressManagerKeeper";
/// Mutex owned by the active Tauri guard process.
pub const MUTEX_GUARD: &str = "OpencodeEgressManagerGuard";
/// Mutex owned by the legacy Python keeper while it runs.
pub const PYTHON_MUTEX_KEEPER: &str = "OpencodeUnlimitedKeeper";
/// Mutex owned by the legacy Python guard while it runs.
pub const PYTHON_MUTEX_GUARD: &str = "OpencodeUnlimitedGuard";
/// User-Agent header sent with ordinary outbound HTTP requests.
pub const USER_AGENT: &str = "opencode-egress-manager/3.0";
/// User-Agent header identifying connectivity-probe traffic.
pub const PROBE_USER_AGENT: &str = "opencode-egress-manager-probe/3.0";

fn local_appdata() -> PathBuf {
    if let Ok(v) = std::env::var("LOCALAPPDATA") {
        if !v.trim().is_empty() {
            return PathBuf::from(v);
        }
    }
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join("AppData").join("Local")
}

/// True when the legacy Python "Opencode Unlimited" app is installed.
///
/// Its data root, proxy port, firewall rule, shims, and workers overlap this
/// app's. When it is present this app yields those shared resources so the
/// Python install keeps working.
pub fn python_app_present() -> bool {
    local_appdata().join("opencode-unlimited").is_dir()
}

/// Bin directory of the legacy Python app (its `opencode.cmd` shim).
pub fn python_bin_dir() -> PathBuf {
    local_appdata().join("opencode-unlimited").join("bin")
}

/// True while the legacy Python keeper owns the shared WARP tunnel / Tor daemon.
pub fn python_keeper_running() -> bool {
    crate::platform::mutex_held(PYTHON_MUTEX_KEEPER)
}

/// True while the legacy Python guard owns quota-wall rotation.
pub fn python_guard_running() -> bool {
    crate::platform::mutex_held(PYTHON_MUTEX_GUARD)
}

/// Per-user application directory layout.
#[derive(Debug, Clone)]
pub struct Paths {
    /// Root data directory (`%LOCALAPPDATA%\opencode-egress-manager`).
    pub data_root: PathBuf,
    /// Directory holding the CLI shims exposed on `PATH`.
    pub bin_dir: PathBuf,
    /// Runtime lane files: egress config, proxy log, edge selection.
    pub lane_dir: PathBuf,
    /// Guard state and quota-wall log.
    pub guard_dir: PathBuf,
    /// Keeper state and tunnel-repair log.
    pub keeper_dir: PathBuf,
    /// Portable Tor bundle and its PID file.
    pub tor_dir: PathBuf,
    /// General diagnostic logs.
    pub logs_dir: PathBuf,
}

impl Paths {
    /// Resolve the standard per-user layout under `%LOCALAPPDATA%`.
    pub fn discover() -> Self {
        let data_root = local_appdata().join(BRAND);
        Self {
            bin_dir: data_root.join("bin"),
            lane_dir: data_root.join("lane"),
            guard_dir: data_root.join("guard"),
            keeper_dir: data_root.join("keeper"),
            tor_dir: data_root.join("tor"),
            logs_dir: data_root.join("logs"),
            data_root,
        }
    }

    /// Create every directory in the layout, ignoring an already-present tree.
    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in [
            &self.data_root,
            &self.bin_dir,
            &self.lane_dir,
            &self.guard_dir,
            &self.keeper_dir,
            &self.tor_dir,
            &self.logs_dir,
        ] {
            fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    /// Selected egress mode/scope persisted for the keeper.
    pub fn egress_file(&self) -> PathBuf {
        self.lane_dir.join("egress.json")
    }
    /// User settings (auto-resume, theme, autostart, strict mode).
    pub fn settings_file(&self) -> PathBuf {
        self.data_root.join("settings.json")
    }
    /// Guard rotation state (blacklist, offsets, backoff timers).
    pub fn guard_state_file(&self) -> PathBuf {
        self.guard_dir.join("state.json")
    }
    /// Rolling proxy event status consumed by the dashboard.
    pub fn proxy_status_file(&self) -> PathBuf {
        self.lane_dir.join("proxy_status.json")
    }
    /// Proxy accept/error log shown in the UI.
    pub fn proxy_log_file(&self) -> PathBuf {
        self.lane_dir.join("proxy.log")
    }
    /// Keeper tunnel-repair log.
    pub fn keeper_log_file(&self) -> PathBuf {
        self.keeper_dir.join("keeper.log")
    }
    /// Guard rotation and wall-detection log.
    pub fn guard_log_file(&self) -> PathBuf {
        self.guard_dir.join("guard.log")
    }
    /// Timestamp written each keeper tick; used for stale-worker detection.
    pub fn keeper_heartbeat(&self) -> PathBuf {
        self.keeper_dir.join("heartbeat.txt")
    }
    /// Timestamp written each guard tick; used for stale-worker detection.
    pub fn guard_heartbeat(&self) -> PathBuf {
        self.guard_dir.join("heartbeat.txt")
    }
    /// Pause file that blocks rotation while a manual hop is in flight.
    pub fn pause_file(&self) -> PathBuf {
        self.data_root.join("keeper.pause")
    }
    /// Flag recording that the proxy fell back to a direct connection.
    pub fn fallback_flag(&self) -> PathBuf {
        self.lane_dir.join("fallback.flag")
    }
    /// Pinned Cloudflare edge override (`auto` or an IPv6 literal).
    pub fn edge_override_file(&self) -> PathBuf {
        self.lane_dir.join("edge_override.txt")
    }
    /// Last edge successfully used, shown in the UI.
    pub fn last_edge_file(&self) -> PathBuf {
        self.lane_dir.join("last_edge.txt")
    }
    /// Timestamp of the last manual/automatic edge re-roll request.
    pub fn edge_reroll_file(&self) -> PathBuf {
        self.lane_dir.join("edge_reroll")
    }
    /// Flag enabling strict proxy mode (no direct fallback).
    pub fn strict_flag(&self) -> PathBuf {
        self.lane_dir.join("strict_proxy.flag")
    }
    /// Flag toggling automatic "continue" after a wall is bypassed.
    pub fn auto_resume_flag(&self) -> PathBuf {
        self.lane_dir.join("auto_resume.flag")
    }

    /// Throwaway layout for unit tests; never used by production code paths.
    #[cfg(test)]
    pub fn for_test(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ouc-test-{}-{tag}", std::process::id()));
        let paths = Self {
            data_root: dir.clone(),
            bin_dir: dir.join("bin"),
            lane_dir: dir.join("lane"),
            guard_dir: dir.join("guard"),
            keeper_dir: dir.join("keeper"),
            tor_dir: dir.join("tor"),
            logs_dir: dir.join("logs"),
        };
        let _ = fs::create_dir_all(&paths.guard_dir);
        paths
    }
    /// OpenCode `service.json` used to wire the desktop/CLI proxy.
    pub fn opencode_service_config() -> PathBuf {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
        PathBuf::from(home)
            .join(".config")
            .join("opencode")
            .join("service.json")
    }
    /// OpenCode Desktop drafts database (client-side lockout rows).
    pub fn opencode_drafts_db() -> PathBuf {
        if let Ok(v) = std::env::var("APPDATA") {
            if !v.trim().is_empty() {
                return PathBuf::from(v)
                    .join("ai.opencode.desktop")
                    .join("drafts.sqlite");
            }
        }
        PathBuf::from(".")
    }
    /// OpenCode session database (`session_v2` table).
    pub fn opencode_db() -> PathBuf {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("opencode")
            .join("opencode.db")
    }
    /// OpenCode log file scanned for quota-wall messages.
    pub fn opencode_log() -> PathBuf {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("opencode")
            .join("log")
            .join("opencode.log")
    }
}

// ---------------------------------------------------------------------------
// Atomic JSON helpers
// ---------------------------------------------------------------------------

/// Public atomic-write helper for service-owned files.
pub fn write_atomic_pub(path: &Path, bytes: &[u8]) -> Result<()> {
    write_atomic(path, bytes)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("tmp")
    ));
    fs::write(&tmp, bytes)?;
    // Keep one rollback snapshot of the previous good value.
    if path.exists() {
        let bak = path.with_extension(format!(
            "{}.bak",
            path.extension().and_then(|e| e.to_str()).unwrap_or("bak")
        ));
        let _ = fs::copy(path, bak);
    }
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = fs::remove_file(&tmp);
            Err(AppError::Io(err.to_string()))
        }
    }
}

fn read_json<T: Default + for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<T>(&text)
            .map_err(|e| AppError::Validation(format!("{}: {e}", path.display()))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(err) => Err(AppError::Io(err.to_string())),
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text = serde_json::to_string_pretty(value)?;
    write_atomic(path, text.as_bytes())
}

pub(crate) fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Egress configuration
// ---------------------------------------------------------------------------

/// Egress route used for OpenCode traffic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EgressMode {
    /// Route through Cloudflare WARP.
    #[default]
    Warp,
    /// Route through a manually selected WARP endpoint.
    Clean,
    /// Route through the Tor network.
    Tor,
    /// Route through a user-supplied proxy chain.
    Custom,
}

impl EgressMode {
    /// Stable lowercase identifier persisted in `egress.json`.
    pub fn as_str(self) -> &'static str {
        match self {
            EgressMode::Warp => "warp",
            EgressMode::Clean => "clean",
            EgressMode::Tor => "tor",
            EgressMode::Custom => "custom",
        }
    }

    /// Parse an egress mode from its persisted lowercase form.
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "warp" => Ok(EgressMode::Warp),
            "clean" => Ok(EgressMode::Clean),
            "tor" => Ok(EgressMode::Tor),
            "custom" => Ok(EgressMode::Custom),
            other => Err(AppError::validation(format!(
                "unknown egress mode: {other}"
            ))),
        }
    }

    /// True for modes that depend on a Cloudflare WARP tunnel.
    pub fn is_warp_family(self) -> bool {
        matches!(self, EgressMode::Warp | EgressMode::Clean)
    }
}

impl fmt::Display for EgressMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How broadly WARP captures traffic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WarpScope {
    /// Full-system TUN mode (default).
    #[default]
    Tun,
    /// App-only isolated SOCKS proxy.
    Isolated,
}

impl WarpScope {
    /// Stable lowercase identifier persisted in `egress.json`.
    pub fn as_str(self) -> &'static str {
        match self {
            WarpScope::Tun => "tun",
            WarpScope::Isolated => "isolated",
        }
    }

    /// Parse a WARP scope from its persisted lowercase form.
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "tun" => Ok(WarpScope::Tun),
            "isolated" => Ok(WarpScope::Isolated),
            other => Err(AppError::validation(format!("unknown WARP scope: {other}"))),
        }
    }

    /// Human-readable label shown in the scope selector.
    pub fn label(self) -> &'static str {
        match self {
            WarpScope::Tun => "Full System TUN",
            WarpScope::Isolated => "App-Only (Isolated Proxy)",
        }
    }
}

/// Transport protocol for a custom proxy node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProxyType {
    /// SOCKS5 (remote DNS).
    #[default]
    Socks5,
    /// HTTP CONNECT.
    Http,
}

impl ProxyType {
    /// Stable lowercase identifier persisted in `egress.json`.
    pub fn as_str(self) -> &'static str {
        match self {
            ProxyType::Socks5 => "socks5",
            ProxyType::Http => "http",
        }
    }

    /// Parse a proxy type, accepting common aliases (`socks`, `https`).
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "socks5" | "socks5h" | "socks" => Ok(ProxyType::Socks5),
            "http" | "https" => Ok(ProxyType::Http),
            other => Err(AppError::validation(format!("unknown proxy type: {other}"))),
        }
    }
}

/// Persisted egress selection shared by the keeper and guard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EgressConfig {
    /// Active route.
    #[serde(default)]
    pub mode: EgressMode,
    /// WARP capture scope when `mode` is `warp`.
    #[serde(default)]
    pub warp_scope: WarpScope,
    /// Custom proxy address or newline-separated list.
    #[serde(default = "default_custom_proxy")]
    pub custom_proxy: String,
    /// Protocol used for custom proxy entries.
    #[serde(default)]
    pub custom_type: ProxyType,
    /// Credential Manager entry name for custom-proxy credentials.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials_key: Option<String>,
}

fn default_custom_proxy() -> String {
    "127.0.0.1:10808".into()
}

impl Default for EgressConfig {
    fn default() -> Self {
        Self {
            mode: EgressMode::Warp,
            warp_scope: WarpScope::Tun,
            custom_proxy: default_custom_proxy(),
            custom_type: ProxyType::Socks5,
            credentials_key: None,
        }
    }
}

impl EgressConfig {
    /// Load and validate `egress.json`.
    pub fn load(paths: &Paths) -> Result<Self> {
        let cfg: EgressConfig = read_json(&paths.egress_file())?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Validate and persist `egress.json`.
    pub fn save(&self, paths: &Paths) -> Result<()> {
        self.validate()?;
        write_json(&paths.egress_file(), self)
    }

    /// Reject empty or oversized custom-proxy lists.
    pub fn validate(&self) -> Result<()> {
        let proxy = self.custom_proxy.trim();
        if proxy.is_empty() {
            return Err(AppError::validation(
                "custom proxy address must not be empty",
            ));
        }
        if proxy.len() > 4096 {
            return Err(AppError::validation("custom proxy list is too long"));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

/// Window theme preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Follow the Windows accent setting.
    #[default]
    System,
    /// Light chrome.
    Light,
    /// Dark chrome.
    Dark,
}

/// User-facing settings persisted in `settings.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// Automatically send `continue` after a wall is bypassed.
    #[serde(default = "truth", alias = "auto_resume")]
    pub auto_resume: bool,
    /// Block direct fallback when the selected route fails.
    #[serde(default, alias = "strict_mode")]
    pub strict_mode: bool,
    /// Periodically repair a dropped tunnel/route in the background.
    #[serde(default = "truth", alias = "auto_repair")]
    pub auto_repair: bool,
    /// Register keeper/guard scheduled tasks at logon.
    #[serde(default, alias = "autostart")]
    pub autostart: bool,
    /// Selected window theme.
    #[serde(default, alias = "theme")]
    pub theme: Theme,
}

fn truth() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            auto_resume: true,
            strict_mode: false,
            auto_repair: true,
            autostart: false,
            theme: Theme::System,
        }
    }
}

impl AppSettings {
    /// Load `settings.json`, falling back to defaults when absent.
    pub fn load(paths: &Paths) -> Result<Self> {
        read_json(&paths.settings_file())
    }

    /// Persist `settings.json`.
    pub fn save(&self, paths: &Paths) -> Result<()> {
        write_json(&paths.settings_file(), self)
    }
}

// ---------------------------------------------------------------------------
// Guard state (rotation policy)
// ---------------------------------------------------------------------------

/// Maximum number of burnt `/64` prefixes retained by the guard.
pub const BLACKLIST_MAX: usize = 200;

/// Persisted quota-wall rotation policy and progress.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GuardState {
    /// Burnt `/64` prefixes that Cloudflare should not hand out again.
    #[serde(default)]
    pub blacklist: Vec<String>,
    /// Byte offset of the OpenCode log already inspected for walls.
    #[serde(default)]
    pub offset: u64,
    /// Epoch seconds of the last rotation attempt.
    #[serde(default)]
    pub last_hop: i64,
    /// Confirmed rotations performed since install.
    #[serde(default)]
    pub hops: u64,
    /// Current WARP egress IPv6 address.
    #[serde(default)]
    pub current: String,
    /// A wall was detected and is waiting for the cooldown to elapse.
    #[serde(default)]
    pub wall_pending: bool,
    /// Epoch seconds when the most recent wall was seen.
    #[serde(default)]
    pub wall_seen: i64,
    /// Epoch seconds of the last confirmed fresh egress address.
    #[serde(default)]
    pub last_ok_hop: i64,
    /// Retained for compatibility with state written by the legacy Python guard.
    #[allow(dead_code)]
    #[serde(default)]
    pub last_wait_log: i64,
    /// Session ID to wall-detection time, awaiting auto-resume.
    #[serde(default)]
    pub pending: std::collections::BTreeMap<String, i64>,
    /// Session ID to last auto-resume time.
    #[serde(default)]
    pub resumed: std::collections::BTreeMap<String, i64>,
    /// Consecutive new addresses that hit the wall again immediately.
    #[serde(default)]
    pub failed_hops: u32,
    /// Epoch seconds until which automatic rotation is paused.
    #[serde(default)]
    pub backoff_until: i64,
    /// Last `last_ok_hop` already counted toward `failed_hops`.
    #[serde(default)]
    pub hop_counted: i64,
    /// Epoch seconds of the last rotation-pause log line.
    #[serde(default)]
    pub backoff_logged: i64,
    /// Epoch seconds of the most recent free-usage wall.
    #[serde(default)]
    pub last_free_wall: i64,
    /// Fresh addresses that still hit the free wall.
    #[serde(default)]
    pub free_wall_hops: u32,
    /// Last `last_ok_hop` counted toward `free_wall_hops`.
    #[serde(default)]
    pub free_wall_counted: i64,
    /// Epoch seconds until which rotation stays paused after repeated free walls.
    #[serde(default)]
    pub free_wall_until: i64,
}

impl GuardState {
    /// Load `guard/state.json`, falling back to defaults.
    pub fn load(paths: &Paths) -> Result<Self> {
        read_json(&paths.guard_state_file())
    }

    /// Persist `guard/state.json`.
    pub fn save(&self, paths: &Paths) -> Result<()> {
        write_json(&paths.guard_state_file(), self)
    }

    /// Add a `/64` prefix to the burnt list, replacing any older entry for it.
    pub fn blacklist_prefix(&mut self, prefix: &str) {
        let clean = crate::services::networking::normalize_v6_64(prefix);
        if clean.is_empty() {
            return;
        }
        self.blacklist
            .retain(|x| crate::services::networking::normalize_v6_64(x) != clean);
        self.blacklist.push(clean);
        let len = self.blacklist.len();
        if len > BLACKLIST_MAX {
            self.blacklist.drain(0..(len - BLACKLIST_MAX));
        }
    }

    /// True when `addr` falls inside a blacklisted `/64`.
    pub fn is_burnt(&self, addr: &str) -> bool {
        let prefix = crate::services::networking::normalize_v6_64(addr);
        if prefix.is_empty() {
            return false;
        }
        self.blacklist
            .iter()
            .any(|x| crate::services::networking::normalize_v6_64(x) == prefix)
    }

    /// Drop pending/resumed entries older than the retention window.
    pub fn prune(&mut self) {
        let cutoff = now_epoch() - 1800;
        self.pending.retain(|_, v| *v >= cutoff);
        self.resumed.retain(|_, v| *v >= cutoff);
    }
}

// ---------------------------------------------------------------------------
// Proxy status / accounting
// ---------------------------------------------------------------------------

/// A single proxy event retained for the dashboard timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyEvent {
    /// Epoch seconds when the event occurred.
    pub ts: i64,
    /// Event kind (for example `start`, `error`, `stream_reset`, `fallback`).
    pub kind: String,
    /// Human-readable detail, sanitized and truncated.
    #[serde(default)]
    pub detail: String,
    /// Host the event relates to, if any.
    #[serde(default)]
    pub host: String,
    /// Cloudflare edge involved, if any.
    #[serde(default)]
    pub edge: String,
}

/// Rolling proxy status persisted across restarts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProxyStatus {
    /// Most recent events, newest last, capped and time-limited by `record`.
    #[serde(default)]
    pub recent: Vec<ProxyEvent>,
    /// Last recorded event, duplicated for quick display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event: Option<ProxyEvent>,
    /// Detail of the most recent error-like event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Epoch seconds of `last_error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error_ts: Option<i64>,
    /// Edge currently in use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<String>,
    /// Most recent edge that failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_bad_edge: Option<String>,
    /// Epoch seconds of the last direct-connection fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_ts: Option<i64>,
}

impl ProxyStatus {
    /// Load `proxy_status.json`, falling back to defaults.
    pub fn load(paths: &Paths) -> Self {
        read_json(&paths.proxy_status_file()).unwrap_or_default()
    }

    /// Persist `proxy_status.json`.
    pub fn save(&self, paths: &Paths) -> Result<()> {
        write_json(&paths.proxy_status_file(), self)
    }

    /// Append an event, trim history, update derived fields, persist, and log.
    pub fn record(&mut self, paths: &Paths, kind: &str, detail: &str, host: &str, edge: &str) {
        let entry = ProxyEvent {
            ts: now_epoch(),
            kind: kind.to_string(),
            detail: crate::errors::sanitize_detail(detail),
            host: host.chars().take(120).collect(),
            edge: edge.chars().take(120).collect(),
        };
        self.recent.retain(|e| entry.ts - e.ts <= 900);
        self.recent.push(entry.clone());
        let len = self.recent.len();
        if len > 30 {
            self.recent.drain(0..(len - 30));
        }
        self.last_event = Some(entry.clone());
        if matches!(
            kind,
            "error"
                | "reset"
                | "stream_reset"
                | "connect_fail"
                | "edge_fail"
                | "fallback"
                | "bind_fail"
        ) {
            self.last_error = Some(entry.detail.clone());
            self.last_error_ts = Some(entry.ts);
        }
        if !edge.is_empty() {
            self.edge = Some(edge.to_string());
        }
        if kind == "fallback" {
            self.fallback_ts = Some(entry.ts);
        }
        if kind == "edge_fail" && !edge.is_empty() {
            self.last_bad_edge = Some(edge.to_string());
        }
        let _ = self.save(paths);
        if matches!(
            kind,
            "error"
                | "reset"
                | "stream_reset"
                | "connect_fail"
                | "edge_fail"
                | "fallback"
                | "bind_fail"
                | "repair"
                | "start"
        ) {
            append_log(
                &paths.proxy_log_file(),
                &format!(
                    "{}: {}{}{}",
                    kind,
                    entry.detail,
                    if host.is_empty() {
                        String::new()
                    } else {
                        format!(" host={}", host)
                    },
                    if edge.is_empty() {
                        String::new()
                    } else {
                        format!(" edge={}", edge)
                    },
                ),
                1_048_576,
            );
        }
    }

    /// Derive five-minute counters and the recent-event view from stored status.
    pub fn snapshot(&self) -> ProxySnapshot {
        let now = now_epoch();
        let recent: Vec<&ProxyEvent> = self.recent.iter().filter(|e| now - e.ts <= 300).collect();
        let errors_5m = recent
            .iter()
            .filter(|e| {
                matches!(
                    e.kind.as_str(),
                    "error" | "reset" | "stream_reset" | "connect_fail" | "edge_fail"
                )
            })
            .count();
        let stream_resets_5m = recent.iter().filter(|e| e.kind == "stream_reset").count();
        let fallbacks_5m = recent.iter().filter(|e| e.kind == "fallback").count();
        ProxySnapshot {
            recent: recent.into_iter().cloned().collect(),
            errors_5m,
            stream_resets_5m,
            fallbacks_5m,
            last_error: self.last_error.clone(),
            edge: self.edge.clone(),
            last_bad_edge: self.last_bad_edge.clone(),
            fallback_age: self.fallback_ts.map(|ts| (now - ts).max(0)),
        }
    }
}

/// Point-in-time proxy counters and events derived from [`ProxyStatus`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxySnapshot {
    /// Events from the last five minutes.
    pub recent: Vec<ProxyEvent>,
    /// Error-like events in the last five minutes.
    pub errors_5m: usize,
    /// Response-stream resets in the last five minutes.
    pub stream_resets_5m: usize,
    /// Direct-connection fallbacks in the last five minutes.
    pub fallbacks_5m: usize,
    /// Detail of the most recent error-like event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Edge currently in use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge: Option<String>,
    /// Most recent edge that failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_bad_edge: Option<String>,
    /// Seconds since the last direct-connection fallback.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_age: Option<i64>,
}

// ---------------------------------------------------------------------------
// Small flag/log helpers shared by workers and UI
// ---------------------------------------------------------------------------

/// True when a flag file exists. Flags are write-once markers, not counters.
pub fn read_flag(path: &Path) -> bool {
    path.is_file()
}

/// Create or remove a flag file atomically.
pub fn write_flag(path: &Path, value: bool) -> Result<()> {
    if value {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        write_atomic(path, b"1\n")
    } else {
        let _ = fs::remove_file(path);
        Ok(())
    }
}

/// Stamp the keeper's or guard's heartbeat file with the current time.
pub fn heartbeat(paths: &Paths, keeper: bool) {
    let path = if keeper {
        paths.keeper_heartbeat()
    } else {
        paths.guard_heartbeat()
    };
    let _ = write_atomic(&path, now_epoch().to_string().as_bytes());
}

/// Seconds since a heartbeat file was last written, or `None` when absent.
pub fn heartbeat_age(path: &Path) -> Option<f64> {
    let meta = fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let age = SystemTime::now()
        .duration_since(modified)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    Some(age.max(0.0))
}

/// Auto-resume is on unless the lane flag explicitly says `0`.
pub fn auto_resume_enabled(paths: &Paths) -> bool {
    fs::read_to_string(paths.auto_resume_flag())
        .map(|v| v.trim() != "0")
        .unwrap_or(true)
}

/// On-disk pause record written by [`PauseGuard`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PauseFile {
    /// Unix timestamp (seconds, fractional) when the pause expires.
    pub until: f64,
    /// Short label identifying who requested the pause.
    pub reason: String,
    /// PID of the process that requested the pause.
    pub pid: u32,
}

/// True while a valid, unexpired pause file exists.
pub fn is_paused(paths: &Paths) -> bool {
    let path = paths.pause_file();
    let Ok(text) = fs::read_to_string(&path) else {
        return false;
    };
    let Ok(data) = serde_json::from_str::<PauseFile>(&text) else {
        let _ = fs::remove_file(&path);
        return false;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    if now < data.until {
        true
    } else {
        let _ = fs::remove_file(&path);
        false
    }
}

/// RAII guard that keeps a pause file alive until dropped.
pub struct PauseGuard {
    path: PathBuf,
    active: bool,
}

impl PauseGuard {
    /// Write a pause file valid for `seconds`, replacing any existing pause.
    pub fn acquire(paths: &Paths, reason: &str, seconds: i64) -> Self {
        let path = paths.pause_file();
        let until = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
            + seconds as f64;
        let payload = PauseFile {
            until,
            reason: reason.to_string(),
            pid: std::process::id(),
        };
        let _ = write_json(&path, &payload);
        Self { path, active: true }
    }
}

impl Drop for PauseGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Append a redacted, timestamped line, rotating to `.log.1` past `max_bytes`.
pub fn append_log(path: &Path, message: &str, max_bytes: u64) {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(meta) = fs::metadata(path) {
        if meta.len() > max_bytes {
            let bak = path.with_extension("log.1");
            let _ = fs::remove_file(&bak);
            let _ = fs::rename(path, bak);
        }
    }
    let stamp = chrono::Local::now().format("%m-%d %H:%M:%S");
    if let Ok(mut fh) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(fh, "{stamp} {}", crate::errors::redact(message));
    }
}

/// Return the last `count` lines of a file (newest last), tolerating absent files.
pub fn tail_lines(path: &Path, count: usize) -> Vec<String> {
    let Ok(meta) = fs::metadata(path) else {
        return Vec::new();
    };
    let size = meta.len();
    let read_bytes = size.min(131_072);
    let Ok(mut fh) = fs::File::open(path) else {
        return Vec::new();
    };
    use std::io::{Read, Seek, SeekFrom};
    let _ = fh.seek(SeekFrom::Start(size - read_bytes));
    let mut buf = Vec::new();
    if fh.read_to_end(&mut buf).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&buf);
    text.lines()
        .rev()
        .take(count)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|s| s.to_string())
        .collect()
}

/// Read a small control file, returning its trimmed contents.
pub fn read_text(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Write a small control file atomically.
pub fn write_text(path: &Path, value: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_atomic(path, value.as_bytes())
}

/// Restore the most recent `.bak` for a JSON config, if present.
pub fn restore_backup(path: &Path) -> Result<()> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("json");
    let bak = path.with_extension(format!("{ext}.bak"));
    if !bak.exists() {
        return Err(AppError::not_found(format!(
            "no backup for {}",
            path.display()
        )));
    }
    fs::copy(&bak, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_keeps_a_restorable_backup() {
        let dir = std::env::temp_dir().join(format!("ouc-cfg-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("egress.json");
        std::fs::write(&path, b"{\"mode\":\"warp\"}").unwrap();

        let cfg = EgressConfig {
            mode: EgressMode::Custom,
            ..Default::default()
        };
        write_json(&path, &cfg).unwrap();
        assert!(path.with_extension("json.bak").exists());

        restore_backup(&path).unwrap();
        let restored: EgressConfig = read_json(&path).unwrap();
        assert_eq!(restored.mode, EgressMode::Warp);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn guard_blacklist_normalizes_prefixes_and_caps_size() {
        let mut state = GuardState::default();
        state.blacklist_prefix("2606:4700:1234:5678::1");
        assert!(state.is_burnt("2606:4700:1234:5678:9abc::2"));
        assert!(!state.is_burnt("2606:4700:9999::1"));

        for index in 0..(BLACKLIST_MAX + 10) {
            state.blacklist_prefix(&format!("2606:4700:{index:04x}::1"));
        }
        assert!(state.blacklist.len() <= BLACKLIST_MAX);
    }

    #[test]
    fn pause_guard_expires_with_the_file() {
        let paths = Paths::for_test("pause");
        {
            let _pause = PauseGuard::acquire(&paths, "test", 60);
            assert!(is_paused(&paths));
        }
        // Drop removes the pause file, so rotation can proceed immediately.
        assert!(!is_paused(&paths));
        let _ = std::fs::remove_dir_all(&paths.data_root);
    }

    #[test]
    fn flags_round_trip() {
        let dir = std::env::temp_dir().join(format!("ouc-flag-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let flag = dir.join("strict.flag");
        assert!(!read_flag(&flag));
        write_flag(&flag, true).unwrap();
        assert!(read_flag(&flag));
        write_flag(&flag, false).unwrap();
        assert!(!read_flag(&flag));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_use_camel_case_and_accept_legacy_keys() {
        let settings = AppSettings {
            auto_resume: true,
            strict_mode: true,
            auto_repair: false,
            autostart: true,
            theme: Theme::Dark,
        };
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(json["autoResume"], true);
        assert_eq!(json["strictMode"], true);
        assert_eq!(json["autoRepair"], false);
        assert_eq!(json["autostart"], true);
        assert_eq!(json["theme"], "dark");

        // Existing snake_case settings.json files must keep loading.
        let legacy: AppSettings = serde_json::from_str(
            r#"{"auto_resume":true,"strict_mode":true,"auto_repair":false,"autostart":true,"theme":"light"}"#,
        )
        .unwrap();
        assert!(legacy.auto_resume);
        assert!(legacy.strict_mode);
        assert!(!legacy.auto_repair);
        assert_eq!(legacy.theme, Theme::Light);
    }
}
