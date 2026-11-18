//! Opencode Egress Manager core library.
//!
//! The GUI binary (`main.rs`), background worker binary and privileged admin helper
//! all build on this library so business logic lives in exactly one place.

pub mod app;
pub mod commands;
pub mod config;
pub mod errors;
pub mod events;
pub mod platform;
pub mod services;

pub use errors::{AppError, Result};
