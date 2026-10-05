//! Shared, dependency-light building blocks for every Tandem crate:
//! paths, errors, ids, settings schema, event bus, status store, redaction,
//! path containment, clock, keychain, command registry and logging.

pub mod atomic;
pub mod bus;
pub mod clock;
pub mod commands;
pub mod contain;
pub mod diagnostics;
pub mod entitlement;
pub mod error;
pub mod ids;
pub mod keychain;
pub mod logging;
pub mod net;
pub mod paths;
pub mod redact;
pub mod remote_config;
pub mod settings;
pub mod status;
pub mod supervisor;
pub mod telemetry;

pub use error::{CoreError, ErrorCode, IpcError};
pub use paths::DataDir;
