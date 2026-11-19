//! Connection health analysis shared by the dashboard and diagnostics page.

use serde::Serialize;

/// One actionable problem surfaced by the health analysis.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HealthIssue {
    /// Severity bucket used for ordering and styling: `ok`, `warn`, or `bad`.
    pub level: String,
    /// Short, user-facing headline describing the problem.
    pub title: String,
    /// Longer explanation of what is wrong and what the user can do.
    pub detail: String,
    /// One-click fix action identifier, when a repair is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
    /// Label for the fix button; empty when there is no fix.
    pub fix_label: String,
}

/// Overall connection health shown on the dashboard.
#[derive(Debug, Clone, Serialize)]
pub struct HealthSummary {
    /// Aggregate status: `ok`, `warn`, `bad`, or `busy`.
    pub status: String,
    /// Headline describing the current state.
    pub title: String,
    /// Supporting sentence shown under the headline.
    pub sub: String,
    /// Problems in diagnostic-phase order: setup, route, worker, proxy, rotation.
    pub issues: Vec<HealthIssue>,
}

/// Live state snapshot fed into the health analysis.
#[derive(Debug, Clone, Default)]
pub struct HealthInput {
    /// Whether Cloudflare WARP is installed on this machine.
    pub warp_installed: bool,
    /// Whether WARP currently reports a connected tunnel.
    pub warp_connected: bool,
    /// Whether a route rotation is currently in progress.
    pub rotating: bool,
    /// Selected egress mode (`warp`, `clean`, `tor`, or `custom`).
    pub mode: String,
    /// Whether Tor is reachable as an egress route.
    pub egress_tor_active: bool,
    /// Whether the WARP Keeper background worker is running.
    pub keeper_running: bool,
    /// Whether the Zen Guard auto-rotation worker is running.
    pub guard_running: bool,
    /// Whether the local proxy is listening.
    pub proxy_listening: bool,
    /// Whether the background service is wired into OpenCode.
    pub svc_wired: bool,
    /// Whether OpenCode's PATH-level hook is wired up.
    pub path_wired: bool,
    /// OpenCode is already routed through the legacy Python app's proxy.
    pub external_svc_wired: bool,
    /// Whether the background service has been installed.
    pub installed: bool,
    /// Last real connectivity probe result, if one has completed.
    pub probe_ok: Option<bool>,
    /// Probe outcome code, e.g. `ok`, `upstream`, `timeout`, or `offline`.
    pub probe_code: String,
    /// Unix timestamp until which auto-rotation is backed off.
    pub guard_backoff: i64,
    /// Unix timestamp until which the free-limit pause is enforced.
    pub guard_free_wall: i64,
    /// Seconds since Keeper last checked in, if known.
    pub keeper_heartbeat: Option<f64>,
    /// Seconds since Zen Guard last checked in, if known.
    pub guard_heartbeat: Option<f64>,
    /// Whether the desktop app is installed.
    pub desktop_installed: bool,
    /// Whether the CLI is installed.
    pub cli_installed: bool,
    /// Number of live response streams cut in the last five minutes.
    pub stream_resets_5m: usize,
    /// Whether the proxy is currently on the direct fallback route.
    pub fallback_active: bool,
    /// Seconds since the fallback route was engaged, if known.
    pub fallback_age: Option<i64>,
    /// A recent network error sample recorded from the proxy.
    pub net_error_sample: Option<String>,
}

const HEARTBEAT_STALE_SECS: f64 = 240.0;

/// Turn live state into plain-language problems with one-click fixes.
pub fn build_health(input: &HealthInput) -> HealthSummary {
    let mut issues: Vec<HealthIssue> = Vec::new();
    let now = chrono::Utc::now().timestamp();
    check_setup(input, &mut issues);
    check_route(input, &mut issues);
    check_worker_liveness(input, &mut issues);
    check_proxy(input, &mut issues);
    check_rotation(input, &mut issues, now);
    summarize(input, &issues)
}

fn issue(level: &str, title: &str, detail: &str, fix: Option<&str>, label: &str) -> HealthIssue {
    HealthIssue {
        level: level.to_string(),
        title: title.to_string(),
        detail: detail.to_string(),
        fix: fix.map(|s| s.to_string()),
        fix_label: label.to_string(),
    }
}

