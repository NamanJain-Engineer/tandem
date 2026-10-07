//! Thin Tauri command wrappers. Each maps to a service and returns
//! `Result<T, IpcError>` (ARCHITECTURE §3.1).

pub mod account;
pub mod app;
pub mod engines;
pub mod handoff;
pub mod lanes;
pub mod launch;
pub mod onboarding;
pub mod restore;
pub mod settings;
pub mod shell;
pub mod terminal;
#[cfg(debug_assertions)]
pub mod test;
pub mod updates;
pub mod window;
pub mod workspaces;
