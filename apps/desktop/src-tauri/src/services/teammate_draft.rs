//! "Create with AI" (F-AG-04, D-8): a description of the job becomes a
//! name, a brief, a suggested engine and suggested plugins, from
//! `/v1/ai/teammate-draft` (hosted: a Vibezeo account, signed in, online). Nothing is
//! saved: the sheet is prefilled for review. Offline or with no account the
//! sheet offers the local templates instead (`TEMPLATES`).
//!
//! The route lands with M13; until then the E2E harness answers from a
//! fixture file (`TANDEM_FIXTURE_TEAMMATE_DRAFT`, debug builds only).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_core::{ErrorCode, IpcError};
use ts_rs::TS;

use crate::services::api::ApiError;

/// `teammate_draft_with_ai`: nothing is saved until Create.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TeammateDraft {
    pub name: String,
    pub brief: String,
    /// One of the installed chat engines, else none.
    pub engine_id: Option<String>,
    /// Connected plugins the draft suggests.
    pub plugin_ids: Vec<String>,
}

/// A local starting point (offline or Free).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TeammateTemplate {
    pub id: String,
    pub name: String,
    pub brief: String,
}

/// The four templates (original text written for Vibezeo).
pub fn templates() -> Vec<TeammateTemplate> {
    [
        (
            "reviewer",
            "Reviewer",
            "You review changes before they land. Read the diff and its tests, check correctness, edge cases and security, and list findings most severe first with the file and line. Never approve something you couldn't follow; say what was unclear.",
        ),
        (
            "release-notes",
            "Release notes writer",
            "You draft release notes from merged changes. Group them for users as New, Improved and Fixed, in plain words. Verify every line against the repository. Never publish; hand the draft back for review.",
        ),
        (
            "researcher",
            "Researcher",
            "You answer questions about the codebase and its dependencies. Read before you conclude, cite the files and lines you relied on, and say plainly when you're not sure. Never change files unless asked.",
        ),
        (
            "triage",
            "Triage",
            "You triage incoming issues and failures. Reproduce what you can, find the likely cause and the owner, and suggest a priority with one line of reasoning. Never close an issue without saying why.",
        ),
    ]
    .into_iter()
    .map(|(id, name, brief)| TeammateTemplate {
        id: id.into(),
        name: name.into(),
        brief: brief.into(),
    })
    .collect()
}

/// The API's answer, checked: a usable name, a brief within its cap, an
/// engine that's installed and chat-capable, plugins that are connected.
pub fn sanitize(raw: &Value, engines: &[String], plugins: &[String]) -> Option<TeammateDraft> {
    let name: String = raw
        .get("name")
        .and_then(Value::as_str)?
        .trim()
        .chars()
        .take(tandem_core::teammates::NAME_MAX)
        .collect();
    let name = tandem_core::teammates::validate_name(&name).ok()?;
    let brief: String = raw
        .get("brief")
        .and_then(Value::as_str)?
        .chars()
        .take(tandem_core::teammates::BRIEF_MAX)
        .collect();
    let engine_id = raw
        .get("engine")
        .or_else(|| raw.get("engineId"))
        .and_then(Value::as_str)
        .filter(|e| engines.iter().any(|x| x == e))
        .map(str::to_string);
    let plugin_ids = raw
        .get("plugins")
        .or_else(|| raw.get("pluginIds"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .filter(|p| plugins.iter().any(|x| x == p))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Some(TeammateDraft {
        name,
        brief,
        engine_id,
        plugin_ids,
    })
}

/// What the sheet says when a draft can't be made.
pub const COULDNT_DRAFT: &str = "Couldn't draft right now. Fill in the fields yourself.";

pub fn api_error(e: &ApiError) -> IpcError {
    match e.status() {
        Some(402) => IpcError::new(
            ErrorCode::CreditsExhausted,
            "You're out of credits for this month.",
        ),
        Some(403) => IpcError::new(
            ErrorCode::ForbiddenPlan,
            "Create with AI comes with Vibezeo accounts. It uses Vibezeo's servers and switches on when Vibezeo accounts open.",
        ),
        Some(401) => IpcError::new(ErrorCode::Unauthenticated, "Sign in to draft with AI."),
        _ => IpcError::new(ErrorCode::Unavailable, COULDNT_DRAFT),
    }
}

/// The job description the request carries (at most 2,000 characters).
pub fn job(description: &str) -> String {
    description.trim().chars().take(2000).collect()
}

/// Debug builds under the E2E harness: the fixture's answer, if one is set.
pub fn fixture() -> Option<Value> {
    if !cfg!(debug_assertions) || std::env::var("TANDEM_E2E").as_deref() != Ok("1") {
        return None;
    }
    let path = std::env::var_os("TANDEM_FIXTURE_TEAMMATE_DRAFT")?;
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn f_ag_04_2_a_draft_is_checked_before_it_prefills_anything() {
        let engines = vec!["claude".to_string()];
        let plugins = vec!["test_echo".to_string()];
        let d = sanitize(
            &json!({"name": "  Release notes writer ", "brief": "Drafts notes.", "engine": "claude", "plugins": ["test_echo", "github"]}),
            &engines,
            &plugins,
        )
        .expect("draft");
        assert_eq!(d.name, "Release notes writer");
        assert_eq!(d.engine_id.as_deref(), Some("claude"));
        assert_eq!(d.plugin_ids, vec!["test_echo".to_string()]);
        // An engine that isn't installed isn't suggested; a long name is cut.
        let d = sanitize(
            &json!({"name": "n".repeat(60), "brief": "b".repeat(9000), "engine": "codex"}),
            &engines,
            &plugins,
        )
        .expect("draft");
        assert_eq!(d.name.chars().count(), 40);
        assert_eq!(d.brief.chars().count(), 8000);
        assert_eq!(d.engine_id, None);
        assert!(sanitize(&json!({"brief": "no name"}), &engines, &plugins).is_none());
    }

    #[test]
    fn f_ag_04_the_four_templates_are_valid_teammates() {
        let t = templates();
        assert_eq!(
            t.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(),
            vec!["Reviewer", "Release notes writer", "Researcher", "Triage"]
        );
        for x in t {
            assert!(tandem_core::teammates::validate_name(&x.name).is_ok());
            assert!(x.brief.chars().count() <= tandem_core::teammates::BRIEF_MAX);
        }
    }

    #[test]
    fn f_ag_04_errors_say_what_happened() {
        let e = |status| ApiError::Http {
            status,
            code: "x".into(),
            detail: None,
        };
        assert_eq!(api_error(&e(402)).code, ErrorCode::CreditsExhausted);
        assert_eq!(api_error(&e(403)).code, ErrorCode::ForbiddenPlan);
        assert_eq!(
            api_error(&ApiError::Network("down".into())).message,
            COULDNT_DRAFT
        );
    }
}
