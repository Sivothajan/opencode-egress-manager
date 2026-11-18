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

pub const BRAND: &str = "opencode-egress-manager";
pub const SERVICE_PROXY_PORT_DEFAULT: u16 = 41987;
pub const WARP_ISOLATED_SOCKS_PORT: u16 = 40000;
pub const TOR_SOCKS_PORT: u16 = 9050;
pub const TOR_BROWSER_SOCKS_PORT: u16 = 9150;
pub const TOR_CONTROL_PORT: u16 = 9051;
pub const TASK_KEEPER: &str = "OpencodeEgressManager-Keeper";
pub const TASK_GUARD: &str = "OpencodeEgressManager-Guard";
pub const FIREWALL_RULE_NAME: &str = "OUC_LEAKGUARD_CLI";

fn local_appdata() -> PathBuf {
    if let Ok(v) = std::env::var("LOCALAPPDATA") {
        if !v.trim().is_empty() {
            return PathBuf::from(v);
        }
    }
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join("AppData").join("Local")
}

/// Per-user application directory layout.
#[derive(Debug, Clone)]
pub struct Paths {
    pub data_root: PathBuf,
    pub bin_dir: PathBuf,
    pub lane_dir: PathBuf,
    pub guard_dir: PathBuf,
    pub keeper_dir: PathBuf,
    pub tor_dir: PathBuf,
    pub logs_dir: PathBuf,
}

impl Paths {
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

    pub fn egress_file(&self) -> PathBuf {
        self.lane_dir.join("egress.json")
    }
    pub fn settings_file(&self) -> PathBuf {
        self.data_root.join("settings.json")
    }
    pub fn guard_state_file(&self) -> PathBuf {
        self.guard_dir.join("state.json")
    }
    pub fn proxy_status_file(&self) -> PathBuf {
        self.lane_dir.join("proxy_status.json")
    }
    pub fn proxy_log_file(&self) -> PathBuf {
        self.lane_dir.join("proxy.log")
    }
    pub fn keeper_log_file(&self) -> PathBuf {
        self.keeper_dir.join("keeper.log")
    }
    pub fn guard_log_file(&self) -> PathBuf {
        self.guard_dir.join("guard.log")
    }
    pub fn keeper_heartbeat(&self) -> PathBuf {
        self.keeper_dir.join("heartbeat.txt")
    }
    pub fn guard_heartbeat(&self) -> PathBuf {
        self.guard_dir.join("heartbeat.txt")
    }
    pub fn pause_file(&self) -> PathBuf {
        self.data_root.join("keeper.pause")
    }
    pub fn fallback_flag(&self) -> PathBuf {
        self.lane_dir.join("fallback.flag")
    }
    pub fn edge_override_file(&self) -> PathBuf {
        self.lane_dir.join("edge_override.txt")
    }
    pub fn last_edge_file(&self) -> PathBuf {
        self.lane_dir.join("last_edge.txt")
    }
    pub fn edge_reroll_file(&self) -> PathBuf {
        self.lane_dir.join("edge_reroll")
    }
    pub fn strict_flag(&self) -> PathBuf {
        self.lane_dir.join("strict_proxy.flag")
    }
    pub fn auto_resume_flag(&self) -> PathBuf {
        self.lane_dir.join("auto_resume.flag")
    }
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
    pub fn opencode_db() -> PathBuf {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("opencode")
            .join("opencode.db")
    }
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

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Egress configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EgressMode {
    #[default]
    Warp,
    Clean,
    Tor,
    Custom,
}

impl EgressMode {
    pub fn as_str(self) -> &'static str {
        match self {
            EgressMode::Warp => "warp",
            EgressMode::Clean => "clean",
            EgressMode::Tor => "tor",
            EgressMode::Custom => "custom",
        }
    }

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

    pub fn is_warp_family(self) -> bool {
        matches!(self, EgressMode::Warp | EgressMode::Clean)
    }
}

impl fmt::Display for EgressMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WarpScope {
    #[default]
    Tun,
    Isolated,
}