/// Setup, wiring, background-worker, and heartbeat problems.
fn check_setup(input: &HealthInput, issues: &mut Vec<HealthIssue>) {
    if !input.installed {
        issues.push(issue(
            "warn",
            "Setup isn't finished",
            "The background service isn't installed yet, so OpenCode isn't protected or auto-rotated.",
            Some("install"),
            "Install",
        ));
    }
    if input.installed
        && (input.desktop_installed || input.cli_installed)
        && !input.svc_wired
        && !input.path_wired
        && !input.external_svc_wired
    {
        issues.push(issue(
            "warn",
            "OpenCode isn't using Opencode Egress Manager",
            "OpenCode is installed but connecting directly, so it will hit the free limit and nothing will rotate. Press Connect, then reopen OpenCode.",
            Some("connect_opencode"),
            "Connect",
        ));
    }
    if (input.svc_wired || input.path_wired) && !input.proxy_listening {
        issues.push(issue(
            "bad",
            "OpenCode can't connect right now",
            &format!(
                "OpenCode is set to go through the local proxy on port {}, but it isn't running. Messages fail with ECONNREFUSED until it's back.",
                crate::config::SERVICE_PROXY_PORT_DEFAULT
            ),
            Some("start_keeper"),
            "Start it",
        ));
    } else if input.installed && !input.keeper_running {
        issues.push(issue(
            "warn",
            "Background protection is off",
            "WARP Keeper isn't running. This window is covering for it, but OpenCode will lose its connection when you close the app.",
            Some("start_keeper"),
            "Turn on",
        ));
    }
    if let Some(age) = input.keeper_heartbeat {
        if age > HEARTBEAT_STALE_SECS {
            issues.push(issue(
                "bad",
                "Background protection stopped responding",
                &format!(
                    "WARP Keeper is still running but hasn't checked in for {} minutes, so OpenCode may lose its connection. Press Restart.",
                    (age / 60.0) as i64
                ),
                Some("restart_workers"),
                "Restart",
            ));
        }
    }
    if let Some(age) = input.guard_heartbeat {
        if age > HEARTBEAT_STALE_SECS {
            issues.push(issue(
                "warn",
                "Auto-rotation stopped responding",
                &format!(
                    "Zen Guard is still running but hasn't checked in for {} minutes, so the free limit won't rotate. Press Restart.",
                    (age / 60.0) as i64
                ),
                Some("restart_workers"),
                "Restart",
            ));
        }
    }
}

/// Availability of the selected egress route.
fn check_route(input: &HealthInput, issues: &mut Vec<HealthIssue>) {
    if input.mode == "warp" || input.mode == "clean" {
        if !input.warp_installed {
            issues.push(issue(
                "bad",
                "Cloudflare WARP isn't installed",
                "WARP mode needs the free Cloudflare WARP app (1.1.1.1).",
                Some("install_warp"),
                "Download WARP",
            ));
        } else if !input.warp_connected && !input.rotating {
            issues.push(issue(
                "bad",
                "WARP is disconnected",
                "Traffic can't go through Cloudflare right now.",
                Some("connect_warp"),
                "Connect",
            ));
        }
    } else if input.mode == "tor" && !input.egress_tor_active {
        issues.push(issue(
            "bad",
            "Tor isn't running",
            "Tor mode is selected but no Tor proxy is reachable.",
            Some("start_tor"),
            "Start Tor",
        ));
    }
}

/// Whether the background quota guard is running at all.
fn check_worker_liveness(input: &HealthInput, issues: &mut Vec<HealthIssue>) {
    if input.installed && !input.guard_running {
        issues.push(issue(
            "warn",
            "Auto-rotation is off",
            "Zen Guard isn't running, so when you hit the free limit you'll have to rotate manually.",
            Some("start_guard"),
            "Turn on",
        ));
    }
}

