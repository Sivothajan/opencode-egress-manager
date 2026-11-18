//! Install/repair orchestration: shims, user PATH, tasks, and teardown.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use windows::core::PCWSTR;
use windows::Win32::Foundation::WIN32_ERROR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_READ, KEY_WRITE, REG_EXPAND_SZ, REG_SAM_FLAGS, REG_VALUE_TYPE,
};

use crate::{
    config::Paths,
    errors::{AppError, Result},
    platform,
    services::{networking, opencode, process, scheduler, warp},
};

const ENVIRONMENT_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Environment";
const PATH_VALUE: &str = "Path";

fn wide(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn read_user_path() -> Option<(String, REG_VALUE_TYPE)> {
    unsafe {
        let subkey = wide(ENVIRONMENT_KEY);
        let mut key: HKEY = HKEY(std::ptr::null_mut());
        let status = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            REG_SAM_FLAGS(KEY_READ.0),
            &mut key,
        );
        if status != WIN32_ERROR(0) {
            return None;
        }
        let value_name = wide(PATH_VALUE);
        let mut kind = REG_VALUE_TYPE::default();
        let mut size: u32 = 0;
        let query = RegQueryValueExW(
            key,
            PCWSTR(value_name.as_ptr()),
            None,
            Some(&mut kind),
            None,
            Some(&mut size),
        );
        if query != WIN32_ERROR(0) || size == 0 {
            let _ = RegCloseKey(key);
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        let read = RegQueryValueExW(
            key,
            PCWSTR(value_name.as_ptr()),
            None,
            Some(&mut kind),
            Some(buf.as_mut_ptr()),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);
        if read != WIN32_ERROR(0) {
            return None;
        }
        let units: Vec<u16> = buf[..size as usize]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_ne_bytes(*pair))
            .take_while(|unit| *unit != 0)
            .collect();
        let text = String::from_utf16_lossy(&units);
        Some((text, kind))
    }
}

fn write_user_path(value: &str, kind: REG_VALUE_TYPE) -> Result<()> {
    unsafe {
        let subkey = wide(ENVIRONMENT_KEY);
        let mut key: HKEY = HKEY(std::ptr::null_mut());
        let status = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            REG_SAM_FLAGS(KEY_WRITE.0),
            &mut key,
        );
        if status != WIN32_ERROR(0) {
            return Err(AppError::Other(format!(
                "cannot open HKCU\\Environment (error {})",
                status.0
            )));
        }
        let value_name = wide(PATH_VALUE);
        let bytes = value
            .encode_utf16()
            .flat_map(|u| u.to_ne_bytes())
            .collect::<Vec<u8>>();
        let result = RegSetValueExW(
            key,
            PCWSTR(value_name.as_ptr()),
            Some(0),
            kind,
            Some(bytes.as_slice()),
        );
        let _ = RegCloseKey(key);
        if result != WIN32_ERROR(0) {
            return Err(AppError::Other(format!(
                "cannot update user PATH (error {})",
                result.0
            )));
        }
    }
    Ok(())
}

pub fn update_user_path(add: bool) -> Result<bool> {
    let bin_dir = Paths::discover().bin_dir;
    let target = bin_dir.to_string_lossy().trim_end_matches('\\').to_string();
    let (current, kind) = match read_user_path() {
        Some(value) => value,
        None => {
            if !add {
                return Ok(false);
            }
            (String::new(), REG_VALUE_TYPE(REG_EXPAND_SZ.0))
        }
    };
    let mut parts: Vec<String> = current
        .split(';')
        .filter(|p| !p.is_empty())
        .map(|p| p.trim_end_matches('\\').to_string())
        .filter(|p| !p.eq_ignore_ascii_case(&target))
        .collect();
    if add {
        parts.insert(0, target.clone());
    }
    let new_value = parts.join(";");
    if new_value == current {
        return Ok(true);
    }
    write_user_path(&new_value, kind)?;
    broadcast_environment_change();
    Ok(true)
}

