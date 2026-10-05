//! Engine layer (ARCHITECTURE §2.5, §6): registry, detection, provider homes,
//! launch specs, the status server and the chat adapters. Built in M3–M6.
//!
//! M3 slice: the bundled registry with the fields terminal launches need
//! (id, label, commands) so agent panes can run and hand back to the shell.
//! M4 adds the rest of §6.1's schema, detection and provider homes.

pub mod registry;

pub use registry::{EngineRegistry, EngineSpec};
