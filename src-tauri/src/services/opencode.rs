//! OpenCode detection, configuration wiring, database maintenance, and resume.
#![allow(non_camel_case_types)]

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use serde::Serialize;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_CONTROL, VK_R, VK_RETURN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow, ShowWindow, SW_RESTORE,
};

use crate::{
    config::Paths,
    errors::{AppError, Result},
    services::networking::run_hidden,
};

const SERVICE_ENV_KEYS: &[&str] = &[
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "https_proxy",
    "http_proxy",
    "NO_PROXY",
    "no_proxy",
];
const NO_PROXY_VALUE: &str = "127.0.0.1,localhost,::1";

fn opencode_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(env_bin) = std::env::var("OUC_REAL_BIN") {
        if !env_bin.trim().is_empty() {
            out.push(PathBuf::from(env_bin));
        }
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        out.push(
            PathBuf::from(&local)
                .join("Programs")
                .join("ai.opencode.desktop")
                .join("resources")
                .join("opencode-cli.exe"),
        );
        out.push(PathBuf::from(&local).join("opencode").join("opencode.exe"));
    }
    if let Ok(home) = std::env::var("USERPROFILE") {
        out.push(
            PathBuf::from(&home)
                .join(".opencode")
                .join("bin")
                .join("opencode.exe"),
        );
        out.push(
            PathBuf::from(&home)
                .join(".bun")
                .join("bin")
                .join("opencode.exe"),
        );
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        out.push(PathBuf::from(&appdata).join("npm").join("opencode.cmd"));
    }
    out
}

fn desktop_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        out.push(
            PathBuf::from(&local)
                .join("Programs")
                .join("ai.opencode.desktop")
                .join("OpenCode.exe"),
        );
        out.push(
            PathBuf::from(&local)
                .join("Programs")
                .join("OpenCode")
                .join("OpenCode.exe"),
        );
        out.push(PathBuf::from(&local).join("OpenCode").join("OpenCode.exe"));
    }
    out.push(PathBuf::from(r"C:\Program Files\OpenCode\OpenCode.exe"));
    out.push(PathBuf::from(
        r"C:\Program Files\ai.opencode.desktop\OpenCode.exe",
    ));
    out.push(PathBuf::from(
        r"C:\Program Files (x86)\OpenCode\OpenCode.exe",
    ));
    out.push(PathBuf::from(
        r"C:\Program Files (x86)\ai.opencode.desktop\OpenCode.exe",
    ));
    out
}

