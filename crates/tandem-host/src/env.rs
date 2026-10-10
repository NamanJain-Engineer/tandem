//! What a remote process's environment may hold (ARCHITECTURE §13.1
//! "Isolation", F-RM-01 AC4, F-AC-01.3). The desktop filters what it sends
//! with [`allowed`] and the host filters again on arrival, so nothing local
//! (provider homes, plugin or gateway credentials, the app's own variables)
//! reaches a host whichever side is out of date.

use std::collections::BTreeMap;

/// Variables the desktop may send. Everything else is dropped.
pub const ALLOWED: &[&str] = &[
    "TERM",
    "COLORTERM",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TANDEM_PANE_ID",
    "TANDEM_STATUS_SECRET",
];

/// Variables the host keeps from its own session (sshd's environment) for
/// the processes it starts.
pub const FROM_HOST: &[&str] = &[
    "HOME", "USER", "LOGNAME", "PATH", "SHELL", "TMPDIR", "LANG", "LC_ALL", "MAIL",
];

/// Values never sent, whatever their name: anything naming a provider
/// home or Tandem's local data. Defence in depth for [`ALLOWED`].
fn local_value(v: &str) -> bool {
    v.contains(".tandem/providers")
        || v.contains(".tandem\\providers")
        || v.contains("/.tandem/")
        || v.contains("\\.tandem\\")
}

pub fn is_allowed(key: &str) -> bool {
    ALLOWED.contains(&key)
}

/// The part of `env` that may cross to a host.
pub fn allowed<'a>(
    env: impl IntoIterator<Item = (&'a String, &'a String)>,
) -> BTreeMap<String, String> {
    env.into_iter()
        .filter(|(k, v)| is_allowed(k) && !local_value(v))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// The environment of a process the host starts: its own session's basics,
/// then what the client sent (filtered again), then the host's status
/// variables.
pub fn compose(
    host: impl IntoIterator<Item = (String, String)>,
    sent: &BTreeMap<String, String>,
    status_url: Option<&str>,
) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> = host
        .into_iter()
        .filter(|(k, _)| FROM_HOST.contains(&k.as_str()))
        .collect();
    env.extend(allowed(sent));
    if let (Some(url), true) = (status_url, env.contains_key("TANDEM_PANE_ID")) {
        env.insert("TANDEM_STATUS_URL".into(), url.into());
    }
    env.entry("TERM".into())
        .or_insert_with(|| "xterm-256color".into());
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_launch_env() -> BTreeMap<String, String> {
        [
            ("TERM", "xterm-256color"),
            ("COLORTERM", "truecolor"),
            ("TANDEM_PANE_ID", "pn_1"),
            (
                "CLAUDE_CONFIG_DIR",
                "/home/a/.tandem/providers/claude/acct_1",
            ),
            ("CODEX_HOME", "/home/a/.tandem/providers/codex/acct_2"),
            ("HOME", "/home/a"),
            ("PATH", "/home/a/.tandem/bin:/usr/bin"),
            ("TANDEM_SHELL_TOKEN", "abc"),
            (
                "TANDEM_STATUS_URL",
                "unix:///home/a/.tandem/run/status.sock",
            ),
            ("SSH_AUTH_SOCK", "/tmp/agent.sock"),
            ("LANG", "en_US.UTF-8"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn f_rm_01_4_no_provider_home_or_local_path_crosses() {
        let sent = allowed(&local_launch_env());
        assert_eq!(
            sent.keys().map(String::as_str).collect::<Vec<_>>(),
            ["COLORTERM", "LANG", "TANDEM_PANE_ID", "TERM"]
        );
        assert!(sent.values().all(|v| !v.contains(".tandem")));
        // A name on the list still can't carry a local data path.
        let sneaky: BTreeMap<String, String> = [(
            "LANG".to_string(),
            "/x/.tandem/providers/claude/a".to_string(),
        )]
        .into();
        assert!(allowed(&sneaky).is_empty());
    }

    #[test]
    fn f_ac_01_3_remote_spawn_env_is_the_hosts_plus_the_allowlist() {
        let host = [
            ("HOME", "/home/tandem"),
            ("PATH", "/usr/bin:/bin"),
            ("SSH_CONNECTION", "1.2.3.4 5 6.7.8.9 22"),
            ("SHELL", "/bin/bash"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()));
        let env = compose(
            host,
            &local_launch_env(),
            Some("unix:///home/tandem/.tandem-host/run/status.sock"),
        );
        assert_eq!(env.get("HOME").map(String::as_str), Some("/home/tandem"));
        assert_eq!(env.get("PATH").map(String::as_str), Some("/usr/bin:/bin"));
        assert!(!env.contains_key("SSH_CONNECTION"));
        assert!(!env.contains_key("CLAUDE_CONFIG_DIR"));
        assert!(!env.contains_key("CODEX_HOME"));
        assert!(!env.contains_key("TANDEM_SHELL_TOKEN"));
        assert_eq!(
            env.get("TANDEM_STATUS_URL").map(String::as_str),
            Some("unix:///home/tandem/.tandem-host/run/status.sock")
        );
        assert!(env.values().all(|v| !v.contains(".tandem/providers")));
    }

    /// Plugin credentials reach engines only through the gateway's lease
    /// (`TANDEM_GATEWAY_*`, MCP config). None of it may cross: M12 adds the
    /// plugin env names it injects to this list.
    #[test]
    fn f_rm_01_plugin_credentials_never_cross() {
        let plugin_env = [
            "TANDEM_GATEWAY_URL",
            "TANDEM_GATEWAY_TOKEN",
            "TANDEM_MCP_CONFIG",
            "GITHUB_TOKEN",
            "LINEAR_API_KEY",
        ];
        for name in plugin_env {
            assert!(!is_allowed(name), "{name} must never reach a host");
        }
        assert!(ALLOWED
            .iter()
            .all(|k| !k.contains("TOKEN") && !k.contains("KEY")));
    }
}
