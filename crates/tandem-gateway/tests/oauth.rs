//! F-PL-03 against the mock authorization server (`support/mock_as.rs`):
//! discovery, registration, PKCE, the loopback callback, the exchange,
//! verification and the keychain, plus the failures the spec names.

mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use support::mock_as::MockAs;
use tandem_core::ids::ConnectionId;
use tandem_core::keychain::{Keychain, MemoryKeychain};
use tandem_gateway::connect::{self, OAuthRequest, Progress, StepState};
use tandem_gateway::credentials::{self, Credential, CredentialStore};
use tandem_gateway::oauth::{Loopback, Step};
use tandem_gateway::vendors::{catalog, VendorModule};
use tokio_util::sync::CancellationToken;

fn github(base: &str) -> Arc<dyn VendorModule> {
    catalog::modules(Some(base))
        .into_iter()
        .find(|m| m.manifest().id == "github")
        .expect("github")
}

/// The "browser": follows the authorize redirect to the loopback callback.
fn browser() -> connect::OpenFn {
    Arc::new(|url: &url::Url| {
        let url = url.clone();
        tokio::spawn(async move {
            let http = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("client");
            let Ok(resp) = http.get(url).send().await else {
                return;
            };
            if let Some(loc) = resp.headers().get("location").and_then(|v| v.to_str().ok()) {
                let _ = http.get(loc).send().await;
            }
        });
        Ok(())
    })
}

struct Rig {
    mock: MockAs,
    keys: MemoryKeychain,
    store: CredentialStore,
    loopback: Loopback,
    steps: Arc<Mutex<Vec<Progress>>>,
}

async fn rig() -> Rig {
    let mock = MockAs::start(0).await.expect("mock");
    let keys = MemoryKeychain::new();
    Rig {
        mock,
        store: CredentialStore::new(Arc::new(keys.clone())),
        keys,
        loopback: Loopback::start().await.expect("loopback"),
        steps: Arc::default(),
    }
}

impl Rig {
    fn request(&self, conn: &ConnectionId, cancel: CancellationToken) -> OAuthRequest {
        let steps = self.steps.clone();
        OAuthRequest {
            module: github(&self.mock.uri()),
            connection: conn.clone(),
            static_client_id: None,
            open: browser(),
            progress: Arc::new(move |p| steps.lock().expect("steps").push(p)),
            cancel,
            timeout: Duration::from_secs(10),
        }
    }

    fn stored(&self, conn: &ConnectionId) -> Option<String> {
        self.keys
            .get(
                &credentials::service(),
                &credentials::account("github", conn),
            )
            .expect("keychain")
            .map(|b| String::from_utf8_lossy(&b).to_string())
    }
}

#[tokio::test]
async fn f_pl_03_1_oauth_with_dcr_stores_the_tokens_in_the_keychain() {
    let r = rig().await;
    let conn = ConnectionId::new();
    let done = connect::oauth(
        &r.store,
        &r.loopback,
        r.request(&conn, CancellationToken::new()),
    )
    .await
    .expect("connected");
    assert_eq!(done.account.label.as_deref(), Some("octo-shop"));
    let stored = r.stored(&conn).expect("in the keychain");
    let issued = r.mock.access_tokens();
    assert_eq!(issued.len(), 1);
    assert!(stored.contains(&issued[0]));
    // Meta keeps no secret.
    assert!(!done.meta.to_string().contains(&issued[0]));
    assert!(done.meta["oauth"]["client_id"]
        .as_str()
        .is_some_and(|c| c.starts_with("client_")));
    let steps: Vec<(Step, StepState)> = r
        .steps
        .lock()
        .expect("steps")
        .iter()
        .map(|p| (p.step, p.state))
        .collect();
    for s in [
        Step::Discovery,
        Step::Registration,
        Step::SignIn,
        Step::TokenExchange,
        Step::Verification,
    ] {
        assert!(
            steps.contains(&(s, StepState::Done)),
            "{s:?} done: {steps:?}"
        );
    }
    // The sign-in step carried the browser link for "Copy link".
    assert!(r.steps.lock().expect("steps").iter().any(|p| p
        .authorize_url
        .as_deref()
        .is_some_and(|u| u.contains("code_challenge_method=S256"))));
}

#[tokio::test]
async fn f_pl_03_confidential_client_when_the_server_refuses_public_ones() {
    let r = rig().await;
    r.mock.switches(|s| s.reject_public = true);
    let conn = ConnectionId::new();
    connect::oauth(
        &r.store,
        &r.loopback,
        r.request(&conn, CancellationToken::new()),
    )
    .await
    .expect("connected");
    let cred = r.store.load("github", &conn).expect("load").expect("cred");
    assert!(
        cred.client_secret()
            .is_some_and(|s| s.starts_with("secret_")),
        "the client secret lives in the keychain"
    );
}

