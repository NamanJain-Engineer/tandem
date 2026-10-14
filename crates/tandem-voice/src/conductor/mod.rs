//! Conductor, the voice assistant (F-CN-01…04, F-CN-08; ARCHITECTURE
//! §12.6): the call's lifecycle ([`call::CallController`]), the local tool
//! executor ([`exec::Executor`]) with its closed schema and confirmation
//! rules, and what the HUD shows ([`view`]).

pub mod call;
pub mod confirm;
pub mod exec;
pub mod memory;
pub mod persona;
pub mod schema;
pub mod speaker;
pub mod view;
