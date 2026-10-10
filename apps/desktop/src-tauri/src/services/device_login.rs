//! Device-code sign-in (F-BE-03) for runs with no browser at hand: the
//! headless routine runner and any future CLI. The user approves the code at
//! `verification_uri`; this polls at the server's interval, slowing down
//! when asked, until tokens arrive, the user denies, or the code expires.

use std::future::Future;
use std::time::Duration;

use super::api::{ApiClient, ApiError, DeviceStart, TokenPair};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DeviceLoginError {
    #[error("the sign-in was denied")]
    Denied,
    #[error("the code expired before it was approved")]
    Expired,
    #[error(transparent)]
    Api(#[from] ApiError),
}

fn oauth_error(e: &ApiError) -> Option<&str> {
    match e {
        ApiError::Http {
            status: 400, code, ..
        } => Some(code.as_str()),
        _ => None,
    }
}

/// Starts a device sign-in, hands the code to `show`, then polls until it
/// resolves. `sleep` waits between polls (tests pass an instant one).
pub async fn sign_in<S, F>(
    api: &dyn ApiClient,
    client: &str,
    version: &str,
    show: impl FnOnce(&DeviceStart),
    sleep: S,
) -> Result<TokenPair, DeviceLoginError>
where
    S: Fn(Duration) -> F,
    F: Future<Output = ()>,
{
    let start = api.device_start(client, version).await?;
    show(&start);
    let mut interval = start.interval.max(1);
    // The server's clock decides expiry; this only bounds a lost server.
    let mut budget = start.expires_in / interval + 10;
    loop {
        sleep(Duration::from_secs(interval)).await;
        match api.device_token(&start.device_code).await {
            Ok(tokens) => return Ok(tokens),
            Err(e) => match oauth_error(&e) {
                Some("authorization_pending") => {}
                Some("slow_down") => interval += 5,
                Some("access_denied") => return Err(DeviceLoginError::Denied),
                Some("expired_token") => return Err(DeviceLoginError::Expired),
                // A dropped connection mid-wait: keep waiting for the user.
                _ if e.is_network() => {}
                _ => return Err(DeviceLoginError::Api(e)),
            },
        }
        budget = budget.saturating_sub(1);
        if budget == 0 {
            return Err(DeviceLoginError::Expired);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::stream::BoxStream;
    use std::sync::Mutex;
    use tandem_core::remote_config::SignedConfig;

    use crate::services::api::MeResponse;

    /// Answers `device_token` from a script of OAuth error codes, then tokens.
    struct Script {
        answers: Mutex<Vec<&'static str>>,
        polls: Mutex<u32>,
    }

    fn http(code: &str) -> ApiError {
        ApiError::Http {
            status: 400,
            code: code.into(),
            detail: None,
        }
    }

    #[async_trait]
    impl ApiClient for Script {
        async fn exchange(&self, _: &str, _: &str, _: &str) -> Result<TokenPair, ApiError> {
            unreachable!()
        }
        async fn refresh(&self, _: &str) -> Result<TokenPair, ApiError> {
            unreachable!()
        }
        async fn logout(&self, _: &str, _: &str) -> Result<(), ApiError> {
            unreachable!()
        }
        async fn me(&self, _: &str) -> Result<MeResponse, ApiError> {
            unreachable!()
        }
        async fn checkout(&self, _: &str, _: &str, _: &str) -> Result<String, ApiError> {
            unreachable!()
        }
        async fn portal(&self, _: &str) -> Result<String, ApiError> {
            unreachable!()
        }
        async fn config(&self) -> Result<SignedConfig, ApiError> {
            unreachable!()
        }
        async fn ai_title(&self, _: &str, _: &str) -> Result<String, ApiError> {
            unreachable!()
        }
        async fn events(
            &self,
            _: &str,
        ) -> Result<BoxStream<'static, Result<String, ApiError>>, ApiError> {
            unreachable!()
        }
        async fn device_start(&self, _: &str, _: &str) -> Result<DeviceStart, ApiError> {
            Ok(DeviceStart {
                device_code: "dev".into(),
                user_code: "BCDF-GHJK".into(),
                verification_uri: "https://app.vibezeo.com/device".into(),
                verification_uri_complete: "https://app.vibezeo.com/device?code=BCDF-GHJK".into(),
                interval: 5,
                expires_in: 600,
            })
        }
        async fn device_token(&self, _: &str) -> Result<TokenPair, ApiError> {
            *self.polls.lock().expect("lock") += 1;
            let next = self.answers.lock().expect("lock").remove(0);
            match next {
                "ok" => Ok(TokenPair {
                    access_token: "a".into(),
                    refresh_token: "r".into(),
                    expires_in: 3600,
                }),
                code => Err(http(code)),
            }
        }
    }

    fn script(answers: Vec<&'static str>) -> Script {
        Script {
            answers: Mutex::new(answers),
            polls: Mutex::new(0),
        }
    }

    #[tokio::test]
    async fn f_be_03_polls_until_approved_and_slows_down_when_asked() {
        let api = script(vec![
            "authorization_pending",
            "slow_down",
            "authorization_pending",
            "ok",
        ]);
        let waits = Mutex::new(Vec::new());
        let mut shown = String::new();
        let tokens = sign_in(
            &api,
            "Vibezeo",
            "0.3.0",
            |s| shown = s.verification_uri_complete.clone(),
            |d| {
                waits.lock().expect("lock").push(d.as_secs());
                async {}
            },
        )
        .await
        .expect("tokens");
        assert_eq!(tokens.access_token, "a");
        assert_eq!(shown, "https://app.vibezeo.com/device?code=BCDF-GHJK");
        assert_eq!(*waits.lock().expect("lock"), vec![5, 5, 10, 10]);
    }

    #[tokio::test]
    async fn f_be_03_a_denied_or_expired_code_ends_the_wait() {
        let denied = sign_in(
            &script(vec!["access_denied"]),
            "c",
            "v",
            |_| {},
            |_| async {},
        )
        .await;
        assert_eq!(denied, Err(DeviceLoginError::Denied));
        let expired = sign_in(
            &script(vec!["authorization_pending", "expired_token"]),
            "c",
            "v",
            |_| {},
            |_| async {},
        )
        .await;
        assert_eq!(expired, Err(DeviceLoginError::Expired));
    }
}