/// Proxy fallback, probe, stream-reset, and logged network-error problems.
fn check_proxy(input: &HealthInput, issues: &mut Vec<HealthIssue>) {
    if input.proxy_listening && input.fallback_active {
        let route = match input.mode.as_str() {
            "warp" => "Cloudflare WARP",
            "clean" => "the clean endpoint",
            "tor" => "Tor",
            "custom" => "your proxy",
            _ => "the selected route",
        };
        let when = match input.fallback_age {
            Some(age) if age >= 60 => format!(
                "{} minute{} ago",
                age / 60,
                if age / 60 != 1 { "s" } else { "" }
            ),
            _ => "in the last few minutes".to_string(),
        };
        let fix = if input.mode == "tor" {
            "start_tor"
        } else {
            "repair_warp"
        };
        issues.push(issue(
            "warn",
            "Using a direct connection",
            &format!(
                "The route through {route} dropped {when}, so OpenCode was sent straight out to stay usable. It keeps working, but the free limit may not rotate until this is repaired."
            ),
            Some(fix),
            "Repair route",
        ));
    }

    if input.proxy_listening
        && matches!(
            input.probe_code.as_str(),
            "upstream" | "timeout" | "offline"
        )
    {
        if input.probe_code == "offline" {
            issues.push(issue(
                "bad",
                "This PC has no internet",
                "Neither the direct connection nor the local proxy could reach the internet. Check Wi-Fi or the network cable, then press Refresh.",
                None,
                "",
            ));
        } else if (matches!(input.mode.as_str(), "warp" | "clean") && input.warp_connected)
            || (input.mode == "tor" && input.egress_tor_active)
            || input.mode == "custom"
        {
            let hint = match input.mode.as_str() {
                "tor" => "Press Start Tor, or pick another connection type.",
                "custom" => {
                    "Press Next Node to try another proxy node, or pick another connection type."
                }
                _ => "Press Fix WARP, or pick another connection type.",
            };
            let fix = if input.mode == "tor" {
                "start_tor"
            } else if input.mode == "custom" {
                "hop"
            } else {
                "repair_warp"
            };
            issues.push(issue(
                "bad",
                "OpenCode can't get through the local proxy",
                &format!(
                    "The local proxy is running, but nothing comes back from opencode.ai. {hint}"
                ),
                Some(fix),
                if input.mode == "tor" || input.mode == "custom" {
                    "Try"
                } else {
                    "Repair"
                },
            ));
        }
    }

    if input.proxy_listening && input.stream_resets_5m >= 2 && !input.fallback_active {
        let fix = if input.mode == "tor" {
            "start_tor"
        } else {
            "hop"
        };
        let label = if input.mode == "tor" {
            "Restart Tor"
        } else {
            "Rotate route"
        };
        issues.push(issue(
            "warn",
            "Response streams keep dropping",
            &format!(
                "{} live response stream{} were cut in the last five minutes. A flaky edge or provider idle timeout can do this; the route itself is still up.",
                input.stream_resets_5m,
                if input.stream_resets_5m != 1 { "s" } else { "" }
            ),
            Some(fix),
            label,
        ));
    }

    if let Some(sample) = &input.net_error_sample {
        if input.proxy_listening
            && !input.fallback_active
            && !matches!(
                input.probe_code.as_str(),
                "upstream" | "timeout" | "offline"
            )
        {
            let fix = if input.mode == "tor" {
                "start_tor"
            } else if input.mode == "custom" {
                "hop"
            } else {
                "repair_warp"
            };
            let label = if input.mode == "tor" {
                "Restart Tor"
            } else if input.mode == "custom" {
                "Next node"
            } else {
                "Repair"
            };
            issues.push(issue(
                "bad",
                "OpenCode hit a network error",
                &format!(
                    "OpenCode logged a connection failure in the last few minutes: {sample}. Press Repair to rebuild the route; the Proxy tab shows which edge dropped it."
                ),
                Some(fix),
                label,
            ));
        }
    }
}

/// Rotation pauses that the user should know about.
fn check_rotation(input: &HealthInput, issues: &mut Vec<HealthIssue>, now: i64) {
    if input.guard_free_wall > now {
        let reset = chrono::DateTime::from_timestamp(input.guard_free_wall, 0)
            .map(|d| d.format("%H:%M").to_string())
            .unwrap_or_else(|| "00:00".into());
        issues.push(issue(
            "warn",
            "The free limit is not following the IP",
            &format!(
                "Two confirmed new IPv6 addresses still hit the free limit, so the allowance is no longer tied to your address. Rotation is paused until it resets around {reset} (UTC). Use another provider, subscribe to OpenCode Go, or press Try now to force another address."
            ),
            Some("hop"),
            "Try now",
        ));
    } else if input.guard_backoff > now {
        let mins = ((input.guard_backoff - now) / 60).max(1);
        issues.push(issue(
            "warn",
            "Auto-rotation is taking a break",
            &format!(
                "New addresses kept hitting the free limit right away, so rotation paused for about {mins} more minute{} to avoid a loop. It tries again by itself after that, or press Try now to get a new IP immediately.",
                if mins != 1 { "s" } else { "" }
            ),
            Some("hop"),
            "Try now",
        ));
    }
}

/// Collapse the collected issues into the dashboard summary.
fn summarize(input: &HealthInput, issues: &[HealthIssue]) -> HealthSummary {
    let bad = issues.iter().any(|i| i.level == "bad");
    let wired = input.svc_wired || input.path_wired || input.external_svc_wired;
    let (status, title, sub) = if input.rotating {
        (
            "busy",
            "Rotating to a fresh IP...",
            "This takes about 10-15 seconds. OpenCode will continue automatically.".to_string(),
        )
    } else if bad {
        (
            "bad",
            "OpenCode needs attention",
            "Fix the problem below and OpenCode will work again.".to_string(),
        )
    } else if !issues.is_empty() {
        (
            "warn",
            "Working, with warnings",
            "OpenCode works, but something below should be turned on.".to_string(),
        )
    } else if wired && input.probe_ok != Some(true) {
        if input.probe_ok.is_none() {
            (
                "warn",
                "Checking the connection...",
                "Making sure traffic really gets through the local proxy. This takes a few seconds.".to_string(),
            )
        } else {
            (
                "warn",
                "Couldn't confirm the connection",
                "The last real test didn't finish. Press Refresh to try again.".to_string(),
            )
        }
    } else {
        (
            "ok",
            "Everything is working",
            match input.mode.as_str() {
                "tor" => "OpenCode is connected through the Tor network.".to_string(),
                "custom" => "OpenCode is connected through your custom proxy.".to_string(),
                _ => "OpenCode is connected through Cloudflare WARP and auto-rotates when you hit the limit.".to_string(),
            },
        )
    };
    HealthSummary {
        status: status.to_string(),
        title: title.to_string(),
        sub,
        issues: issues.to_vec(),
    }
}

