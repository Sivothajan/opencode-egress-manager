//! Scheduled-task management via `schtasks` with generated task XML.
//!
//! No PowerShell string building is used: the action and arguments are embedded in
//! an escaped XML document written to a temp file.

use std::path::Path;

use crate::{
    config::{TASK_GUARD, TASK_KEEPER},
    errors::{AppError, Result},
    services::networking::run_hidden,
};

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn task_xml(exe: &str, args: &str) -> String {
    let user = format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap_or_else(|_| ".".into()),
        std::env::var("USERNAME").unwrap_or_else(|_| "user".into())
    );
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers><LogonTrigger><Enabled>true</Enabled></LogonTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <Enabled>true</Enabled>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <RestartOnFailure><Interval>PT1M</Interval><Count>3</Count></RestartOnFailure>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <StartWhenAvailable>true</StartWhenAvailable>
  </Settings>
  <Actions Context="Author"><Exec><Command>{exe}</Command><Arguments>{args}</Arguments></Exec></Actions>
</Task>"#
    )
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 128
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
    {
        return Err(AppError::validation("invalid scheduled task name"));
    }
    Ok(())
}

pub async fn task_exists(name: &str) -> bool {
    validate_name(name).ok();
    matches!(
        run_hidden(
            "schtasks",
            &["/Query".into(), "/TN".into(), name.to_string()],
            6,
        )
        .await,
        Ok((0, _))
    )
}

/// True only for an existing, enabled logon task.
pub async fn task_enabled(name: &str) -> bool {
    if validate_name(name).is_err() {
        return false;
    }
    match run_hidden(
        "schtasks",
        &[
            "/Query".into(),
            "/TN".into(),
            name.to_string(),
            "/XML".into(),
        ],
        6,
    )
    .await
    {
        Ok((0, text)) => {
            let cleaned = text.replace('\0', "");
            !cleaned
                .to_ascii_lowercase()
                .contains("<enabled>false</enabled>")
        }
        Ok((_, _)) => false,
        Err(_) => false,
    }
}

async fn create_task(name: &str, exe: &str, args: &str) -> Result<()> {
    validate_name(name)?;
    if !std::path::Path::new(exe).is_file() {
        return Err(AppError::validation(format!(
            "task executable does not exist: {exe}"
        )));
    }
    let xml = task_xml(&xml_escape(exe), &xml_escape(args));
    let temp_dir = std::env::temp_dir();
    let xml_path = temp_dir.join(format!("ouc-task-{name}.xml"));
    // Task Scheduler requires UTF-16LE with a byte-order mark.
    let utf16: Vec<u8> = std::iter::once(0xFFu8)
        .chain(std::iter::once(0xFEu8))
        .chain(xml.encode_utf16().flat_map(|u| u.to_le_bytes()))
        .collect();
    std::fs::write(&xml_path, utf16)?;
    let result = run_hidden(
        "schtasks",
        &[
            "/Create".into(),
            "/TN".into(),
            name.to_string(),
            "/XML".into(),
            xml_path.to_string_lossy().to_string(),
            "/F".into(),
        ],
        12,
    )
    .await;
    let _ = std::fs::remove_file(&xml_path);
    let (code, out) = result?;
    if code == 0 {
        Ok(())
    } else {
        Err(AppError::Process(out.trim().to_string()))
    }
}

pub async fn delete_task(name: &str) -> Result<()> {
    validate_name(name)?;
    let (code, out) = run_hidden(
        "schtasks",
        &[
            "/Delete".into(),
            "/TN".into(),
            name.to_string(),
            "/F".into(),
        ],
        8,
    )
    .await?;
    if code == 0 || out.to_ascii_lowercase().contains("cannot find") {
        return Ok(());
    }
    Err(AppError::Process(out.trim().to_string()))
}

pub async fn enable_task(name: &str) -> Result<()> {
    validate_name(name)?;
    let (code, out) = run_hidden(
        "schtasks",
        &[
            "/Change".into(),
            "/TN".into(),
            name.to_string(),
            "/ENABLE".into(),
        ],
        8,
    )
    .await?;
    if code == 0 {
        Ok(())
    } else {
        Err(AppError::Process(out.trim().to_string()))
    }
}

pub async fn disable_task(name: &str) -> Result<()> {
    validate_name(name)?;
    let (code, out) = run_hidden(
        "schtasks",
        &[
            "/Change".into(),
            "/TN".into(),
            name.to_string(),
            "/DISABLE".into(),
        ],
        8,
    )
    .await?;
    if code == 0 {
        Ok(())
    } else {
        Err(AppError::Process(out.trim().to_string()))
    }
}

pub async fn run_task(name: &str) -> Result<()> {
    validate_name(name)?;
    let (code, out) = run_hidden(
        "schtasks",
        &["/Run".into(), "/TN".into(), name.to_string()],
        8,
    )
    .await?;
    if code == 0 {
        Ok(())
    } else {
        Err(AppError::Process(out.trim().to_string()))
    }
}

/// Register logon tasks that run the worker roles.
pub async fn register_autostart(app_exe: &Path) -> Result<()> {
    let exe = app_exe.to_string_lossy().to_string();
    create_task(TASK_KEEPER, &exe, "--worker keeper").await?;
    create_task(TASK_GUARD, &exe, "--worker guard").await?;
    Ok(())
}

pub async fn clear_autostart() -> Result<()> {
    let _ = delete_task(TASK_KEEPER).await;
    let _ = delete_task(TASK_GUARD).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_xml() {
        assert_eq!(xml_escape("a&b<c>"), "a&amp;b&lt;c&gt;");
        let xml = task_xml(r"C:\app\egress.exe", "--worker keeper");
        assert!(xml.contains(r"c:\app\egress.exe") || xml.contains("C:\\app\\egress.exe"));
        let hostile = task_xml("x", &xml_escape("<script>"));
        assert!(hostile.contains("&lt;script&gt;"));
        assert!(!hostile.contains("<script>"));
    }

    #[test]
    fn rejects_bad_task_names() {
        assert!(validate_name("../../evil").is_err());
        assert!(validate_name("").is_err());
        assert!(validate_name(TASK_KEEPER).is_ok());
    }
}