/// Locate the real OpenCode CLI binary, preferring running CLI processes.
pub fn find_opencode_cli() -> Option<PathBuf> {
    for candidate in opencode_candidates() {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let mut found: Vec<PathBuf> = Vec::new();
    if let Some(system) = crate::services::process::system_all() {
        for process in system.processes().values() {
            let name = process.name().to_string_lossy().to_string();
            let exe = process.exe().map(|p| p.to_path_buf());
            let Some(exe) = exe else { continue };
            let low = exe.to_string_lossy().to_ascii_lowercase();
            if low.contains("opencode-egress-manager") {
                continue;
            }
            if name.to_ascii_lowercase().contains("opencode-cli") && exe.is_file() {
                return Some(exe);
            }
            if name.eq_ignore_ascii_case("opencode.exe")
                && !low.contains("ai.opencode.desktop")
                && exe.is_file()
            {
                found.push(exe);
            }
        }
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        let cli_dir = PathBuf::from(appdata)
            .join("ai.opencode.desktop")
            .join("cli");
        if cli_dir.is_dir() {
            let mut versions: Vec<PathBuf> = Vec::new();
            collect_files(&cli_dir, &mut versions);
            versions.retain(|p| {
                p.file_name().and_then(|n| n.to_str()) == Some("opencode-cli.exe") && p.is_file()
            });
            versions.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
            if let Some(latest) = versions.pop() {
                return Some(latest);
            }
        }
    }
    if let Some(found) = found.into_iter().next() {
        return Some(found);
    }
    for name in ["opencode.exe", "opencode"] {
        if let Ok(path) = which::which(name) {
            let low = path.to_string_lossy().to_ascii_lowercase();
            if low.contains("opencode-egress-manager") {
                continue;
            }
            if low.contains("ai.opencode.desktop") && !low.ends_with("opencode-cli.exe") {
                continue;
            }
            return Some(path);
        }
    }
    None
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Locate the OpenCode Desktop executable.
pub fn find_opencode_desktop() -> Option<PathBuf> {
    if let Some(system) = crate::services::process::system_all() {
        for process in system.processes().values() {
            let name = process.name().to_string_lossy().to_ascii_lowercase();
            if name != "opencode.exe" {
                continue;
            }
            let Some(exe) = process.exe() else { continue };
            let low = exe.to_string_lossy().to_ascii_lowercase();
            if low.contains("ai.opencode.desktop") && exe.is_file() {
                return Some(exe.to_path_buf());
            }
        }
    }
    desktop_candidates().into_iter().find(|p| p.is_file())
}

/// Run the OpenCode CLI with a hidden console (used for service maintenance).
pub async fn run_hidden_oc(cli: &Path, args: &[&str], timeout_secs: u64) -> Result<(i32, String)> {
    let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    crate::services::networking::run_hidden(&cli.to_string_lossy(), &owned, timeout_secs).await
}

// ---------------------------------------------------------------------------
// service.json wiring
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ServiceConfigStatus {
    pub wired: bool,
    pub path: PathBuf,
}

pub fn service_config_status(proxy_port: u16) -> ServiceConfigStatus {
    let path = Paths::opencode_service_config();
    let wired = read_service_config(&path)
        .and_then(|value| {
            let env = value.get("env")?.as_object()?;
            let https = env.get("HTTPS_PROXY")?.as_str()?;
            Some(https.contains(&format!("127.0.0.1:{proxy_port}")))
        })
        .unwrap_or(false);
    ServiceConfigStatus { wired, path }
}

fn read_service_config(path: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Wire or unwire OpenCode's background service proxy variables, preserving all
/// unrelated keys and keeping a rollback snapshot.
pub async fn wire_service_proxy(paths: &Paths, enable: bool) -> Result<String> {
    let path = Paths::opencode_service_config();
    let proxy_url = format!(
        "http://127.0.0.1:{}",
        crate::config::SERVICE_PROXY_PORT_DEFAULT
    );
    let mut cfg = read_service_config(&path).unwrap_or_else(|| serde_json::json!({}));
    if !cfg.is_object() {
        cfg = serde_json::json!({});
    }
    let obj = cfg.as_object_mut().unwrap();
    let env = obj
        .entry("env".to_string())
        .or_insert_with(|| serde_json::json!({}));
    if !env.is_object() {
        *env = serde_json::json!({});
    }
    let env_map = env.as_object_mut().unwrap();
    if enable {
        for key in ["HTTPS_PROXY", "HTTP_PROXY", "https_proxy", "http_proxy"] {
            env_map.insert(
                (*key).to_string(),
                serde_json::Value::String(proxy_url.clone()),
            );
        }
        for key in ["NO_PROXY", "no_proxy"] {
            env_map.insert(
                (*key).to_string(),
                serde_json::Value::String(NO_PROXY_VALUE.into()),
            );
        }
    } else {
        for key in SERVICE_ENV_KEYS {
            env_map.remove(*key);
        }
        if env_map.is_empty() {
            obj.remove("env");
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Rollback snapshot before replacing.
    if path.exists() {
        let bak = path.with_extension("json.bak");
        let _ = std::fs::copy(&path, bak);
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&cfg)?)?;
    std::fs::rename(&tmp, &path)?;

    if let Some(cli) = find_opencode_cli() {
        let args: Vec<Vec<String>> = if enable {
            let mut a = Vec::new();
            for key in ["HTTPS_PROXY", "HTTP_PROXY"] {
                a.push(vec![
                    "service".to_string(),
                    "set".into(),
                    "env".into(),
                    key.into(),
                    proxy_url.clone(),
                ]);
            }
            a.push(vec![
                "service".into(),
                "set".into(),
                "env".into(),
                "NO_PROXY".into(),
                NO_PROXY_VALUE.into(),
            ]);
            a
        } else {
            SERVICE_ENV_KEYS
                .iter()
                .map(|key| {
                    vec![
                        "service".to_string(),
                        "unset".into(),
                        "env".into(),
                        (*key).to_string(),
                    ]
                })
                .collect()
        };
        for arg in args {
            let _ = run_hidden(&cli.to_string_lossy(), &arg, 4).await;
        }
        let _ = run_hidden(
            &cli.to_string_lossy(),
            &["service".into(), "restart".into()],
            15,
        )
        .await;
    }
    let _ = paths;
    Ok(if enable {
        format!("OpenCode Desktop service wired to {proxy_url}.")
    } else {
        "OpenCode Desktop service restored to direct egress.".into()
    })
}

/// Restore service.json from its rollback snapshot, if present.
pub fn restore_service_backup() -> Result<()> {
    let path = Paths::opencode_service_config();
    let bak = path.with_extension("json.bak");
    if !bak.exists() {
        return Err(AppError::not_found("no service.json backup"));
    }
    std::fs::copy(bak, path)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Database maintenance
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct DbStats {
    pub projects: i64,
    pub sessions: i64,
    pub messages: i64,
}

/// Delete client-side rate-limit lockout rows from drafts.sqlite.
///
/// Only the `state` rows named below are removed; chat history is untouched.
pub fn clear_free_limit() -> i64 {
    let db_path = Paths::opencode_drafts_db();
    if !db_path.is_file() {
        return 0;
    }
    let conn = match rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
    ) {
        Ok(conn) => conn,
        Err(err) => {
            tracing::debug!(error = %err, "drafts.sqlite open failed");
            return 0;
        }
    };
    let _ = conn.busy_timeout(Duration::from_millis(5000));
    match conn.execute(
        "DELETE FROM state WHERE key IN ('free-limit','go-upsell','free_usage_limit','usage_limit','quota_exceeded')",
        [],
    ) {
        Ok(changed) => changed as i64,
        Err(err) => {
            tracing::debug!(error = %err, "drafts.sqlite lockout purge failed");
            0
        }
    }
}

pub fn db_stats() -> DbStats {
    let db_path = Paths::opencode_db();
    let mut out = DbStats {
        projects: 0,
        sessions: 0,
        messages: 0,
    };
    if !db_path.is_file() {
        return out;
    }
    if let Ok(conn) =
        rusqlite::Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    {
        for (key, table) in [
            ("projects", "project"),
            ("sessions", "session_v2"),
            ("messages", "session_message"),
        ] {
            if let Ok(count) = conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get::<_, i64>(0)
            }) {
                match key {
                    "projects" => out.projects = count,
                    "sessions" => out.sessions = count,
                    _ => out.messages = count,
                }
            }
        }
    }
    out
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub directory: String,
    pub title: String,
}

pub fn latest_session() -> Option<SessionInfo> {
    let db_path = Paths::opencode_db();
    if !db_path.is_file() {
        return None;
    }
    let conn =
        rusqlite::Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .ok()?;
    let row: Option<(String, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT id, directory, title FROM session_v2 WHERE time_archived IS NULL ORDER BY time_updated DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .ok();
    row.and_then(|(id, directory, title)| {
        if id.is_empty() {
            None
        } else {
            Some(SessionInfo {
                id,
                directory: directory.unwrap_or_default(),
                title: title.unwrap_or_default(),
            })
        }
    })
}

pub fn session_directory(session_id: &str) -> Option<String> {
    if session_id.len() > 128
        || !session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    let conn = rusqlite::Connection::open_with_flags(
        Paths::opencode_db(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .ok()?;
    conn.query_row(
        "SELECT directory FROM session_v2 WHERE id = ?1",
        [session_id],
        |row| row.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
}

fn opencode_command(cli: &Path, args: &[String]) -> Vec<String> {
    let text = cli.to_string_lossy().to_ascii_lowercase();
    if text.ends_with(".cmd") || text.ends_with(".bat") {
        let comspec = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into());
        let mut out = vec![comspec, "/c".to_string()];
        out.extend(args.iter().cloned());
        out
    } else {
        args.to_vec()
    }
}

fn spawn_detached(program: &str, args: &[String], cwd: Option<&Path>) -> Result<()> {
    let mut cmd = stdio_safe_command(program, args);
    cmd.creation_flags(0x0800_0000 | 0x0000_0008);
    if let Some(cwd) = cwd {
        if cwd.is_dir() {
            cmd.current_dir(cwd);
        }
    }
    cmd.spawn()
        .map_err(|e| AppError::Process(format!("failed to launch {program}: {e}")))?;
    Ok(())
}

fn stdio_safe_command(program: &str, args: &[String]) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd
}

/// Dispatch `opencode run --session <id> continue` without a console window.
pub async fn resume_sessions(session_ids: &[String]) -> Result<(u32, Vec<String>)> {
    let cli = find_opencode_cli().ok_or_else(|| AppError::not_found("OpenCode CLI not found"))?;
    let mut sent = 0;
    let mut notes = Vec::new();
    for sid in session_ids.iter().filter(|s| !s.is_empty()) {
        let directory = session_directory(sid);
        let cwd = directory.as_deref().map(Path::new).filter(|p| p.is_dir());
        let base = vec![
            "run".to_string(),
            "--session".to_string(),
            sid.clone(),
            "continue".to_string(),
        ];
        let program = cli.to_string_lossy().to_string();
        let full = opencode_command(&cli, &base);
        let argv = full_split(&full, &program);
        match spawn_detached(&argv.0, &argv.1, cwd) {
            Ok(()) => sent += 1,
            Err(err) => notes.push(format!(
                "{}: {}",
                &sid[..sid.len().min(12)],
                err.user_message()
            )),
        }
    }
    Ok((sent, notes))
}

/// Split an argument vector into (program, arguments) preserving cmd.exe wrappers.
fn full_split(full: &[String], fallback_program: &str) -> (String, Vec<String>) {
    if full.is_empty() {
        return (fallback_program.to_string(), Vec::new());
    }
    let program = full[0].clone();
    let rest = full[1..].to_vec();
    if program.eq_ignore_ascii_case("cmd.exe") || program.eq_ignore_ascii_case("cmd") {
        // cmd.exe /c <exe> <args...>
        if rest.len() >= 2 && rest[0].eq_ignore_ascii_case("/c") {
            return (rest[1].clone(), rest[2..].to_vec());
        }
    }
    (program, rest)
}

fn dispatch_cli_resume(cli: &Path, session: Option<SessionInfo>) -> Result<()> {
    let args = match &session {
        Some(info) => vec![
            "run".to_string(),
            "--session".to_string(),
            info.id.clone(),
            "continue".to_string(),
        ],
        None => vec![
            "run".to_string(),
            "--continue".to_string(),
            "continue".to_string(),
        ],
    };
    let cwd = session
        .as_ref()
        .and_then(|s| {
            if s.directory.is_empty() {
                None
            } else {
                Some(PathBuf::from(&s.directory))
            }
        })
        .filter(|p| p.is_dir());
    let program = cli.to_string_lossy().to_string();
    let full = opencode_command(cli, &args);
    let argv = full_split(&full, &program);
    spawn_detached(&argv.0, &argv.1, cwd.as_deref())
}

// ---------------------------------------------------------------------------
// Native keystroke delivery
// ---------------------------------------------------------------------------

fn send_key(virtual_key: VIRTUAL_KEY, extra_flags: KEYBD_EVENT_FLAGS) {
    unsafe {
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: virtual_key,
                    wScan: 0,
                    dwFlags: extra_flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

fn send_unicode(scan: u16, flags: KEYBD_EVENT_FLAGS) {
    unsafe {
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

fn send_text(text: &str) {
    for ch in text.chars() {
        let scan = ch as u16;
        send_unicode(scan, KEYEVENTF_UNICODE);
        send_unicode(scan, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP);
    }
}

fn send_ctrl_r() {
    send_key(VK_CONTROL, KEYBD_EVENT_FLAGS(0));
    send_key(VK_R, KEYBD_EVENT_FLAGS(0));
    send_key(VK_R, KEYEVENTF_KEYUP);
    send_key(VK_CONTROL, KEYEVENTF_KEYUP);
}

extern "system" fn resume_enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
    unsafe {
        let targets = lparam.0 as *mut Vec<HWND>;
        if targets.is_null() || !IsWindowVisible(hwnd).as_bool() {
            return true.into();
        }
        let len = GetWindowTextLengthW(hwnd);
        if len == 0 {
            return true.into();
        }
        let mut buf = vec![0u16; (len as usize) + 1];
        let read = GetWindowTextW(hwnd, &mut buf);
        if read == 0 {
            return true.into();
        }
        let title = String::from_utf16_lossy(&buf[..read as usize]).to_ascii_lowercase();
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == std::process::id() {
            return true.into();
        }
        let name = platform_process_name(pid).to_ascii_lowercase();
        let terminal = [
            "windowsterminal.exe",
            "cmd.exe",
            "powershell.exe",
            "pwsh.exe",
            "conhost.exe",
            "mintty.exe",
            "wezterm-gui.exe",
            "alacritty.exe",
        ];
        let matches = name == "opencode.exe"
            || name == "opencode-cli.exe"
            || (terminal.contains(&name.as_str()) && title.contains("opencode"));
        if matches {
            (*targets).push(hwnd);
        }
    }
    true.into()
}

fn platform_process_name(pid: u32) -> String {
    let Some(system) = crate::services::process::system_all() else {
        return String::new();
    };
    system
        .process(sysinfo::Pid::from_u32(pid))
        .map(|p| p.name().to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Focus the OpenCode window and send reload + `continue`. Returns false when no
/// suitable window can be focused safely.
pub fn send_continue_keystroke(reload_first: bool) -> bool {
    let hwnds: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(resume_enum_cb), LPARAM(&hwnds as *const _ as isize));
    }
    for hwnd in hwnds {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
            std::thread::sleep(Duration::from_millis(350));
            if GetForegroundWindow() != hwnd {
                continue;
            }
            if reload_first {
                send_ctrl_r();
                std::thread::sleep(Duration::from_millis(900));
                let _ = SetForegroundWindow(hwnd);
                std::thread::sleep(Duration::from_millis(250));
                if GetForegroundWindow() != hwnd {
                    continue;
                }
            }
            send_text("continue");
            send_key(VK_RETURN, KEYBD_EVENT_FLAGS(0));
            send_key(VK_RETURN, KEYEVENTF_KEYUP);
            return true;
        }
    }
    false
}

/// Full auto-resume flow: clear lockouts, then resume known sessions or the window.
pub async fn resume_active_session(
    paths: &Paths,
    session_ids: Option<Vec<String>>,
) -> Result<String> {
    let enabled = std::fs::read_to_string(paths.auto_resume_flag())
        .map(|v| v.trim() != "0")
        .unwrap_or(true);
    if !enabled {
        return Ok("Auto-resume is disabled.".into());
    }
    let cleared = clear_free_limit();
    let mut details: Vec<String> = Vec::new();
    if cleared > 0 {
        details.push(format!(
            "cleared {cleared} rate limit entries from database"
        ));
    }
    if let Some(ids) = session_ids.filter(|v| !v.is_empty()) {
        let (sent, notes) = resume_sessions(&ids).await?;
        details.push(format!(
            "sent 'continue' to {sent} of {} chat(s) that hit the limit",
            ids.len()
        ));
        details.extend(notes);
        if sent > 0 {
            return Ok(details.join(" | "));
        }
    }
    if send_continue_keystroke(true) {
        details.push("sent reload and 'continue' to the OpenCode window".to_string());
        return Ok(details.join(" | "));
    }
    if let Some(cli) = find_opencode_cli() {
        let session = latest_session();
        dispatch_cli_resume(&cli, session.clone())?;
        let target = session
            .map(|s| format!("session {}...", &s.id[..s.id.len().min(8)]))
            .unwrap_or_else(|| "last session".into());
        details.push(format!("dispatched CLI continue to {target}"));
        return Ok(details.join(" | "));
    }
    if cleared > 0 {
        return Ok(format!(
            "{} | but no OpenCode window or session was available",
            details.join(" | ")
        ));
    }
    Ok("No active session or window found to resume.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_session_ids() {
        // A valid-looking id may resolve to None when the DB is absent; the point
        // is that hostile input can never reach the SQL layer.
        assert!(session_directory("../../etc/passwd").is_none());
        assert!(session_directory(&"a".repeat(200)).is_none());
        assert!(session_directory("").is_none());
    }

    #[test]
    fn splits_cmd_wrappers() {
        let full = vec![
            "cmd.exe".into(),
            "/c".into(),
            "opencode.cmd".into(),
            "run".into(),
        ];
        let argv = full_split(&full, "fallback");
        assert_eq!(argv.0, "opencode.cmd");
        assert_eq!(argv.1, vec!["run".to_string()]);
        let direct = vec!["opencode.exe".into(), "--version".into()];
        let argv = full_split(&direct, "fallback");
        assert_eq!(argv.0, "opencode.exe");
        assert_eq!(argv.1, vec!["--version".to_string()]);
    }

    #[test]
    fn detects_wired_service() {
        let status = service_config_status(41987);
        assert_eq!(status.path, Paths::opencode_service_config());
    }
}
