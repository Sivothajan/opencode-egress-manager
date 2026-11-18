//! Local proxy and egress-chain services.

pub mod chain;
pub mod edge;
pub mod server;
pub mod socks;

pub use chain::{parse_proxy_specs, ProxySpec};
pub use server::{proxy_status_snapshot, start_proxy_server, RunningProxy};