#[tokio::test]
async fn f_pl_03_2_a_redirect_to_169_254_169_254_is_rejected() {
    let r = rig().await;
    r.mock.switches(|s| s.private_redirect = true);
    let conn = ConnectionId::new();
    let err = connect::oauth(
        &r.store,
        &r.loopback,
        r.request(&conn, CancellationToken::new()),
    )
    .await
    .expect_err("rejected");
    assert_eq!(err.step, Step::Discovery);
    assert!(
        err.message.contains("169.254.169.254") && err.message.contains("private address"),
        "{}",
        err.message
    );
    assert!(r.stored(&conn).is_none());
    // Nothing was sent to the metadata address (it isn't reachable here, but
    // the flow stopped before trying).
    assert!(r
        .steps
        .lock()
        .expect("steps")
        .iter()
        .any(|p| p.state == StepState::Failed
            && p.error.as_deref().is_some_and(|e| e.contains("private"))));
}

#[tokio::test]
async fn f_pl_03_3_a_failed_verification_on_reconnect_leaves_the_old_credential() {
    let r = rig().await;
    let conn = ConnectionId::new();
    connect::oauth(
        &r.store,
        &r.loopback,
        r.request(&conn, CancellationToken::new()),
    )
    .await
    .expect("first connect");
    let before = r.stored(&conn).expect("stored");
    r.mock.switches(|s| s.upstream_rejects = true);
    let err = connect::oauth(
        &r.store,
        &r.loopback,
        r.request(&conn, CancellationToken::new()),
    )
    .await
    .expect_err("verification fails");
    assert_eq!(err.step, Step::Verification);
    assert_eq!(
        r.stored(&conn),
        Some(before),
        "the old credential is untouched"
    );
}

#[tokio::test]
async fn f_pl_03_4_a_cancelled_flows_late_callback_is_ignored() {
    let r = rig().await;
    r.mock.switches(|s| s.authorize_delay_ms = 800);
    let conn = ConnectionId::new();
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        stop.cancel();
    });
    let err = connect::oauth(&r.store, &r.loopback, r.request(&conn, cancel))
        .await
        .expect_err("cancelled");
    assert_eq!(err.message, "Cancelled");
    // The browser comes back after the cancel…
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(
        r.loopback.ignored(),
        1,
        "the late callback reached the app and was ignored"
    );
    assert!(r.stored(&conn).is_none(), "nothing was saved");
    assert!(
        r.mock.access_tokens().is_empty(),
        "no code was ever exchanged"
    );
}

#[tokio::test]
async fn f_pl_03_failures_name_the_step_and_keep_the_providers_words() {
    let r = rig().await;
    let words = "The code was already used. Ask the user to start again; this message is deliberately long to prove nothing truncates it.";
    r.mock.switches(|s| s.token_error = Some(words.into()));
    let err = connect::oauth(
        &r.store,
        &r.loopback,
        r.request(&ConnectionId::new(), CancellationToken::new()),
    )
    .await
    .expect_err("exchange fails");
    assert_eq!(err.step, Step::TokenExchange);
    assert_eq!(err.message, format!("invalid_grant: {words}"));
}

#[tokio::test]
async fn f_pl_03_api_keys_are_verified_before_they_are_saved() {
    let r = rig().await;
    // Mint a token the mock upstream accepts, and use it as a "key".
    let conn = ConnectionId::new();
    connect::oauth(
        &r.store,
        &r.loopback,
        r.request(&conn, CancellationToken::new()),
    )
    .await
    .expect("connect");
    let good = r.mock.access_tokens()[0].clone();
    let module = github(&r.mock.uri());
    let other = ConnectionId::new();
    let err = connect::api_key(&r.store, module.as_ref(), &other, "ghp_wrong")
        .await
        .expect_err("bad key");
    assert_eq!(err.step, Step::Verification);
    assert!(r.store.load("github", &other).expect("load").is_none());
    let ok = connect::api_key(&r.store, module.as_ref(), &other, &good)
        .await
        .expect("good key");
    assert_eq!(ok.account.label.as_deref(), Some("octo-shop"));
    assert!(matches!(
        r.store.load("github", &other).expect("load"),
        Some(Credential::ApiKey { .. })
    ));
}

#[tokio::test]
async fn f_pl_03_disconnect_revokes_upstream_and_deletes_the_credential() {
    let r = rig().await;
    let conn = ConnectionId::new();
    let done = connect::oauth(
        &r.store,
        &r.loopback,
        r.request(&conn, CancellationToken::new()),
    )
    .await
    .expect("connect");
    let module = github(&r.mock.uri());
    let mut info = tandem_gateway::ConnectionInfo::new(conn.clone(), "github");
    info.meta = done.meta.clone();
    connect::disconnect(&r.store, module.as_ref(), &info)
        .await
        .expect("disconnect");
    assert!(r.stored(&conn).is_none());
    let state: serde_json::Value = reqwest::get(format!("{}/__state", r.mock.uri()))
        .await
        .expect("state")
        .json()
        .await
        .expect("json");
    assert_eq!(state["revoked"].as_array().map(Vec::len), Some(1));
}
