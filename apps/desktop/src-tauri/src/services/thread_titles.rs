//! Thread titles (F-TH-15, D-118): a local title from the first message at
//! once (its first 8 words), then, only when the user opted in under Privacy
//! and is signed in, a concise one from `/v1/ai/title` after the first reply.
//! A user's rename is never overwritten.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::services::api::ApiClient;
use crate::services::auth::AuthService;

/// Titles are at most this many characters (F-TH-15).
pub const MAX_TITLE: usize = 60;
/// How long the AI title may take before the local one stays.
pub const AI_WAIT: Duration = Duration::from_secs(10);

/// Asks a model for a title; `None` when it can't (offline, signed out,
/// rate-limited, not opted in).
#[async_trait]
pub trait Titler: Send + Sync {
    async fn title(&self, first_message: &str) -> Option<String>;
}

/// Never asks: every title is the local one.
pub struct LocalOnly;

#[async_trait]
impl Titler for LocalOnly {
    async fn title(&self, _: &str) -> Option<String> {
        None
    }
}

/// `/v1/ai/title` with the signed-in user's token, when the opt-in is on.
pub struct ApiTitler {
    pub auth: Arc<AuthService>,
    pub api: Arc<dyn ApiClient>,
    /// Reads the Privacy opt-in (`ai_thread_titles`) at call time.
    pub enabled: Arc<dyn Fn() -> bool + Send + Sync>,
}

#[async_trait]
impl Titler for ApiTitler {
    async fn title(&self, first_message: &str) -> Option<String> {
        if !(self.enabled)() {
            return None;
        }
        let text: String = first_message.chars().take(4000).collect();
        let api = self.api.clone();
        let call = self.auth.with_token(|token| {
            let api = api.clone();
            let text = text.clone();
            async move { api.ai_title(&token, &text).await }
        });
        match tokio::time::timeout(AI_WAIT, call).await {
            Ok(Ok(title)) => clean(&title),
            Ok(Err(e)) => {
                tracing::info!(code = "threads.title_unavailable", error = %e, "AI title unavailable; keeping the local one");
                None
            }
            Err(_) => None,
        }
    }
}

/// Trims quotes, whitespace and trailing punctuation; at most 60 characters.
pub fn clean(raw: &str) -> Option<String> {
    let one_line = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = one_line
        .trim()
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | '“' | '”' | '‘' | '’'))
        .trim_end_matches(['.', ':', ';'])
        .trim();
    if t.is_empty() {
        return None;
    }
    Some(truncate(t))
}

fn truncate(t: &str) -> String {
    if t.chars().count() <= MAX_TITLE {
        return t.to_string();
    }
    let cut: String = t.chars().take(MAX_TITLE - 1).collect();
    // At a word boundary when there is one in the second half.
    let at = cut
        .rfind(' ')
        .filter(|i| *i > MAX_TITLE / 2)
        .unwrap_or(cut.len());
    format!("{}…", cut[..at].trim_end())
}

/// The local title: the first message's first 8 words (F-TH-15), without
/// markdown markers, `@` mentions kept as written.
pub fn fallback(first_message: &str) -> String {
    let words: Vec<&str> = first_message
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| matches!(c, '#' | '*' | '_' | '`' | '>' | '~')))
        .filter(|w| !w.is_empty())
        .take(8)
        .collect();
    if words.is_empty() {
        return "New thread".into();
    }
    let mut title = words.join(" ");
    if let Some(first) = title.chars().next() {
        title = first.to_uppercase().collect::<String>() + &title[first.len_utf8()..];
    }
    truncate(&title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_th_15_the_local_title_is_the_first_eight_words() {
        assert_eq!(
            fallback("fix the login bug where the redirect loops forever after sign in"),
            "Fix the login bug where the redirect loops"
        );
        assert_eq!(
            fallback("## **Explain** `src/auth.ts`"),
            "Explain src/auth.ts"
        );
        assert_eq!(fallback("   \n "), "New thread");
        let long = "a".repeat(100);
        assert_eq!(fallback(&long).chars().count(), MAX_TITLE);
    }

    #[test]
    fn f_th_15_ai_titles_are_cleaned_and_capped() {
        assert_eq!(
            clean("  \"Fix login redirect loop.\"\n").as_deref(),
            Some("Fix login redirect loop")
        );
        assert_eq!(clean("   "), None);
        let long = "word ".repeat(30);
        let t = clean(&long).expect("title");
        assert!(t.chars().count() <= MAX_TITLE);
        assert!(t.ends_with('…'));
    }

    #[tokio::test]
    async fn d_118_local_only_never_asks() {
        assert_eq!(LocalOnly.title("anything").await, None);
    }
}