impl WarpScope {
    pub fn as_str(self) -> &'static str {
        match self {
            WarpScope::Tun => "tun",
            WarpScope::Isolated => "isolated",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "tun" => Ok(WarpScope::Tun),
            "isolated" => Ok(WarpScope::Isolated),
            other => Err(AppError::validation(format!("unknown WARP scope: {other}"))),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WarpScope::Tun => "Full System TUN",
            WarpScope::Isolated => "App-Only (Isolated Proxy)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProxyType {
    #[default]
    Socks5,
    Http,
}

impl ProxyType {
    pub fn as_str(self) -> &'static str {
        match self {
            ProxyType::Socks5 => "socks5",
            ProxyType::Http => "http",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "socks5" | "socks5h" | "socks" => Ok(ProxyType::Socks5),
            "http" | "https" => Ok(ProxyType::Http),
            other => Err(AppError::validation(format!("unknown proxy type: {other}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EgressConfig {
    #[serde(default)]
    pub mode: EgressMode,
    #[serde(default)]
    pub warp_scope: WarpScope,
    #[serde(default = "default_custom_proxy")]
    pub custom_proxy: String,
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
    pub fn load(paths: &Paths) -> Result<Self> {
        let cfg: EgressConfig = read_json(&paths.egress_file())?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        self.validate()?;
        write_json(&paths.egress_file(), self)
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default = "truth")]
    pub auto_resume: bool,
    #[serde(default)]
    pub strict_mode: bool,
    #[serde(default = "truth")]
    pub auto_repair: bool,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
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
    pub fn load(paths: &Paths) -> Result<Self> {
        read_json(&paths.settings_file())
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        write_json(&paths.settings_file(), self)
    }
}

// ---------------------------------------------------------------------------
// Guard state (rotation policy)
// ---------------------------------------------------------------------------

pub const BLACKLIST_MAX: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GuardState {
    #[serde(default)]
    pub blacklist: Vec<String>,
    #[serde(default)]
    pub offset: u64,
    #[serde(default)]
    pub last_hop: i64,
    #[serde(default)]
    pub hops: u64,
    #[serde(default)]
    pub current: String,
    #[serde(default)]
    pub wall_pending: bool,
    #[serde(default)]
    pub wall_seen: i64,
    #[serde(default)]
    pub last_ok_hop: i64,
    /// Retained for compatibility with state written by the legacy Python guard.
    #[allow(dead_code)]
    #[serde(default)]
    pub last_wait_log: i64,
    #[serde(default)]
    pub pending: std::collections::BTreeMap<String, i64>,
    #[serde(default)]
    pub resumed: std::collections::BTreeMap<String, i64>,
    #[serde(default)]
    pub failed_hops: u32,
    #[serde(default)]
    pub backoff_until: i64,
    #[serde(default)]
    pub hop_counted: i64,
    #[serde(default)]
    pub backoff_logged: i64,
    #[serde(default)]
    pub last_free_wall: i64,
    #[serde(default)]
    pub free_wall_hops: u32,
    #[serde(default)]
    pub free_wall_counted: i64,
    #[serde(default)]
    pub free_wall_until: i64,
}

impl GuardState {
    pub fn load(paths: &Paths) -> Result<Self> {
        read_json(&paths.guard_state_file())
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        write_json(&paths.guard_state_file(), self)
    }

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

    pub fn is_burnt(&self, addr: &str) -> bool {
        let prefix = crate::services::networking::normalize_v6_64(addr);
        if prefix.is_empty() {
            return false;
        }
        self.blacklist
            .iter()
            .any(|x| crate::services::networking::normalize_v6_64(x) == prefix)
    }

    pub fn prune(&mut self) {
        let cutoff = now_epoch() - 1800;
        self.pending.retain(|_, v| *v >= cutoff);
        self.resumed.retain(|_, v| *v >= cutoff);
    }
}

// ---------------------------------------------------------------------------
// Proxy status / accounting
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyEvent {
    pub ts: i64,
    pub kind: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub edge: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProxyStatus {
    #[serde(default)]
    pub recent: Vec<ProxyEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event: Option<ProxyEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error_ts: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_bad_edge: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_ts: Option<i64>,
}

impl ProxyStatus {
    pub fn load(paths: &Paths) -> Self {
        read_json(&paths.proxy_status_file()).unwrap_or_default()
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        write_json(&paths.proxy_status_file(), self)
    }

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxySnapshot {
    pub recent: Vec<ProxyEvent>,
    pub errors_5m: usize,
    pub stream_resets_5m: usize,
    pub fallbacks_5m: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_bad_edge: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_age: Option<i64>,
}

// ---------------------------------------------------------------------------
// Small flag/log helpers shared by workers and UI
// ---------------------------------------------------------------------------

pub fn read_flag(path: &Path) -> bool {
    path.is_file()
}

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

pub fn heartbeat(paths: &Paths, keeper: bool) {
    let path = if keeper {
        paths.keeper_heartbeat()
    } else {
        paths.guard_heartbeat()
    };
    let _ = write_atomic(&path, now_epoch().to_string().as_bytes());
}

pub fn heartbeat_age(path: &Path) -> Option<f64> {
    let meta = fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let age = SystemTime::now()
        .duration_since(modified)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    Some(age.max(0.0))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PauseFile {
    pub until: f64,
    pub reason: String,
    pub pid: u32,
}

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

pub struct PauseGuard {
    path: PathBuf,
    active: bool,
}

impl PauseGuard {
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
        let dir = std::env::temp_dir().join(format!("ouc-pause-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_root: dir.clone(),
            bin_dir: dir.join("bin"),
            lane_dir: dir.join("lane"),
            guard_dir: dir.join("guard"),
            keeper_dir: dir.join("keeper"),
            tor_dir: dir.join("tor"),
            logs_dir: dir.join("logs"),
        };
        {
            let _pause = PauseGuard::acquire(&paths, "test", 60);
            assert!(is_paused(&paths));
        }
        // Drop removes the pause file, so rotation can proceed immediately.
        assert!(!is_paused(&paths));
        let _ = std::fs::remove_dir_all(&dir);
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
}
