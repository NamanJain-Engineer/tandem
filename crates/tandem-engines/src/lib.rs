//! Engine layer (ARCHITECTURE §2.5, §6): registry, detection, provider homes,
//! launch specs, the status server and the chat adapters. Built in M3–M6.
//!
//! M4: the full §6.1 registry, detection, provider homes, terminal launch
//! specs, the status server and `tandem-hook`. M7: the normalized thread
//! events and the chat adapters.

pub mod accounts;
pub mod briefing;
pub mod chat;
pub mod detect;
pub mod events;
pub mod forecast;
pub mod guard;
pub mod handoff;
pub mod homes;
pub mod hook;
pub mod launch;
pub mod learn;
pub mod registry;
pub mod status_server;
pub mod transcript;
pub mod usage;
pub mod usage_log;

pub use detect::{detect, DetectEnv, DetectionReport, FoundEngine, FoundSource};
pub use registry::{EngineRegistry, EngineSpec};
