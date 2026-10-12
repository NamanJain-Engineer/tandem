//! `tandem://` links (F-SH-13). Only known routes with known, length-limited
//! parameters are accepted; anything else is "Unrecognized link" and does
//! nothing. Links that arrive before the UI is ready wait in a queue
//! (`deeplink_pending`).

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const MAX_LINK: usize = 4096;
pub const MAX_PARAM: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DeepLink {
    /// `auth/callback`, `open`, or `unrecognized`.
    pub route: String,
    pub params: BTreeMap<String, String>,
}

impl DeepLink {
    pub fn unrecognized() -> Self {
        Self {
            route: "unrecognized".into(),
            params: BTreeMap::new(),
        }
    }
}

/// Routes and the parameters each may carry (M2 set; thread/, teammate/,
/// dictate/toggle and plugin/callback arrive in M7, M10, M8 and M12).
const ROUTES: &[(&str, &[&str], &[&str])] = &[
    // (route, required, optional)
    ("auth/callback", &["code", "state"], &[]),
    ("open", &["path"], &[]),
    // Starts or stops a take (F-DI-01), e.g. bound to a Wayland desktop shortcut.
    ("dictate/toggle", &[], &[]),
    // A plugin sign-in that returns by the custom scheme (F-PL-03).
    (
        "plugin/callback",
        &["state"],
        &["code", "error", "error_description", "iss"],
    ),
];

fn scheme_ok(scheme: &str) -> bool {
    scheme == "tandem" || (cfg!(debug_assertions) && scheme == "tandem-dev")
}

/// Parses and validates a link. Never panics; bad input is `None`.
pub fn parse(raw: &str) -> Option<DeepLink> {
    if raw.len() > MAX_LINK || raw.chars().any(|c| c.is_control()) {
        return None;
    }
    let (scheme, rest) = raw.split_once("://")?;
    if !scheme_ok(&scheme.to_ascii_lowercase()) {
        return None;
    }
    let (path, query) = match rest.split_once('?') {
        Some((p, q)) => (p, q),
        None => (rest, ""),
    };
    let route = path.trim_end_matches('/');
    // Reject traversal and encoded tricks outright.
    if route.contains("..") || route.contains('%') || route.contains('\\') {
        return None;
    }
    let (_, required, optional) = ROUTES.iter().find(|(r, _, _)| *r == route)?;
    let mut params = BTreeMap::new();
    for (k, v) in url::form_urlencoded::parse(query.as_bytes()) {
        let (k, v) = (k.into_owned(), v.into_owned());
        if !(required.contains(&k.as_str()) || optional.contains(&k.as_str()))
            || v.len() > MAX_PARAM
            || params.contains_key(&k)
        {
            return None;
        }
        params.insert(k, v);
    }
    if !required
        .iter()
        .all(|r| params.get(*r).is_some_and(|v| !v.is_empty()))
    {
        return None;
    }
    if route == "open" && !valid_open_path(params.get("path")?) {
        return None;
    }
    Some(DeepLink {
        route: route.to_string(),
        params,
    })
}

fn valid_open_path(p: &str) -> bool {
    let absolute = std::path::Path::new(p).is_absolute() || p.starts_with('/');
    absolute && !p.split(['/', '\\']).any(|seg| seg == "..") && !p.contains('\0')
}

/// Finds a link among process arguments (first launch or forwarded by the
/// single-instance plugin).
pub fn from_args<I: IntoIterator<Item = String>>(args: I) -> Vec<String> {
    args.into_iter()
        .filter(|a| {
            a.contains("://")
                && scheme_ok(&a.split("://").next().unwrap_or("").to_ascii_lowercase())
        })
        .collect()
}

/// The schemes in tauri.conf.json's `plugins.deep-link.desktop.schemes`.
pub const CONFIGURED_SCHEMES: &[&str] = &["tandem"];

/// Links a second instance's arguments carry that the single-instance
/// callback must route itself (D-518). With its `deep-link` feature the
/// single-instance plugin already hands `bin <link>` to the deep-link plugin
/// when the link's scheme is configured, and that arrives through
/// `on_open_url`; routing it here too handled every such link twice (a
/// `tandem://dictate/toggle` started a take and stopped it at once).
pub fn second_instance_links(args: &[String]) -> Vec<String> {
    let forwarded = args.len() == 2
        && args[1]
            .parse::<url::Url>()
            .is_ok_and(|u| CONFIGURED_SCHEMES.contains(&u.scheme()));
    if forwarded {
        return Vec::new();
    }
    from_args(args.iter().skip(1).cloned())
}

/// Links received before the UI asked for them.
#[derive(Default)]
pub struct DeepLinkQueue {
    pending: Mutex<Vec<DeepLink>>,
    ui_ready: Mutex<bool>,
}

impl DeepLinkQueue {
    /// Queues `link` unless the UI is ready; returns true when it should be
    /// emitted right away instead.
    pub fn offer(&self, link: DeepLink) -> bool {
        if self.ui_ready.lock().map(|r| *r).unwrap_or(false) {
            return true;
        }
        if let Ok(mut p) = self.pending.lock() {
            p.push(link);
        }
        false
    }

