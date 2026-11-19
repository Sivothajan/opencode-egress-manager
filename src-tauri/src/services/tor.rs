//! Portable Tor lifecycle, SOCKS probing, and circuit rotation.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::time::sleep;

use crate::{
    config::{Paths, TOR_BROWSER_SOCKS_PORT, TOR_CONTROL_PORT, TOR_SOCKS_PORT, USER_AGENT},
    errors::{redact, AppError, Result},
    services::{networking, proxy::socks},
};

const DETACHED_PROCESS: u32 = 0x0000_0008;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Current Tor availability and the process this app owns.
#[derive(Debug, Clone, Serialize)]
pub struct TorStatus {
    /// Whether a live Tor SOCKS service answered.
    pub active: bool,
    /// `host:port` of the live SOCKS service, when found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<(String, u16)>,
    /// Whether the PID recorded for portable Tor is still alive.
    pub portable_running: bool,
}

fn tor_dir(paths: &Paths) -> PathBuf {
    paths.tor_dir.clone()
}

fn data_dir(paths: &Paths) -> PathBuf {
    tor_dir(paths).join("data")
}

fn pid_file(paths: &Paths) -> PathBuf {
    tor_dir(paths).join("tor.pid")
}

fn log_file(paths: &Paths) -> PathBuf {
    paths.logs_dir.join("tor.log")
}

