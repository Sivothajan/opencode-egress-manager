//! Privileged helper binary.
//!
//! This executable performs exactly three operations and nothing else:
//!   egress-manager-admin firewall-status
//!   egress-manager-admin firewall-install <canonical-opencode-cli-path>
//!   egress-manager-admin firewall-remove
//!
//! It is launched by the UI through `ShellExecuteW("runas")`, so UAC is scoped to a
//! single firewall operation. The main application is never elevated.

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!(
            "usage: egress-manager-admin <firewall-status|firewall-install|firewall-remove> [path]"
        );
        return ExitCode::from(2);
    }
    match args[0].as_str() {
        "firewall-status" => {
            let installed = egress_manager::services::firewall::is_installed().await;
            println!("{{\"installed\":{installed}}}");
            ExitCode::SUCCESS
        }
        "firewall-install" => {
            let Some(path) = args.get(1) else {
                eprintln!("missing CLI path");
                return ExitCode::from(2);
            };
            match egress_manager::services::firewall::install(path).await {
                Ok(message) => {
                    println!("{{\"ok\":true,\"message\":{}}}", json_string(&message));
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    println!(
                        "{{\"ok\":false,\"message\":{}}}",
                        json_string(&err.user_message())
                    );
                    ExitCode::FAILURE
                }
            }
        }
        "firewall-remove" => match egress_manager::services::firewall::remove().await {
            Ok(message) => {
                println!("{{\"ok\":true,\"message\":{}}}", json_string(&message));
                ExitCode::SUCCESS
            }
            Err(err) => {
                println!(
                    "{{\"ok\":false,\"message\":{}}}",
                    json_string(&err.user_message())
                );
                ExitCode::FAILURE
            }
        },
        other => {
            eprintln!("unknown operation: {other}");
            ExitCode::from(2)
        }
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
}
