//! Per-launch lane: ephemeral local proxy + environment for one OpenCode process.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{
    config::{EgressConfig, Paths},
    services::{networking, opencode, proxy, warp},
};

const INFO_FLAGS: &[&str] = &["--help", "-h", "--version", "-v", "--completions"];
const NO_TUNNEL_COMMANDS: &[&str] = &["service", "upgrade", "update", "uninstall", "debug"];

fn needs_tunnel(args: &[String]) -> bool {
    if args
        .iter()
        .any(|a| INFO_FLAGS.contains(&a.as_str()) || a.starts_with("--completions="))
    {
        return false;
    }
    for arg in args {
        if !arg.starts_with('-') {
            return !NO_TUNNEL_COMMANDS.contains(&arg.as_str());
        }
    }
    true
}

fn should_add_standalone(args: &[String]) -> bool {
    if args
        .iter()
        .any(|a| a == "--standalone" || a == "--server" || a.starts_with("--server="))
    {
        return false;
    }
    needs_tunnel(args)
}

async fn ensure_tunnel_up(paths: &Paths, strict: bool) -> bool {
    let cfg = EgressConfig::load(paths).unwrap_or_default();
    if matches!(
        cfg.mode,
        crate::config::EgressMode::Tor | crate::config::EgressMode::Custom
    ) {
        return true;
    }
    if cfg.warp_scope == crate::config::WarpScope::Isolated {
        if networking::port_open("127.0.0.1", crate::config::WARP_ISOLATED_SOCKS_PORT, 600).await {
            return true;
        }
    } else if networking::warp_local_v6().is_some() {
        return true;
    }
    if !strict {
        // The proxy has a direct fallback, so a missing tunnel is not fatal.
        return true;
    }
    let Some(bin) = warp::find_warp_cli() else {
        return true;
    };
    if !warp::registration_healthy(&bin).await {
        let _ = warp::repair_registration(&bin, 2).await;
    }
    let _ = warp::ensure_warp_protocol(&bin, "wireguard").await;
    let _ = warp::ensure_warp_mode(&bin, cfg.warp_scope).await;
    let _ = warp::run(&bin, &["connect"], 15).await;
    for _ in 0..10 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if cfg.warp_scope == crate::config::WarpScope::Isolated {
            if networking::port_open("127.0.0.1", crate::config::WARP_ISOLATED_SOCKS_PORT, 600)
                .await
            {
                return true;
            }
        } else if networking::warp_local_v6().is_some() {
            return true;
        }
    }
    false
}

/// Run one OpenCode process through an ephemeral proxy lane.
pub async fn run_lane(desktop: bool, args: Vec<String>) -> i32 {
    let paths = Paths::discover();
    if let Err(err) = paths.ensure_dirs() {
        eprintln!("[lane] cannot create state directories: {err}");
        return 1;
    }
    let target: PathBuf = if desktop {
        match opencode::find_opencode_desktop() {
            Some(path) => path,
            None => {
                eprintln!("OpenCode Desktop executable not found.");
                return 1;
            }
        }
    } else {
        match opencode::find_opencode_cli() {
            Some(path) => path,
            None => {
                eprintln!(
                    "OpenCode CLI binary not found - set OUC_REAL_BIN or reinstall OpenCode."
                );
                return 1;
            }
        }
    };

    if !needs_tunnel(&args) && !desktop {
        return run_target(&target, &args, None).await;
    }

    let strict = std::env::var("OUC_STRICT_PROXY").ok().as_deref() == Some("1")
        || paths.strict_flag().is_file();
    if !ensure_tunnel_up(&paths, strict).await {
        eprintln!(
            "[lane] Network tunnel is not accessible (fail-closed strict mode is active).\n\
             Disable Strict Mode or run the Diagnostics page."
        );
        return 1;
    }

    let proxy_server = match proxy::server::start_proxy_server(&paths, 0).await {
        Ok(server) => server,
        Err(err) => {
            eprintln!(
                "[lane] could not start the local proxy: {}",
                err.user_message()
            );
            return 1;
        }
    };
    let port = proxy_server.port;
    let proxy_url = format!("http://127.0.0.1:{port}");
    let mut launch_args = args.clone();
    if desktop {
        launch_args.insert(0, format!("--proxy-server={proxy_url}"));
    } else if should_add_standalone(&launch_args) {
        launch_args.insert(0, "--standalone".to_string());
    }

    let code = run_target(&target, &launch_args, Some(&proxy_url)).await;
    proxy_server.stop().await;
    code
}

async fn run_target(target: &Path, args: &[String], proxy_url: Option<&str>) -> i32 {
    let lower = target.to_string_lossy().to_ascii_lowercase();
    let is_cmd = lower.ends_with(".cmd") || lower.ends_with(".bat");
    let mut cmd = tokio::process::Command::new(if is_cmd {
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into())
    } else {
        target.to_string_lossy().to_string()
    });
    if is_cmd {
        cmd.arg("/c");
    }
    if is_cmd {
        cmd.arg(target);
    }
    cmd.args(args);
    if let Some(url) = proxy_url {
        cmd.env("HTTPS_PROXY", url)
            .env("HTTP_PROXY", url)
            .env("https_proxy", url)
            .env("http_proxy", url)
            .env("NO_PROXY", "127.0.0.1,localhost,::1")
            .env("no_proxy", "127.0.0.1,localhost,::1");
    }
    match cmd.status().await {
        Ok(status) => status.code().unwrap_or(1),
        Err(err) => {
            eprintln!("[lane] failed to launch OpenCode: {err}");
            1
        }
    }
}
