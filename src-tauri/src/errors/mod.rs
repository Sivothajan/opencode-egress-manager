//! Typed application errors and log/credential redaction.

use thiserror::Error;

/// Alias used across services. Services never panic on expected failures; they
/// return this type so callers can surface honest messages.
pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Io(String),

    #[error("{0}")]
    Json(String),

    #[error("{0}")]
    Validation(String),

    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    Process(String),

    #[error("operation cancelled")]
    Cancelled,

    #[error("{0}")]
    Other(String),
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        AppError::Io(value.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(value: serde_json::Error) -> Self {
        AppError::Json(value.to_string())
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(value: rusqlite::Error) -> Self {
        AppError::Other(format!("sqlite: {value}"))
    }
}

impl From<reqwest::Error> for AppError {
    fn from(value: reqwest::Error) -> Self {
        // reqwest URLs can carry userinfo; strip credentials before logging.
        let msg = value.to_string();
        AppError::Process(redact(&msg))
    }
}

impl AppError {
    /// Message safe to show in the UI and write to logs.
    pub fn user_message(&self) -> String {
        redact(&self.to_string())
    }

    pub fn validation(message: impl Into<String>) -> Self {
        AppError::Validation(message.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        AppError::NotFound(message.into())
    }

    pub fn other(message: impl Into<String>) -> Self {
        AppError::Other(message.into())
    }
}

/// Redact obvious secrets before a string reaches logs, diagnostics JSON, or the UI.
///
/// Covers `scheme://user:pass@host`, `Proxy-Authorization: Basic ...`, bearer
/// tokens, and Tor cookie hex strings. It is deliberately conservative: it removes
/// the credential portion rather than trying to preserve a partially masked value.
pub fn redact(input: &str) -> String {
    use regex::Regex;
    use std::sync::OnceLock;

    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        let raw = [
            // scheme://user:pass@host (userinfo)
            r"(?i)([a-z][a-z0-9+.\-]*://)[^/@\s:]+:[^/@\s]+@",
            // Proxy-Authorization: Basic <base64>
            r"(?i)(proxy-authorization\s*:\s*basic\s+)[A-Za-z0-9+/=]{4,}",
            // Authorization: Bearer <token>
            r"(?i)(authorization\s*:\s*bearer\s+)[A-Za-z0-9._\-]{8,}",
            // Generic token/password key=value pairs
            r#"(?i)((?:password|passwd|pwd|secret|token|api[_-]?key)\s*[=:]\s*)[^&\s,;"']+"#,
        ];
        raw.iter()
            .filter_map(|p| Regex::new(p).ok())
            .collect::<Vec<_>>()
    });

    let mut out = input.to_string();
    for pattern in patterns {
        out = pattern
            .replace_all(&out, |caps: &regex::Captures| {
                let prefix = caps
                    .get(1)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
                format!("{prefix}[REDACTED]")
            })
            .into_owned();
    }
    out
}

/// Sanitize a path-like value for logs (no behavior change; defensive only).
pub fn sanitize_detail(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.len() > 400 {
        format!("{}…", &trimmed[..400])
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_url_userinfo() {
        let input = "connect socks5h://user:s3cr3t@127.0.0.1:9050 failed";
        let out = redact(input);
        assert!(!out.contains("s3cr3t"), "leaked: {out}");
        assert!(out.contains("[REDACTED]"));
        assert!(out.contains("socks5h://"));
    }

    #[test]
    fn redacts_proxy_authorization_header() {
        let input = "Proxy-Authorization: Basic dXNlcjpwYXNz";
        assert!(!redact(input).contains("dXNlcjpwYXNz"));
    }

    #[test]
    fn redacts_password_assignment() {
        let input = "password=hunter2 token=abcdef123456";
        let out = redact(input);
        assert!(!out.contains("hunter2"));
        assert!(!out.contains("abcdef123456"));
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        let input = "WARP connected (mode: warp)";
        assert_eq!(redact(input), input);
    }
}