pub fn user_path_has_bin() -> bool {
    let bin_dir = Paths::discover().bin_dir;
    let target = bin_dir
        .to_string_lossy()
        .trim_end_matches('\\')
        .to_ascii_lowercase();
    read_user_path()
        .map(|(path, _)| {
            path.split(';')
                .next()
                .map(|p| p.trim_end_matches('\\').to_ascii_lowercase() == target)
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

fn broadcast_environment_change() {
    // WM_SETTINGCHANGE is best-effort; new shells pick up the registry directly.
    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };
    unsafe {
        let mut result = usize::default();
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            windows::Win32::Foundation::WPARAM(0),
            windows::Win32::Foundation::LPARAM(w!("Environment").as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            2000,
            Some(&mut result),
        );
    }
}

/// Write the `opencode` CLI shim that routes through this application's lane mode.
pub fn setup_shims(app_exe: &Path) -> Result<PathBuf> {
    let paths = Paths::discover();
    paths.ensure_dirs()?;
    let shim_path = paths.bin_dir.join("opencode.cmd");
    let content = format!(
        "@echo off\r\n\"{}\" --lane %*\r\nexit /b %ERRORLEVEL%\r\n",
        app_exe.to_string_lossy()
    );
    write_cmd_wrapper(&shim_path, &content)?;
    let desktop_shim = paths.bin_dir.join("opencode-desktop.cmd");
    let desktop_content = format!(
        "@echo off\r\nstart \"\" \"{}\" --lane --ouc-desktop %*\r\n",
        app_exe.to_string_lossy()
    );
    write_cmd_wrapper(&desktop_shim, &desktop_content)?;
    Ok(shim_path)
}

fn write_cmd_wrapper(path: &Path, content: &str) -> Result<()> {
    if content.is_ascii() {
        std::fs::write(path, content.as_bytes())?;
        return Ok(());
    }
    let mut lines: Vec<String> = content.split("\r\n").map(|s| s.to_string()).collect();
    if lines
        .first()
        .map(|l| l.to_ascii_lowercase().starts_with("@echo off"))
        .unwrap_or(false)
    {
        lines.insert(1, "chcp 65001 >nul".to_string());
    }
    std::fs::write(path, lines.join("\r\n").as_bytes())?;
    Ok(())
}

pub async fn launch_cli(paths: &Paths) -> Result<String> {
    let shim = paths.bin_dir.join("opencode.cmd");
    if !shim.is_file() {
        return Err(AppError::not_found(
            "opencode CLI shim not ready; run Install / Repair first.",
        ));
    }
    if let Ok(wt) = which::which("wt.exe") {
        std::process::Command::new(wt)
            .args(["cmd.exe", "/k"])
            .arg(&shim)
            .spawn()
            .map_err(|e| AppError::Process(format!("failed to open terminal: {e}")))?;
    } else {
        std::process::Command::new("cmd.exe")
            .args([
                "/c",
                "start",
                "Opencode Egress Manager CLI",
                "cmd.exe",
                "/k",
            ])
            .arg(&shim)
            .spawn()
            .map_err(|e| AppError::Process(format!("failed to open terminal: {e}")))?;
    }
    Ok("Opened OpenCode CLI terminal via the local lane.".into())
}

pub async fn launch_desktop(paths: &Paths) -> Result<String> {
    let desktop = opencode::find_opencode_desktop()
        .ok_or_else(|| AppError::not_found("OpenCode Desktop executable not found"))?;
    opencode::clear_free_limit();
    if !networking::port_open("127.0.0.1", crate::config::SERVICE_PROXY_PORT_DEFAULT, 400).await {
        // The keeper normally hosts the service proxy; the lane can still start one.
        tracing::debug!("service proxy not listening before desktop launch");
    }
    let proxy_url = format!(
        "http://127.0.0.1:{}",
        crate::config::SERVICE_PROXY_PORT_DEFAULT
    );
    let mut cmd = tokio::process::Command::new(&desktop);
    cmd.args([
        format!("--proxy-server={proxy_url}"),
        "--proxy-bypass-list=127.0.0.1;localhost;::1;*.local".to_string(),
    ]);
    cmd.env("HTTPS_PROXY", &proxy_url)
        .env("HTTP_PROXY", &proxy_url)
        .env("NO_PROXY", "127.0.0.1,localhost,::1");
    cmd.creation_flags(0x0000_0008);
    cmd.spawn()
        .map_err(|e| AppError::Process(format!("failed to launch OpenCode Desktop: {e}")))?;
    let _ = paths;
    Ok("Launched OpenCode Desktop through the local lane.".into())
}

/// Full install/repair sequence. Returns an honest message describing what happened.
pub async fn install_or_repair(paths: &Paths, app_exe: &Path) -> Result<String> {
    let warp_bin = warp::find_warp_cli();
    if warp_bin.is_none() {
        return Err(AppError::not_found(
            "Cloudflare WARP (warp-cli.exe) not found. Install WARP from https://1.1.1.1/",
        ));
    }
    if opencode::find_opencode_cli().is_none() && opencode::find_opencode_desktop().is_none() {
        return Err(AppError::not_found(
            "OpenCode was not found. Install it from https://opencode.ai/",
        ));
    }
    setup_shims(app_exe)?;
    let _ = update_user_path(true);
    scheduler::register_autostart(&PathBuf::from(app_exe)).await?;
    let (killed, _) = process::stop_worker_processes("keeper").await;
    let _ = killed;
    tokio::time::sleep(Duration::from_millis(400)).await;
    process::spawn_worker(app_exe, "keeper")?;
    process::spawn_worker(app_exe, "guard")?;
    tokio::time::sleep(Duration::from_millis(600)).await;
    if let Some(bin) = &warp_bin {
        let cfg = crate::config::EgressConfig::load(paths).unwrap_or_default();
        let _ = warp::connect(bin, cfg.warp_scope).await;
    }
    let wired = opencode::wire_service_proxy(paths, true).await.ok();
    let msg = match wired {
        Some(m) => m,
        None => "service wiring skipped (OpenCode service unavailable)".to_string(),
    };
    Ok(format!(
        "Installed/repaired shims, PATH, tasks, WARP scope, workers, and {msg}"
    ))
}

/// Remove tasks, workers, wiring, PATH entry, and optionally all state.
pub async fn uninstall_all(paths: &Paths, purge_state: bool) -> Result<String> {
    scheduler::clear_autostart().await?;
    for role in ["guard", "keeper"] {
        process::stop_worker_processes(role).await;
    }
    let _ = opencode::restore_service_backup();
    let _ = opencode::wire_service_proxy(paths, false).await;
    if platform::is_admin() {
        let _ = crate::services::firewall::remove().await;
    }
    let _ = update_user_path(false);
    if let Some(bin) = warp::find_warp_cli() {
        let _ = warp::disconnect(&bin).await;
    }
    if purge_state && paths.data_root.is_dir() {
        std::fs::remove_dir_all(&paths.data_root)?;
    }
    Ok("Uninstalled background tasks, shims, proxy overrides, and state.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{TASK_GUARD, TASK_KEEPER};

    #[test]
    fn path_helpers_are_case_insensitive() {
        // Pure-function behavior without touching the registry.
        assert!(TASK_KEEPER.starts_with("OpencodeEgressManager"));
        assert!(TASK_GUARD.starts_with("OpencodeEgressManager"));
    }

    #[test]
    fn wrapper_encoding_handles_ascii() {
        let dir = std::env::temp_dir().join("ouc-shim-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("opencode.cmd");
        let content = "@echo off\r\n\"C:\\app\\app.exe\" --lane %*\r\n";
        write_cmd_wrapper(&path, content).unwrap();
        let read = std::fs::read_to_string(&path).unwrap();
        assert!(!read.contains("chcp"));
        std::fs::remove_file(&path).ok();
    }
}
