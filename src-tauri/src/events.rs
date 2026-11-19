//! Tauri event names and emit helpers.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

/// Emitted whenever the assembled app state changes.
pub const STATE_CHANGED: &str = "state-changed";
/// Emitted when the connectivity probe result changes.
pub const PROBE_CHANGED: &str = "probe-changed";
/// Emitted for each notable worker/log line.
pub const LOG_LINE: &str = "log-line";
/// Emitted to show a user-facing toast.
pub const TOAST: &str = "toast";
/// Emitted when a background worker starts, stops, or changes health.
pub const WORKER_STATE: &str = "worker-state";

/// Emit a serializable payload to the webview, logging (not surfacing) failures.
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
