//! `tandem_app` services. Each takes its dependencies explicitly so tests run
//! without a window (ARCHITECTURE §2.12).

pub mod api;
pub mod approvals;
pub mod auth;
pub mod boot;
pub mod config;
pub mod dashboard;
pub mod deeplink;
pub mod engines;
pub mod entitlement;
pub mod events;
pub mod names;
pub mod opener;
pub mod staging;
pub mod status;
pub mod telemetry;
pub mod terminal;
pub mod workspaces;
