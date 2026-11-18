//! Tauri event names and emit helpers.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

pub const STATE_CHANGED: &str = "state-changed";
pub const PROBE_CHANGED: &str = "probe-changed";
pub const LOG_LINE: &str = "log-line";
pub const TOAST: &str = "toast";
pub const WORKER_STATE: &str = "worker-state";

pub fn emit<R: Runtime, T: Serialize + Clone>(app: &AppHandle<R>, event: &str, payload: T) {
    if let Err(err) = app.emit(event, payload) {
        tracing::debug!(error = %crate::errors::redact(&err.to_string()), event, "emit failed");
    }
}

/// Emit a user-facing toast (message is redacted before it reaches the webview).
pub fn toast<R: Runtime>(app: &AppHandle<R>, message: impl AsRef<str>) {
    let payload = crate::errors::redact(message.as_ref());
    emit(app, TOAST, payload);
}
