//! Windows Firewall kill-switch helpers (privileged).
//!
//! Only the separate `egress-manager-admin` binary performs mutations. The UI asks
//! that helper to run elevated once per operation; the main application is never
//! elevated.

use std::path::{Component, PathBuf};

use crate::{
    config::FIREWALL_RULE_NAME,
    errors::{AppError, Result},
    platform,
    services::networking::run_hidden,
};

const PUBLIC_RANGES: &str = "0.0.0.0-9.255.255.255,11.0.0.0-126.255.255.255,128.0.0.0-172.15.255.255,172.32.0.0-192.167.255.255,192.169.0.0-255.255.255.255,2000::/3";

/// Validate a candidate OpenCode CLI path before embedding it in a firewall rule.
///
/// Rules:
/// * absolute canonical path, `.exe` suffix
/// * contains no parent-traversal components or shell metacharacters
/// * lives under an allowlisted OpenCode install root
pub fn validate_cli_path(input: &str) -> Result<PathBuf> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AppError::validation("CLI path must not be empty"));
    }
    if trimmed
        .chars()
        .any(|c| matches!(c, '"' | '\'' | ';' | '&' | '|' | '>' | '<' | '^' | '`'))
    {
        return Err(AppError::validation(
            "CLI path contains forbidden characters",
        ));
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(AppError::validation("CLI path must be absolute"));
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| AppError::validation("CLI path does not exist"))?;
    if canonical.extension().and_then(|e| e.to_str()) != Some("exe") {
        return Err(AppError::validation("CLI path must point to a .exe"));
    }
    if canonical
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(AppError::validation("CLI path may not contain traversal"));
    }
    let roots = allowed_roots();
    let allowed = roots.iter().any(|root| canonical.starts_with(root));
    if !allowed {
        return Err(AppError::validation(format!(
            "CLI path is outside the allowed OpenCode locations: {}",
            canonical.display()
        )));
    }
    Ok(canonical)
}

fn allowed_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        roots.push(
            PathBuf::from(&local)
                .join("Programs")
                .join("ai.opencode.desktop"),
        );
        roots.push(PathBuf::from(&local).join("opencode"));
    }
    if let Ok(home) = std::env::var("USERPROFILE") {
        roots.push(PathBuf::from(&home).join(".opencode"));
        roots.push(PathBuf::from(&home).join(".bun"));
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        roots.push(PathBuf::from(&appdata).join("npm"));
    }
    roots.push(PathBuf::from(r"C:\Program Files\OpenCode"));
    roots.push(PathBuf::from(r"C:\Program Files\ai.opencode.desktop"));
    roots.push(PathBuf::from(r"C:\Program Files (x86)\OpenCode"));
    roots.push(PathBuf::from(r"C:\Program Files (x86)\ai.opencode.desktop"));
    roots
}

async fn rule_exists(name: &str) -> bool {
    match run_hidden(
        "netsh",
        &[
            "advfirewall".into(),
            "firewall".into(),
            "show".into(),
            "rule".into(),
            format!("name={name}"),
        ],
        6,
    )
    .await
    {
        Ok((code, out)) => code == 0 && out.contains(name),
        Err(_) => false,
    }
}

pub async fn is_installed() -> bool {
    rule_exists(FIREWALL_RULE_NAME).await
}

/// True when the legacy Python app's kill-switch rule is present.
pub async fn is_python_installed() -> bool {
    rule_exists("OUC_LEAKGUARD_CLI").await
}

pub async fn install(cli_path: &str) -> Result<String> {
    if !platform::is_admin() {
        return Err(AppError::Validation(
            "administrator privileges required for firewall kill-switch".into(),
        ));
    }
    let canonical = validate_cli_path(cli_path)?;
    let _ = remove().await;
    let (code, out) = run_hidden(
        "netsh",
        &[
            "advfirewall".into(),
            "firewall".into(),
            "add".into(),
            "rule".into(),
            format!("name={FIREWALL_RULE_NAME}"),
            "dir=out".into(),
            "action=block".into(),
            format!("program={}", canonical.display()),
            format!("remoteip={PUBLIC_RANGES}"),
            "profile=any".into(),
            "enable=yes".into(),
            "description=Opencode Egress Manager: force opencode-cli through the local proxy/WARP"
                .into(),
        ],
        10,
    )
    .await?;
    if code == 0 {
        return Ok(format!(
            "installed rule {FIREWALL_RULE_NAME} for {}",
            canonical.display()
        ));
    }
    Err(AppError::Process(out.trim().to_string()))
}

pub async fn remove() -> Result<String> {
    if !platform::is_admin() {
        return Err(AppError::Validation(
            "administrator privileges required to remove firewall rules".into(),
        ));
    }
    let (code, out) = run_hidden(
        "netsh",
        &[
            "advfirewall".into(),
            "firewall".into(),
            "delete".into(),
            "rule".into(),
            format!("name={FIREWALL_RULE_NAME}"),
        ],
        8,
    )
    .await?;
    if code == 0 || out.to_ascii_lowercase().contains("no rules match") {
        return Ok(format!("removed rule {FIREWALL_RULE_NAME}"));
    }
    Err(AppError::Process(out.trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal_and_relative_paths() {
        assert!(validate_cli_path(r"..\Windows\System32\cmd.exe").is_err());
        assert!(validate_cli_path("relative\\opencode.exe").is_err());
        assert!(validate_cli_path("").is_err());
        assert!(validate_cli_path("https://example.com/opencode.exe").is_err());
    }

    #[test]
    fn rejects_metacharacters() {
        assert!(validate_cli_path(r"C:\temp\a;b.exe").is_err());
        assert!(validate_cli_path(r"C:\temp\a&b.exe").is_err());
        assert!(validate_cli_path(r"C:\temp\a|b.exe").is_err());
    }
}