    /// `deeplink_pending`: returns queued links and marks the UI ready.
    pub fn take(&self) -> Vec<DeepLink> {
        if let Ok(mut r) = self.ui_ready.lock() {
            *r = true;
        }
        self.pending
            .lock()
            .map(|mut p| std::mem::take(&mut *p))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn d_518_a_link_the_deep_link_plugin_forwards_is_routed_once() {
        // `tandem tandem://dictate/toggle`: on_open_url gets it, not the callback.
        assert!(
            second_instance_links(&args(&["/usr/bin/tandem", "tandem://dictate/toggle"]))
                .is_empty()
        );
        assert!(
            second_instance_links(&args(&["tandem", "tandem://auth/callback?code=x"])).is_empty()
        );
        // Anything the plugin skips still reaches the router from the callback.
        assert_eq!(
            second_instance_links(&args(&["tandem", "--flag", "tandem://dictate/toggle"])),
            args(&["tandem://dictate/toggle"])
        );
        assert!(second_instance_links(&args(&["tandem"])).is_empty());
        assert!(second_instance_links(&args(&["tandem", "https://example.com"])).is_empty());
        if cfg!(debug_assertions) {
            // Debug builds register tandem-dev at runtime, outside the config.
            assert_eq!(
                second_instance_links(&args(&["tandem", "tandem-dev://auth/callback"])),
                args(&["tandem-dev://auth/callback"])
            );
        }
    }

    #[test]
    fn d_518_configured_schemes_match_tauri_conf() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).expect("tauri.conf.json");
        let schemes: Vec<&str> = conf["plugins"]["deep-link"]["desktop"]["schemes"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        assert_eq!(schemes, CONFIGURED_SCHEMES);
    }

    use super::*;

    #[test]
    fn f_sh_13_1_open_path_is_recognized() {
        let link = parse("tandem://open?path=/tmp/x").expect("open");
        assert_eq!(link.route, "open");
        assert_eq!(link.params["path"], "/tmp/x");
        let link = parse("tandem://open?path=%2Ftmp%2Fwith%20space").expect("encoded value");
        assert_eq!(link.params["path"], "/tmp/with space");
        let cb = parse("tandem://auth/callback?code=abc&state=def").expect("callback");
        assert_eq!(cb.route, "auth/callback");
    }

    #[test]
    fn f_di_01_dictate_toggle_is_a_route_without_parameters() {
        let link = parse("tandem://dictate/toggle").expect("toggle");
        assert_eq!(link.route, "dictate/toggle");
        assert!(link.params.is_empty());
        assert!(parse("tandem://dictate/toggle?x=1").is_none());
    }

    #[test]
    fn f_sh_13_plugin_callback_carries_the_sign_in_result() {
        let link = parse("tandem://plugin/callback?code=c1&state=s1").expect("plugin callback");
        assert_eq!(link.route, "plugin/callback");
        assert_eq!(
            (link.params["code"].as_str(), link.params["state"].as_str()),
            ("c1", "s1")
        );
        let denied =
            parse("tandem://plugin/callback?state=s1&error=access_denied&error_description=No")
                .expect("denied");
        assert_eq!(denied.params["error"], "access_denied");
        // Without the attempt's state it's nothing to deliver.
        assert!(parse("tandem://plugin/callback?code=c1").is_none());
        assert!(parse("tandem://plugin/callback?code=c1&state=s1&token=x").is_none());
    }

    #[test]
    fn f_sh_13_2_traversal_long_and_unknown_links_are_rejected() {
        for bad in [
            "tandem://../../etc",
            "tandem://open/../../etc?path=/tmp",
            "tandem://open?path=../relative",
            "tandem://open?path=/tmp/../../etc",
            "tandem://open?path=relative/dir",
            "tandem://open",
            "tandem://open?path=/tmp&evil=1",
            "tandem://open?path=/a&path=/b",
            "tandem://settings",
            "tandem://auth/callback?code=abc",
            "tandem://%2e%2e/etc",
            "https://open?path=/tmp",
            "tandem:open?path=/tmp",
            "tandem://open?path=/tmp\n",
        ] {
            assert!(parse(bad).is_none(), "{bad} must be rejected");
        }
        let long = format!("tandem://open?path=/{}", "a".repeat(5 * 1024));
        assert!(parse(&long).is_none(), "links over 4 KB are rejected");
        let long_param = format!("tandem://open?path=/{}", "a".repeat(MAX_PARAM + 1));
        assert!(parse(&long_param).is_none());
    }

    #[test]
    fn queue_holds_links_until_the_ui_is_ready() {
        let q = DeepLinkQueue::default();
        let link = parse("tandem://open?path=/tmp/x").expect("link");
        assert!(!q.offer(link.clone()));
        assert_eq!(q.take(), vec![link.clone()]);
        assert!(q.offer(link), "after the UI is ready links go straight out");
        assert!(q.take().is_empty());
    }

    #[test]
    fn args_are_filtered_to_links() {
        let args = vec![
            "tandem".to_string(),
            "--flag".into(),
            "tandem://open?path=/x".into(),
            "http://x".into(),
        ];
        assert_eq!(from_args(args), vec!["tandem://open?path=/x".to_string()]);
    }
}
