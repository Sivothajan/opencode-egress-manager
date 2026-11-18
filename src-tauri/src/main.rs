#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if let Some(_role) = args.first().filter(|a| *a == "--worker") {
        let role = args.get(1).cloned().unwrap_or_default();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to start async runtime");
        return ExitCode::from(
            runtime.block_on(egress_manager::services::worker::run_role(&role)) as u8,
        );
    }

    if args.first().map(|a| a.as_str()) == Some("--lane") {
        let rest: Vec<String> = args.iter().skip(1).cloned().collect();
        let desktop = rest.first().map(|a| a == "--ouc-desktop").unwrap_or(false);
        let launch_args = if desktop {
            rest.iter().skip(1).cloned().collect()
        } else {
            rest
        };
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to start async runtime");
        return ExitCode::from(runtime.block_on(egress_manager::services::lane::run_lane(
            desktop,
            launch_args,
        )) as u8);
    }

    egress_manager::app::run();
    ExitCode::SUCCESS
}
