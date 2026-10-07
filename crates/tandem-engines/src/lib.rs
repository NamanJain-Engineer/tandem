//! Engine layer (ARCHITECTURE §2.5, §6): registry, detection, provider homes,
//! launch specs, the status server and the chat adapters. Built in M3–M6.
//!
//! M4: the full §6.1 registry, detection, provider homes, terminal launch
//! specs, the status server and `tandem-hook`. Chat adapters arrive in M6.

pub mod detect;
pub mod homes;
pub mod hook;
pub mod launch;
pub mod registry;
pub mod status_server;

pub use detect::{detect, DetectEnv, DetectionReport, FoundEngine, FoundSource};
pub use registry::{EngineRegistry, EngineSpec};
