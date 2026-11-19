//! Local proxy and egress-chain services.

/// Custom proxy pool parsing and rotation.
pub mod chain;
/// Cloudflare edge selection and failure accounting.
pub mod edge;
/// The local HTTP/CONNECT proxy server.
pub mod server;
/// Minimal SOCKS5 client used for WARP isolated mode, Tor, and custom pools.
pub mod socks;

/// Parse a custom proxy pool string into typed specs.
pub use chain::{parse_proxy_specs, ProxySpec};
/// Proxy status snapshot, server startup, and running-server handle.
pub use server::{proxy_status_snapshot, start_proxy_server, RunningProxy};