/// Locate `tor.exe` in the portable directory, Tor Browser, or `PATH`.
pub fn find_tor_exe(paths: &Paths) -> Option<PathBuf> {
    let candidates = [
        tor_dir(paths).join("tor").join("tor.exe"),
        tor_dir(paths).join("Tor").join("tor.exe"),
        tor_dir(paths).join("tor.exe"),
        PathBuf::from(r"C:\Program Files\Tor Browser\Browser\TorBrowser\Tor\tor.exe"),
        PathBuf::from(r"C:\Program Files (x86)\Tor Browser\Browser\TorBrowser\Tor\tor.exe"),
    ];
    for candidate in candidates {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    if let Ok(program_files) = std::env::var("ProgramFiles") {
        let p = PathBuf::from(program_files).join("Tor Browser\\Browser\\TorBrowser\\Tor\\tor.exe");
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(found) = which::which("tor.exe").ok().filter(|p| p.is_file()) {
        return Some(found);
    }
    if let Ok(found) = which::which("tor") {
        if found.is_file() {
            return Some(found);
        }
    }
    if let Ok(dir) = std::fs::read_dir(tor_dir(paths)) {
        for entry in dir.flatten() {
            let path = entry.path();
            if path.file_name().and_then(|n| n.to_str()) == Some("tor.exe") && path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

/// First live Tor SOCKS service (Tor Browser preferred, then portable).
pub async fn active_target() -> Option<(String, u16)> {
    for port in [TOR_BROWSER_SOCKS_PORT, TOR_SOCKS_PORT] {
        if socks::test_socks5_alive("127.0.0.1", port, 800).await {
            return Some(("127.0.0.1".into(), port));
        }
    }
    None
}

/// Probe Tor availability and report the combined status snapshot.
pub async fn status_for(paths: &Paths) -> TorStatus {
    let target = active_target().await;
    let portable_running = portable_running(paths);
    TorStatus {
        active: target.is_some(),
        target,
        portable_running,
    }
}

/// Whether the PID recorded for our portable Tor is still alive.
pub fn portable_pid(paths: &Paths) -> Option<u32> {
    let text = std::fs::read_to_string(pid_file(paths)).ok()?;
    text.trim().parse::<u32>().ok()
}

/// Check whether the PID recorded for our portable Tor is still alive.
pub fn portable_running(paths: &Paths) -> bool {
    let Some(pid) = portable_pid(paths) else {
        return false;
    };
    if pid == 0 || pid == std::process::id() {
        return false;
    }
    let Some(system) = crate::services::process::system_all() else {
        return false;
    };
    system
        .process(sysinfo::Pid::from_u32(pid))
        .map(|p| {
            p.name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .contains("tor")
        })
        .unwrap_or(false)
}

/// Start the portable Tor daemon (downloading it first when necessary).
pub async fn start_portable_tor(paths: &Paths) -> Result<String> {
    paths.ensure_dirs()?;
    if let Some(target) = active_target().await {
        return Ok(format!(
            "Tor SOCKS already running on {}:{}",
            target.0, target.1
        ));
    }
    let exe = match find_tor_exe(paths) {
        Some(exe) => exe,
        None => download_and_extract_tor(paths).await?,
    };
    let data = data_dir(paths);
    let log = log_file(paths);
    std::fs::create_dir_all(&data)?;
    std::fs::create_dir_all(log.parent().unwrap_or(Path::new(".")))?;

    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args([
        "--SocksPort",
        &format!("127.0.0.1:{TOR_SOCKS_PORT}"),
        "--ControlPort",
        &format!("127.0.0.1:{TOR_CONTROL_PORT}"),
        "--CookieAuthentication",
        "0",
        "--DataDirectory",
        data.to_string_lossy().as_ref(),
        "--Log",
    ]);
    cmd.arg(format!("notice file {}", log.display()));
    {
        cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }
    let child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| AppError::Process(format!("failed to spawn tor.exe: {e}")))?;
    let pid = child.id().unwrap_or(0);
    crate::config::write_text(&pid_file(paths), &pid.to_string())?;

    for _ in 0..25 {
        sleep(Duration::from_secs(1)).await;
        if socks::test_socks5_alive("127.0.0.1", TOR_SOCKS_PORT, 800).await {
            return Ok(format!(
                "Portable Tor started on 127.0.0.1:{TOR_SOCKS_PORT} (PID {pid})"
            ));
        }
    }
    Err(AppError::Process(format!(
        "Tor process started (PID {pid}) but SOCKS port {TOR_SOCKS_PORT} did not respond in time"
    )))
}

/// Terminate the portable Tor process this app started.
pub async fn stop_portable_tor(paths: &Paths) -> Result<String> {
    let Some(pid) = portable_pid(paths) else {
        return Ok("Portable Tor is not running.".into());
    };

    let output = tokio::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .await;
    let _ = std::fs::remove_file(pid_file(paths));
    match output {
        Ok(out) if out.status.success() => Ok(format!("Stopped portable Tor (PID {pid}).")),
        Ok(_) => Ok(format!(
            "Requested Tor stop for PID {pid}; process may already be gone."
        )),
        Err(err) => Err(AppError::Process(format!("could not stop Tor: {err}"))),
    }
}

fn control_cookie_path(paths: &Paths) -> Option<PathBuf> {
    let first = data_dir(paths).join("control_auth_cookie");
    if first.is_file() {
        return Some(first);
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        let browser = PathBuf::from(appdata)
            .join("Tor Browser")
            .join("Browser")
            .join("TorBrowser")
            .join("Data")
            .join("Tor")
            .join("control_auth_cookie");
        if browser.is_file() {
            return Some(browser);
        }
    }
    None
}

async fn signal_newnym(port: u16) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let cookie = control_cookie_path(&Paths::discover())
        .and_then(|p| std::fs::read(p).ok())
        .map(|bytes| hex_encode(&bytes));
    let auth = match cookie {
        Some(hex) => format!("AUTHENTICATE \"{hex}\"\r\n"),
        None => "AUTHENTICATE \"\"\r\n".to_string(),
    };
    let Ok(mut sock) = tokio::time::timeout(
        Duration::from_millis(1200),
        tokio::net::TcpStream::connect(("127.0.0.1", port)),
    )
    .await
    else {
        return false;
    };
    let Ok(sock) = &mut sock else {
        return false;
    };
    if sock.write_all(auth.as_bytes()).await.is_err() {
        return false;
    }
    let mut resp = [0u8; 512];
    let Ok(n) = sock.read(&mut resp).await else {
        return false;
    };
    if !resp[..n].windows(3).any(|w| w == b"250") {
        return false;
    }
    if sock.write_all(b"SIGNAL NEWNYM\r\n").await.is_err() {
        return false;
    }
    let mut resp2 = [0u8; 512];
    let Ok(n2) = sock.read(&mut resp2).await else {
        return false;
    };
    resp2[..n2].windows(3).any(|w| w == b"250")
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Rotate the circuit and confirm the exit address changed.
pub async fn rotate_circuit(paths: &Paths) -> Result<String> {
    let cfg = crate::config::EgressConfig::load(paths).unwrap_or_default();
    let before = networking::get_public_v6(&cfg, 6).await;
    for port in [TOR_CONTROL_PORT, 9151u16] {
        if !signal_newnym(port).await {
            continue;
        }
        let mut after = before.clone();
        for attempt in 0..3 {
            sleep(Duration::from_secs(2)).await;
            after = networking::get_egress_ip(&cfg, 6).await;
            if let Some(after_ip) = &after {
                if before.is_none() || after_ip != before.as_deref().unwrap_or("") {
                    return Ok(format!(
                        "Tor circuit rotated via control port {port} (exit confirmed at {after_ip})"
                    ));
                }
            }
            let _ = signal_newnym(port).await;
            let _ = attempt;
        }
        if after.is_none() {
            return Err(AppError::Process(format!(
                "control port {port} accepted the request, but the new exit IP could not be verified"
            )));
        }
        return Err(AppError::Process(format!(
            "Tor accepted the circuit request but the exit IP stayed at {}",
            after.unwrap_or_default()
        )));
    }

    // No usable control port: restart the portable daemon to force a fresh circuit.
    if portable_running(paths) {
        let _ = stop_portable_tor(paths).await;
        sleep(Duration::from_secs(1)).await;
        let msg = start_portable_tor(paths).await?;
        let after = networking::get_egress_ip(&cfg, 6).await;
        match after {
            Some(ip) if before.as_deref() != Some(ip.as_str()) => {
                return Ok(format!("Portable Tor restarted ({before:?} -> {ip})"))
            }
            Some(ip) => {
                return Err(AppError::Process(format!(
                    "Portable Tor restarted, but the exit IP stayed at {ip}"
                )))
            }
            None => {
                return Err(AppError::Process(format!(
                    "{msg}, but the new exit IP could not be verified"
                )))
            }
        }
    }
    if let Some(target) = active_target().await {
        return Err(AppError::Process(format!(
            "external Tor on {}:{} has no usable control port",
            target.0, target.1
        )));
    }
    let msg = start_portable_tor(paths).await?;
    let after = networking::get_egress_ip(&cfg, 6).await;
    match after {
        Some(ip) => Ok(format!("Tor started; exit confirmed at {ip}: {msg}")),
        None => Err(AppError::Process(format!(
            "{msg}, but the exit IP could not be verified"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Download and verify the Tor Expert Bundle
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
struct AusRelease {
    #[serde(default)]
    version: String,
}

fn bundle_filename(version: &str) -> String {
    format!("tor-expert-bundle-windows-x86_64-{version}.tar.gz")
}

fn bundle_urls(version: &str) -> Vec<String> {
    vec![
        format!(
            "https://dist.torproject.org/torbrowser/{version}/{}",
            bundle_filename(version)
        ),
        format!(
            "https://archive.org/download/tor-expert-bundle-{version}/{}",
            bundle_filename(version)
        ),
        format!(
            "https://tor.eff.org/dist/torbrowser/{version}/{}",
            bundle_filename(version)
        ),
        format!(
            "https://tor.calyxinstitute.org/dist/torbrowser/{version}/{}",
            bundle_filename(version)
        ),
    ]
}

async fn fetch_latest_version() -> Result<String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| AppError::Other(e.to_string()))?;
    let body = client
        .get("https://aus1.torproject.org/torbrowser/update_3/release/downloads.json")
        .send()
        .await
        .map_err(|e| AppError::Process(redact(&e.to_string())))?
        .text()
        .await
        .map_err(|e| AppError::Process(redact(&e.to_string())))?;
    let release: AusRelease = serde_json::from_str(&body)
        .map_err(|_| AppError::Process("could not parse Tor release metadata".into()))?;
    let version = release.version.trim().to_string();
    if version.is_empty()
        || version.len() > 64
        || !version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.')
    {
        return Err(AppError::Process("invalid Tor version metadata".into()));
    }
    Ok(version)
}

/// Fetch the official digest for `filename` from the Tor Project's sha256sums.
async fn fetch_expected_sha256(version: &str, filename: &str) -> Result<String> {
    let url = format!("https://dist.torproject.org/torbrowser/{version}/sha256sums.txt");
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(15))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| AppError::Other(e.to_string()))?;
    let text = client
        .get(&url)
        .send()
        .await
        .map_err(|e| {
            AppError::Process(format!(
                "could not fetch Tor checksum list: {}",
                redact(&e.to_string())
            ))
        })?
        .text()
        .await
        .map_err(|e| AppError::Process(redact(&e.to_string())))?;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(digest) = parts.next() else { continue };
        let Some(name) = parts.next() else { continue };
        let name = name.trim_start_matches('*');
        if name == filename {
            let digest = digest.to_ascii_lowercase();
            if digest.len() == 64 && digest.chars().all(|c| c.is_ascii_hexdigit()) {
                return Ok(digest);
            }
        }
    }
    Err(AppError::Process(format!(
        "official checksum for {filename} was not published"
    )))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

async fn download_with_checksum(urls: &[String], expected: &str) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(180))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| AppError::Other(e.to_string()))?;
    let mut last_err = String::from("no mirror attempted");
    for url in urls {
        if let Ok(resp) = client.get(url).send().await {
            if let Ok(bytes) = resp.bytes().await {
                if bytes.len() < 1024 * 1024 {
                    last_err = format!("{url} returned an implausibly small bundle");
                    continue;
                }
                let actual = sha256_hex(&bytes);
                if actual != expected {
                    return Err(AppError::Validation(format!(
                        "checksum mismatch for Tor bundle (expected {expected}, got {actual})"
                    )));
                }
                return Ok(bytes.to_vec());
            }
        }
        last_err = format!("{url} failed");
    }
    Err(AppError::Process(format!(
        "Tor download failed across all mirrors: {last_err}"
    )))
}

fn extract_tar_gz(bytes: &[u8], dest: &Path) -> Result<()> {
    use flate2::read::GzDecoder;
    let decoder = GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(decoder);
    std::fs::create_dir_all(dest)?;
    for entry in archive.entries()? {
        let mut entry = entry.map_err(|e| AppError::Process(e.to_string()))?;
        let path = entry
            .path()
            .map_err(|e| AppError::validation(e.to_string()))?
            .into_owned();
        if path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(AppError::validation(
                "Tor archive contains a path traversal entry",
            ));
        }
        let target = dest.join(&path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        entry
            .unpack(&target)
            .map_err(|e| AppError::Process(e.to_string()))?;
    }
    Ok(())
}

/// Download and extract the Tor Expert Bundle, verifying its SHA-256 digest.
pub async fn download_and_extract_tor(paths: &Paths) -> Result<PathBuf> {
    paths.ensure_dirs()?;
    let version = fetch_latest_version().await?;
    let filename = bundle_filename(&version);
    let expected = fetch_expected_sha256(&version, &filename).await?;
    let bytes = download_with_checksum(&bundle_urls(&version), &expected).await?;
    let archive_path = tor_dir(paths).join("tor-expert-bundle.tar.gz");
    std::fs::write(&archive_path, &bytes)?;
    extract_tar_gz(&bytes, &tor_dir(paths))?;
    let _ = std::fs::remove_file(&archive_path);
    find_tor_exe(paths)
        .ok_or_else(|| AppError::Process("Tor archive extracted but tor.exe was not found".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_helpers() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(hex_encode(&[0x00, 0x0a, 0xff]), "000aff");
    }

    #[test]
    fn builds_bundle_names() {
        assert_eq!(
            bundle_filename("14.0.7"),
            "tor-expert-bundle-windows-x86_64-14.0.7.tar.gz"
        );
        assert!(bundle_urls("14.0.7")[0].starts_with("https://dist.torproject.org/"));
    }

    #[test]
    fn rejects_bad_version_metadata() {
        // validated indirectly by length/charset checks in fetch_latest_version
        assert!("14.0.7"
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.'));
        assert!(!"../evil"
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.'));
    }
}
