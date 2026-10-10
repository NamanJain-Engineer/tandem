//! `tandem_app` services. Each takes its dependencies explicitly so tests run
//! without a window (ARCHITECTURE §2.12).

pub mod accounts;
pub mod api;
pub mod approvals;
pub mod audit;
pub mod auth;
pub mod auto;
pub mod boot;
pub mod config;
pub mod dashboard;
pub mod data_controls;
pub mod deeplink;
pub mod device_login;
pub mod engines;
pub mod entitlement;
pub mod events;
pub mod files;
pub mod find_work;
pub mod gateway;
pub mod git_panel;
pub mod handoff;
pub mod hosted;
pub mod lanes;
pub mod launch;
pub mod learn;
pub mod limits;
pub mod mail;
pub mod models;
pub mod names;
pub mod notifications;
pub mod notifier;
pub mod opener;
pub mod plugins;
pub mod restore;
pub mod staging;
pub mod status;
pub mod teammate_draft;
pub mod teammate_tools;
pub mod teammates;
pub mod telemetry;
pub mod terminal;
pub mod thread_changes;
pub mod thread_files;
pub mod thread_summary;
pub mod thread_titles;
pub mod threads;
pub mod updates;
pub mod usage;
pub mod usage_report;
pub mod workspaces;

/// Debug builds: the E2E fake engines' switches (`TANDEM_FAKE_*`), for
/// engine runs that aren't panes and so don't read the test profile.
pub(crate) fn fake_engine_env() -> Vec<(String, String)> {
    if !cfg!(debug_assertions) {
        return Vec::new();
    }
    std::env::vars()
        .filter(|(k, _)| k.starts_with("TANDEM_FAKE_"))
        .collect()
}
