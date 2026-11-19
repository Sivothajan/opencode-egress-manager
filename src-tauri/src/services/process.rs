//! Process discovery and lifecycle helpers.

use std::time::Duration;

use crate::{
    errors::{AppError, Result},
    services::networking::run_hidden,
};

/// Create a fully refreshed `sysinfo::System` snapshot.
///
/// Returns `None` when the OS refuses to expose process information; callers then
/// skip the process-based checks instead of failing the whole operation.
pub fn system_all() -> Option<sysinfo::System> {
    sysinfo::System::new_all().ok()
}

/// Stop running worker processes for a role.
///
/// Matches this application's `--worker <role>` argument and the legacy Python
/// worker scripts, so an upgrade does not leave two keepers fighting over WARP.
pub async fn stop_worker_processes(role: &str) -> (u32, String) {
    if role != "keeper" && role != "guard" {
        return (0, format!("unknown worker role: {role}"));
    }
    let Some(system) = system_all() else {
        return (0, "process enumeration unavailable".into());
    };
    let mut targets: Vec<u32> = Vec::new();
    for (pid, process) in system.processes() {
        if pid.as_u32() == std::process::id() {
            continue;
        }
        let cmdline = process
            .cmd()
            .iter()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join(" ");
        // Only ever stop this application's own workers. The legacy Python app
        // owns its keeper/guard processes and must not be touched.
        if cmdline.contains(&format!("--worker {role}")) {
            targets.push(pid.as_u32());
        }
    }
    let mut killed = 0;
    let mut errors = Vec::new();
    for pid in targets {
        match run_hidden(
            "taskkill",
            &["/PID".into(), pid.to_string(), "/T".into(), "/F".into()],
            5,
        )
        .await
        {
            Ok((0, _)) | Ok((128, _)) => killed += 1,
            Ok((_, out)) => errors.push(format!("PID {pid}: {}", out.trim())),
            Err(err) => errors.push(format!("PID {pid}: {}", err.user_message())),
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let message = if errors.is_empty() {
        String::new()
    } else {
        errors[..errors.len().min(3)].join("; ")
    };
    (killed, message)
}

/// Spawn a detached worker process running this same executable.
pub fn spawn_worker(app_exe: &std::path::Path, role: &str) -> Result<()> {
    if role != "keeper" && role != "guard" {
        return Err(AppError::validation(format!("unknown worker role: {role}")));
    }
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new(app_exe);
    cmd.arg("--worker")
        .arg(role)
        .creation_flags(0x0800_0000 | 0x0000_0008)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd.spawn()
        .map_err(|e| AppError::Process(format!("failed to spawn {role}: {e}")))?;
    Ok(())
}

/// Wait until a loopback port answers or the deadline elapses.
pub async fn wait_for_port(port: u16, timeout_secs: u64) -> bool {
    let deadline = Duration::from_secs(timeout_secs);
    let start = std::time::Instant::now();
    while start.elapsed() < deadline {
        if crate::services::networking::port_open("127.0.0.1", port, 400).await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}
