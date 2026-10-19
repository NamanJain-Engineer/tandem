//! Thin Tauri command wrappers. Each maps to a service and returns
//! `Result<T, IpcError>` (ARCHITECTURE §3.1).

pub mod account;
pub mod accounts;
pub mod app;
pub mod auto;
pub mod bestof;
pub mod browser;
pub mod checkpoints;
pub mod conductor;
pub mod conductor_persona;
pub mod conductor_tools;
pub mod data_controls;
pub mod dictate;
pub mod engine_versions;
pub mod engines;
pub mod files;
pub mod find_work;
pub mod git;
pub mod handoff;
pub mod instructions;
pub mod lanes;
pub mod launch;
pub mod library;
pub mod limits;
pub mod onboarding;
pub mod phone;
pub mod plugins;
pub mod ports;
pub mod prompts;
pub mod recall;
pub mod remote;
pub mod restore;
pub mod review;
pub mod routines;
pub mod runaway;
pub mod scratchpad;
pub mod settings;
pub mod shell;
#[cfg(any(target_os = "macos", test))]
pub mod simulator;
pub mod task_exec;
pub mod tasks;
pub mod teammates;
pub mod terminal;
#[cfg(debug_assertions)]
pub mod test;
pub mod threads;
pub mod updates;
pub mod usage;
pub mod window;
pub mod workspaces;
