//! Path quoting lives in `tandem-core::shellquote` (shared with the desktop
//! commands); re-exported here for PTY callers.

pub use tandem_core::shellquote::{quote_path, quote_paths, ShellDialect, UnsafeFilename};