/// Apply the one-click fix action names used by the UI.
pub fn fix_action_id(action: &str) -> bool {
    matches!(
        action,
        "install"
            | "connect_opencode"
            | "start_keeper"
            | "restart_workers"
            | "connect_warp"
            | "repair_warp"
            | "hop"
            | "fix_port"
            | "start_tor"
            | "start_guard"
            | "install_warp"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_issue_serializes_camel_case() {
        let issue = HealthIssue {
            level: "bad".into(),
            title: "Local proxy is down".into(),
            detail: "Port 41988 is not listening.".into(),
            fix: Some("start_keeper".into()),
            fix_label: "Start it".into(),
        };
        let json = serde_json::to_value(&issue).unwrap();
        assert_eq!(json["fixLabel"], "Start it");
        assert!(json.get("fix_label").is_none());
    }

    fn base() -> HealthInput {
        HealthInput {
            warp_installed: true,
            warp_connected: true,
            mode: "warp".into(),
            keeper_running: true,
            guard_running: true,
            proxy_listening: true,
            svc_wired: true,
            installed: true,
            cli_installed: true,
            probe_ok: Some(true),
            ..Default::default()
        }
    }

    #[test]
    fn healthy_state_has_no_issues() {
        let summary = build_health(&base());
        assert_eq!(summary.status, "ok");
        assert!(summary.issues.is_empty());
    }

    #[test]
    fn missing_proxy_is_bad() {
        let mut input = base();
        input.proxy_listening = false;
        let summary = build_health(&input);
        assert!(summary.issues.iter().any(|i| i.level == "bad"));
    }

    #[test]
    fn fallback_is_warned_and_actionable() {
        let mut input = base();
        input.fallback_active = true;
        input.fallback_age = Some(120);
        let summary = build_health(&input);
        let issue = summary
            .issues
            .iter()
            .find(|i| i.title == "Using a direct connection")
            .expect("fallback issue");
        assert_eq!(issue.fix.as_deref(), Some("repair_warp"));
        assert!(issue.detail.contains("2 minutes ago"));
    }

    #[test]
    fn free_wall_explains_the_reset() {
        let mut input = base();
        input.guard_free_wall = chrono::Utc::now().timestamp() + 3600;
        let summary = build_health(&input);
        assert!(summary
            .issues
            .iter()
            .any(|i| i.title == "The free limit is not following the IP"));
    }

    #[test]
    fn repeated_stream_resets_suggest_rotation() {
        let mut input = base();
        input.stream_resets_5m = 3;
        let summary = build_health(&input);
        assert!(summary.issues.iter().any(
            |i| i.title == "Response streams keep dropping" && i.fix.as_deref() == Some("hop")
        ));
    }

    #[test]
    fn known_fix_actions_are_allowed() {
        assert!(fix_action_id("start_keeper"));
        assert!(!fix_action_id("shutdown_pc"));
    }

    #[test]
    fn issues_keep_their_diagnostic_order() {
        let mut input = base();
        // Trigger one issue from each phase: setup, route, worker, proxy, rotation.
        input.keeper_running = false;
        input.warp_installed = false;
        input.guard_running = false;
        input.fallback_active = true;
        input.guard_backoff = chrono::Utc::now().timestamp() + 600;

        let summary = build_health(&input);
        let titles: Vec<&str> = summary.issues.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(
            titles,
            vec![
                "Background protection is off",
                "Cloudflare WARP isn't installed",
                "Auto-rotation is off",
                "Using a direct connection",
                "Auto-rotation is taking a break",
            ]
        );
    }

    #[test]
    fn rotating_status_beats_a_bad_issue() {
        let mut input = base();
        input.proxy_listening = false; // bad issue
        input.rotating = true;
        assert_eq!(build_health(&input).status, "busy");
    }

    #[test]
    fn bad_issues_outrank_warnings() {
        let mut input = base();
        input.keeper_running = false; // warning
        input.proxy_listening = false; // bad
        assert_eq!(build_health(&input).status, "bad");
    }

    #[test]
    fn pending_probe_is_reported_as_warn() {
        let mut input = base();
        input.probe_ok = None;
        assert_eq!(build_health(&input).status, "warn");
    }
}
