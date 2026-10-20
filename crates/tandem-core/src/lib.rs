//! Shared, dependency-light building blocks for every Tandem crate:
//! paths, errors, ids, settings schema, event bus, status store, redaction,
//! path containment, files, clock, keychain, command registry and logging.

pub mod access;
pub mod accounts;
pub mod allow_rules;
pub mod approvals;
pub mod atomic;
pub mod bus;
pub mod clock;
pub mod commands;
pub mod conductor_persona;
pub mod contain;
pub mod diagnostics;
pub mod entitlement;
pub mod error;
pub mod files;
pub mod find_work;
pub mod ids;
pub mod install;
pub mod keychain;
pub mod lanes;
pub mod limits;
pub mod logging;
pub mod net;
pub mod origin;
pub mod paths;
pub mod policy;
pub mod prompts;
pub mod redact;
pub mod remote;
pub mod remote_config;
pub mod sandbox;
pub mod secrets;
pub mod settings;
pub mod shellquote;
pub mod source;
pub mod status;
pub mod status_heuristics;
pub mod status_report;
pub mod supervisor;
pub mod teammates;
pub mod telemetry;
pub mod untrusted;
pub mod voice_tools;
pub mod wslpath;

pub use error::{CoreError, ErrorCode, IpcError};
pub use paths::DataDir;
