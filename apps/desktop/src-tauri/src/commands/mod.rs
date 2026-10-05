//! Thin Tauri command wrappers. Each maps to a service and returns
//! `Result<T, IpcError>` (ARCHITECTURE §3.1).

pub mod account;
pub mod app;
pub mod settings;
pub mod terminal;
#[cfg(debug_assertions)]
pub mod test;
pub mod window;
pub mod workspaces;
