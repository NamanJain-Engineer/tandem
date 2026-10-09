//! `tandem-tasks`: the local task tracker and its stdio MCP server
//! (ARCHITECTURE §2.8, §9; F-TK-01, F-TK-02).

pub mod error;
pub mod lock;
pub mod model;
pub mod store;

pub use error::{TaskError, TaskErrorCode};
pub use model::*;
pub use store::TaskStore;
