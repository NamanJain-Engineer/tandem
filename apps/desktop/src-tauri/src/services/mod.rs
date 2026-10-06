//! `tandem_app` services. Each takes its dependencies explicitly so tests run
//! without a window (ARCHITECTURE §2.12).

pub mod api;
pub mod auth;
pub mod boot;
pub mod config;
pub mod deeplink;
pub mod engines;
pub mod entitlement;
pub mod events;
pub mod opener;
pub mod staging;
pub mod status;
pub mod telemetry;
pub mod terminal;
pub mod workspaces;
