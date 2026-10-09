//! Thin Tauri command wrappers. Each maps to a service and returns
//! `Result<T, IpcError>` (ARCHITECTURE §3.1).

pub mod account;
pub mod accounts;
pub mod app;
pub mod browser;
pub mod engines;
pub mod files;
pub mod git;
pub mod handoff;
pub mod lanes;
pub mod launch;
pub mod limits;
pub mod onboarding;
pub mod plugins;
pub mod restore;
pub mod settings;
pub mod shell;
pub mod teammates;
pub mod terminal;
#[cfg(debug_assertions)]
pub mod test;
pub mod threads;
pub mod updates;
pub mod window;
pub mod workspaces;
