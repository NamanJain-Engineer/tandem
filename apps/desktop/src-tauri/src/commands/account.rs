//! Auth, entitlements and billing (ARCHITECTURE §3.2).

use serde::{Deserialize, Serialize};
use tauri::State;
use ts_rs::TS;

use tandem_core::settings::SettingKey;
use tandem_core::{ErrorCode, IpcError};
use tandem_store::repo::settings;

use crate::services::auth::AuthState;
use crate::services::entitlement::EntitlementView;
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OpHandle {
    pub op_id: String,
}

#[tauri::command]
pub async fn auth_sign_in(state: State<'_, AppState>) -> Result<OpHandle, IpcError> {
    Ok(OpHandle {
        op_id: state.auth.sign_in().await?,
    })
}

#[tauri::command]
pub fn auth_cancel(state: State<'_, AppState>, op_id: String) {
    let _ = op_id;
    state.auth.cancel_attempt();
}

#[tauri::command]
pub async fn auth_sign_out(state: State<'_, AppState>) -> Result<(), IpcError> {
    // Paired phones go first, while the session can still tell the relay (F-RM-03).
    if let Err(err) = state.phone.revoke_all("signed_out", false).await {
        tracing::warn!(error = %err.message, "couldn't revoke phone pairings at sign-out");
    }
    state.auth.sign_out().await;
    // "Allow for this conversation" ends with the session (F-PL-06).
    state.gateway.forget_approvals(None);
    // Signing out returns to UI-03, not to offline mode.
    settings::reset(&state.store, Some(vec![SettingKey::OfflineStartedAt])).await?;
    Ok(())
}

#[tauri::command]
pub fn auth_state(state: State<'_, AppState>) -> AuthState {
    state.auth.state()
}

#[tauri::command]
pub async fn auth_continue_offline(state: State<'_, AppState>) -> Result<(), IpcError> {
    let started = state.auth.continue_offline();
    settings::set(
        &state.store,
        SettingKey::OfflineStartedAt,
        serde_json::json!(started),
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub fn entitlement_get(state: State<'_, AppState>) -> EntitlementView {
    state.entitlements.view()
}

#[tauri::command]
pub async fn entitlement_refresh(state: State<'_, AppState>) -> Result<EntitlementView, IpcError> {
    Ok(state.entitlements.refresh().await)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum BillingTarget {
    Upgrade {
        plan: String,
        interval: String,
    },
    Portal,
    Credits,
    /// The web account page, where "Delete account…" lives (F-TL-05).
    Account,
}

/// Opens checkout, the billing portal, the credits page or the account page
/// in the system browser (never an in-app webview for payment, UI-121).
#[tauri::command]
pub async fn billing_open(
    state: State<'_, AppState>,
    target: BillingTarget,
) -> Result<(), IpcError> {
    if !state.auth.is_signed_in() {
        return Err(IpcError::new(
            ErrorCode::Unauthenticated,
            "Sign in to upgrade",
        ));
    }
    let api = state.api.clone();
    let url = match target {
        BillingTarget::Upgrade { plan, interval } => {
            if !matches!(plan.as_str(), "pro" | "team")
                || !matches!(interval.as_str(), "month" | "year")
            {
                return Err(IpcError::new(ErrorCode::Invalid, "Unknown plan"));
            }
            state
                .auth
                .with_token(move |t| {
                    let (api, plan, interval) = (api.clone(), plan.clone(), interval.clone());
                    async move { api.checkout(&t, &plan, &interval).await }
                })
                .await
        }
        BillingTarget::Portal => {
            state
                .auth
                .with_token(move |t| {
                    let api = api.clone();
                    async move { api.portal(&t).await }
                })
                .await
        }
        BillingTarget::Credits => Ok(format!("{}/credits", state.endpoints.web)),
        BillingTarget::Account => Ok(format!("{}/account", state.endpoints.web)),
    }
    .map_err(|e| {
        if e.is_network() {
            IpcError::new(
                ErrorCode::Unavailable,
                "Couldn't reach Vibezeo. Check your connection.",
            )
            .with_stage("network")
        } else {
            IpcError::new(ErrorCode::Unavailable, "Couldn't open billing. Try again.")
                .with_stage("billing")
        }
    })?;
    state
        .opener
        .open(&url)
        .map_err(|e| IpcError::new(ErrorCode::Unavailable, e).with_stage("browser"))
}

#[tauri::command]
pub fn telemetry_capture(state: State<'_, AppState>, event: String, props: serde_json::Value) {
    crate::services::telemetry::capture(&state.telemetry, &event, &props);
}
