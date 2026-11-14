//! The first-run wizard's step machine (F-SH-08, UI-04).
//!
//! Welcome is the sign-in screen (UI-03); after it come the wizard's three
//! steps: Engines (with each engine's sign-in), Your first folder, Done
//! (DESIGN §5.3, D-2647). The current step is persisted, so quitting
//! mid-way resumes there (AC2). Dictate's setup (F-DI-11) lives in
//! Settings › Dictate; first run points there after the first finish.

use serde::{Deserialize, Serialize};
use tauri::State;
use ts_rs::TS;

use tandem_core::settings::SettingKey;
use tandem_core::{ErrorCode, IpcError};
use tandem_store::repo::{settings, workspaces};
use tandem_store::Store;

use crate::state::AppState;

/// The wizard after Welcome, in order.
pub const STEPS: &[&str] = &["engines", "workspace", "finish"];
const DONE: &str = "done";

/// Where a step saved by the five-step wizard (before D-2647) resumes: its
/// content's new home. Accounts merged into Engines; Dictate and the
/// privacy switches left the wizard, so the next step is the folder.
fn current_step(saved: &str) -> Option<&'static str> {
    match saved {
        "accounts" => Some("engines"),
        "dictate" | "privacy" => Some("workspace"),
        s => STEPS.iter().copied().find(|step| *step == s),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OnboardingState {
    /// The step to show; `None` once done.
    pub step: Option<String>,
    pub done: bool,
    pub steps: Vec<String>,
}

fn state_of(step: Option<&str>) -> OnboardingState {
    let done = step.is_none();
    OnboardingState {
        step: step.map(str::to_string),
        done,
        steps: STEPS.iter().map(|s| s.to_string()).collect(),
    }
}

pub async fn read(store: &Store) -> Result<OnboardingState, IpcError> {
    let value = settings::get(store, SettingKey::OnboardingStep).await?;
    Ok(match value.as_str() {
        Some(DONE) => state_of(None),
        Some(saved) if current_step(saved).is_some() => state_of(current_step(saved)),
        // Never started. A profile from before the wizard (it has
        // workspaces) never sees it; a fresh one starts at the first step.
        _ => {
            if workspaces::list(store).await?.is_empty() {
                state_of(STEPS.first().copied())
            } else {
                state_of(None)
            }
        }
    })
}

pub async fn write(
    store: &Store,
    step: Option<&str>,
    done: bool,
) -> Result<OnboardingState, IpcError> {
    let value = if done {
        DONE
    } else {
        match step {
            Some(s) if STEPS.contains(&s) => s,
            _ => return Err(IpcError::new(ErrorCode::Invalid, "Unknown setup step")),
        }
    };
    settings::set(store, SettingKey::OnboardingStep, serde_json::json!(value)).await?;
    // Setup's end arms the first finish's toast, once per profile: a profile
    // that saw it never sees it again, after Run setup again either.
    if done && settings::get(store, SettingKey::FirstFinish).await? == serde_json::json!("unset") {
        settings::set(store, SettingKey::FirstFinish, serde_json::json!("armed")).await?;
    }
    read(store).await
}

#[tauri::command]
pub async fn onboarding_get(state: State<'_, AppState>) -> Result<OnboardingState, IpcError> {
    read(&state.store).await
}

/// Moves to `step`, or finishes with `done: true`. "Run setup again"
/// (Settings › General) sets the first step.
#[tauri::command]
pub async fn onboarding_set(
    state: State<'_, AppState>,
    step: Option<String>,
    done: Option<bool>,
) -> Result<OnboardingState, IpcError> {
    write(&state.store, step.as_deref(), done.unwrap_or(false)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::DataDir;

    async fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        (dir, Store::open(&data).expect("store"))
    }

    #[test]
    fn f_sh_08_the_wizard_is_three_steps_engines_folder_done() {
        // DESIGN §5.3.2 (D-2647): Accounts merged into Engines; Dictate and
        // privacy left the wizard.
        assert_eq!(STEPS, &["engines", "workspace", "finish"]);
    }

    #[tokio::test]
    async fn f_sh_08_1_a_fresh_profile_starts_at_the_first_step_and_finishes_once() {
        let (_dir, store) = store().await;
        let s = read(&store).await.expect("read");
        assert_eq!(s.step.as_deref(), Some("engines"));
        assert_eq!(s.steps, vec!["engines", "workspace", "finish"]);
        assert!(!s.done);
        let s = write(&store, Some("finish"), false).await.expect("next");
        assert_eq!(s.step.as_deref(), Some("finish"));
        let s = write(&store, None, true).await.expect("finish");
        assert!(s.done);
        assert_eq!(s.step, None);
        assert!(write(&store, Some("bogus"), false).await.is_err());
        // The five-step wizard's names aren't steps any more.
        assert!(write(&store, Some("accounts"), false).await.is_err());
    }

    #[tokio::test]
    async fn f_sh_08_2_the_step_survives_a_relaunch() {
        let (dir, store) = store().await;
        // Step 3 counts Welcome: Welcome, Engines, Your first folder.
        assert_eq!(STEPS[1], "workspace");
        write(&store, Some("workspace"), false)
            .await
            .expect("step 3");
        drop(store);
        let again =
            Store::open(&DataDir::at(dir.path().to_path_buf()).expect("data")).expect("store");
        assert_eq!(
            read(&again).await.expect("read").step.as_deref(),
            Some("workspace")
        );
    }

    #[tokio::test]
    async fn f_sh_08_2_a_step_saved_by_the_five_step_wizard_resumes_where_its_content_went() {
        let (_dir, store) = store().await;
        for (old, now) in [
            ("engines", "engines"),
            ("accounts", "engines"),
            ("dictate", "workspace"),
            ("privacy", "workspace"),
            ("workspace", "workspace"),
        ] {
            settings::set(&store, SettingKey::OnboardingStep, serde_json::json!(old))
                .await
                .expect("set");
            let s = read(&store).await.expect("read");
            assert_eq!(s.step.as_deref(), Some(now), "{old}");
            assert!(!s.done, "{old}");
        }
    }

    #[tokio::test]
    async fn f_sh_08_5_finishing_setup_arms_the_first_finish_once() {
        let (_dir, store) = store().await;
        let first = || async {
            settings::get(&store, SettingKey::FirstFinish)
                .await
                .expect("get")
        };
        assert_eq!(first().await, serde_json::json!("unset"));
        // Moving between steps arms nothing.
        write(&store, Some("workspace"), false).await.expect("step");
        assert_eq!(first().await, serde_json::json!("unset"));
        write(&store, None, true).await.expect("done");
        assert_eq!(first().await, serde_json::json!("armed"));
        // The toast showed; Run setup again and finishing again never re-arms it.
        settings::set(&store, SettingKey::FirstFinish, serde_json::json!("shown"))
            .await
            .expect("shown");
        write(&store, Some("engines"), false).await.expect("again");
        write(&store, None, true).await.expect("done again");
        assert_eq!(first().await, serde_json::json!("shown"));
    }

    #[tokio::test]
    async fn f_sh_08_a_profile_from_before_the_wizard_never_sees_it() {
        let (dir, store) = store().await;
        let root = dir.path().join("proj");
        std::fs::create_dir_all(&root).expect("mkdir");
        crate::services::workspaces::add(&store, root.to_str().expect("utf8"), None)
            .await
            .expect("ws");
        assert!(read(&store).await.expect("read").done);
        // "Run setup again" still works.
        assert_eq!(
            write(&store, Some("engines"), false)
                .await
                .expect("again")
                .step
                .as_deref(),
            Some("engines")
        );
    }
}
