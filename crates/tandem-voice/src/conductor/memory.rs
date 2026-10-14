//! Conductor's memory during a call (F-CN-06): "remember that…", "what do
//! you know about me" and "forget…" are answered in the app, like the speed
//! and yes/no answers, with no planner round; and when the call ends, the
//! things the user said about themselves become proposed memories for the
//! "After each call" setting (Ask: cards, Auto: saved, Off: nothing).
//!
//! Proposals come from a local reading of the user's own words (first-person
//! statements such as "I prefer…", "I work on…"), so nothing leaves the
//! machine for reflection. The store refuses anything while memory is off,
//! and anything credential-shaped; the call checks credentials too, so a
//! spoken secret never reaches the host.

use tandem_core::conductor_persona::looks_like_credential;

/// A memory request, handled in the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryAsk {
    Remember(String),
    Recall,
    Forget(String),
}

/// Why a fact wasn't remembered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RememberRefusal {
    /// "Remember things between calls" is off: nothing is written.
    Off,
    /// It looks like a password, key or token.
    Credential,
    /// Nothing to remember, or the file couldn't be written.
    Failed,
}

/// "remember (that) …", "what do you know about me", "forget (that|about) …".
pub fn parse_memory(request: &str) -> Option<MemoryAsk> {
    let r = request.trim();
    let lower = r.to_lowercase();
    let lower = lower.trim_end_matches(['.', '!', '?']);
    for recall in [
        "what do you know about me",
        "what do you remember about me",
        "what do you remember",
        "what have you remembered",
        "what have you remembered about me",
        "what's in your memory",
    ] {
        if lower == recall {
            return Some(MemoryAsk::Recall);
        }
    }
    let rest = |prefixes: &[&str]| {
        prefixes.iter().find_map(|p| {
            lower
                .starts_with(p)
                .then(|| r.get(p.len()..))
                .flatten()
                .map(|s| {
                    s.trim()
                        .trim_end_matches(['.', '!', '?'])
                        .trim()
                        .to_string()
                })
                .filter(|s| !s.is_empty())
        })
    };
    if let Some(fact) = rest(&[
        "remember that ",
        "please remember that ",
        "please remember ",
        "remember ",
    ]) {
        return Some(MemoryAsk::Remember(fact));
    }
    if let Some(query) = rest(&[
        "forget that ",
        "forget about ",
        "please forget that ",
        "please forget about ",
        "please forget ",
        "forget ",
    ]) {
        // "Forget it" means "never mind": like any request, it goes to the
        // planner, and memory is left alone.
        return names_something(&query).then_some(MemoryAsk::Forget(query));
    }
    None
}

/// What "forget …" says when it names nothing to forget.
const NEVER_MIND: [&str; 17] = [
    "it",
    "that",
    "this",
    "them",
    "those",
    "these",
    "it all",
    "all of it",
    "all that",
    "all of that",
    "all this",
    "that one",
    "this one",
    "everything",
    "i said that",
    "what i said",
    "what i just said",
];

/// Whether a "forget …" names something, rather than meaning "never mind"
/// ("forget it", "forget that, open code mode instead").
fn names_something(query: &str) -> bool {
    let lower = query.to_lowercase();
    let head = lower.split([',', ';']).next().unwrap_or_default().trim();
    !NEVER_MIND.contains(&head)
}

/// What Conductor says back for a remember request.
pub fn remembered_reply(result: Result<(), RememberRefusal>) -> String {
    match result {
        Ok(()) => "Okay, I'll remember that.".into(),
        Err(RememberRefusal::Off) => {
            "Memory is off, so I didn't save that. You can turn it on in Settings, Conductor."
                .into()
        }
        Err(RememberRefusal::Credential) => {
            "That looks like a password or key, so I won't remember it.".into()
        }
        Err(RememberRefusal::Failed) => "I couldn't save that just now.".into(),
    }
}

/// The remember step as the call runs it: a credential is refused here,
/// before the host sees it (F-CN-06 AC2).
pub fn check_fact(fact: &str) -> Result<(), RememberRefusal> {
    if looks_like_credential(fact) {
        return Err(RememberRefusal::Credential);
    }
    Ok(())
}

/// What Conductor says back for "what do you know about me".
pub fn recall_reply(memory_on: bool, facts: &[String]) -> String {
    if !memory_on {
        return "Memory is off, so I don't keep anything between calls.".into();
    }
    match facts {
        [] => "I haven't remembered anything about you yet.".into(),
        facts => format!("Here's what I remember: {}.", facts.join("; ")),
    }
}

/// What Conductor says back for "forget …".
pub fn forgot_reply(query: &str, removed: usize) -> String {
    match removed {
        0 => format!("I didn't have anything about {query}."),
        1 => "Okay, I've forgotten that.".into(),
        n => format!("Okay, I've forgotten {n} things."),
    }
}

/// At most this many proposals from one call.
pub const MAX_PROPOSALS: usize = 5;

/// First-person openings that state something lasting about the user.
const ABOUT_ME: [&str; 14] = [
    "i prefer ",
    "i like to ",
    "i usually ",
    "i always ",
    "i never ",
    "i work on ",
    "i'm working on ",
    "i am working on ",
    "i use ",
    "i mostly use ",
    "my name is ",
    "call me ",
    "my team ",
    "i'm a ",
];

/// Post-call reflection: the user's own statements about themselves, from
/// what they said during the call (already redacted), as proposed memories.
/// Questions, requests to remember (already handled), credential-shaped text
/// and repeats are left out.
pub fn proposals(said: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for utterance in said {
        for sentence in utterance.split(['.', '!', '\n']) {
            let s = sentence.trim();
            let lower = s.to_lowercase();
            if s.ends_with('?')
                || s.contains('?')
                || lower.starts_with("remember")
                || !ABOUT_ME.iter().any(|p| lower.starts_with(p))
                || looks_like_credential(s)
                || s.contains('[')
            {
                continue;
            }
            let fact: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
            if !out.iter().any(|o| o.eq_ignore_ascii_case(&fact)) {
                out.push(fact);
            }
            if out.len() == MAX_PROPOSALS {
                return out;
            }
        }
    }
    out
}

/// The call journal's line: when (local time), how long and which tools
/// ran, never what was said (so "Don't keep" call logs has nothing of the
/// call's content to keep out of it).
pub fn journal_line(time: &str, seconds: u32, tools: &[String]) -> String {
    let mins = seconds.div_ceil(60).max(1);
    let mut used: Vec<&str> = Vec::new();
    for t in tools {
        if !used.contains(&t.as_str()) {
            used.push(t);
        }
    }
    let actions = tools.len();
    let what = if used.is_empty() {
        "no actions".to_string()
    } else {
        format!(
            "{actions} action{}: {}",
            if actions == 1 { "" } else { "s" },
            used.join(", ")
        )
    };
    format!("{time} call, {mins} min, {what}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_cn_06_memory_requests_parse_and_others_do_not() {
        assert_eq!(
            parse_memory("Remember that I deploy on Fridays."),
            Some(MemoryAsk::Remember("I deploy on Fridays".into()))
        );
        assert_eq!(
            parse_memory("remember the billing service is mine"),
            Some(MemoryAsk::Remember("the billing service is mine".into()))
        );
        assert_eq!(
            parse_memory("What do you know about me?"),
            Some(MemoryAsk::Recall)
        );
        assert_eq!(
            parse_memory("what do you remember"),
            Some(MemoryAsk::Recall)
        );
        assert_eq!(
            parse_memory("Forget that I deploy on Fridays"),
            Some(MemoryAsk::Forget("I deploy on Fridays".into()))
        );
        assert_eq!(
            parse_memory("forget about Fridays."),
            Some(MemoryAsk::Forget("Fridays".into()))
        );
        for other in [
            "open code mode",
            "remember",
            "forget",
            "do you remember the last thread?",
            "launch two claude agents",
        ] {
            assert_eq!(parse_memory(other), None, "{other}");
        }
    }

    #[test]
    fn f_cn_06_forget_it_means_never_mind_and_leaves_memory_alone() {
        // "Forget …" removes every fact that names what follows, so a "forget
        // that" meaning "never mind" would take every fact with "that" in it.
        for never_mind in [
            "forget it",
            "Forget that.",
            "forget about it",
            "forget this",
            "Forget all that!",
            "forget that, open code mode instead",
            "forget I said that",
            "forget everything",
        ] {
            assert_eq!(parse_memory(never_mind), None, "{never_mind}");
        }
        assert_eq!(
            parse_memory("forget that I use yarn"),
            Some(MemoryAsk::Forget("I use yarn".into()))
        );
        assert_eq!(
            parse_memory("Forget about the billing service."),
            Some(MemoryAsk::Forget("the billing service".into()))
        );
    }

    #[test]
    fn f_cn_06_2_a_spoken_credential_is_refused_before_the_host() {
        assert_eq!(
            check_fact("my token is sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789"),
            Err(RememberRefusal::Credential)
        );
        assert_eq!(
            check_fact("my password is hunter22"),
            Err(RememberRefusal::Credential)
        );
        assert_eq!(check_fact("I deploy on Fridays"), Ok(()));
        assert!(remembered_reply(Err(RememberRefusal::Credential)).contains("won't remember"));
        assert!(remembered_reply(Err(RememberRefusal::Off)).contains("Memory is off"));
    }

    #[test]
    fn f_cn_06_replies_say_what_happened() {
        assert_eq!(
            recall_reply(true, &["Deploys on Fridays".into(), "Uses pnpm".into()]),
            "Here's what I remember: Deploys on Fridays; Uses pnpm."
        );
        assert!(recall_reply(true, &[]).contains("haven't remembered"));
        assert!(recall_reply(false, &["x".into()]).contains("Memory is off"));
        assert_eq!(
            forgot_reply("Fridays", 0),
            "I didn't have anything about Fridays."
        );
        assert_eq!(forgot_reply("Fridays", 1), "Okay, I've forgotten that.");
        assert_eq!(forgot_reply("pnpm", 2), "Okay, I've forgotten 2 things.");
    }

    #[test]
    fn f_cn_06_reflection_proposes_what_the_user_said_about_themselves() {
        let said = vec![
            "Open code mode.".to_string(),
            "I prefer pnpm over npm. Launch two agents".to_string(),
            "I usually review in the morning".to_string(),
            "Do I always do that?".to_string(),
            "remember that I like tea".to_string(),
            "My password is hunter22".to_string(),
            "my token is [REDACTED]".to_string(),
            "I prefer pnpm over npm".to_string(),
        ];
        assert_eq!(
            proposals(&said),
            vec![
                "I prefer pnpm over npm".to_string(),
                "I usually review in the morning".to_string()
            ]
        );
        let many: Vec<String> = (0..9).map(|i| format!("I use tool {i}")).collect();
        assert_eq!(proposals(&many).len(), MAX_PROPOSALS);
        assert!(proposals(&["Open code mode".into()]).is_empty());
    }

    #[test]
    fn f_cn_06_the_journal_line_has_no_words_from_the_call() {
        let tools = vec![
            "launch_agents".to_string(),
            "read_pane".to_string(),
            "read_pane".to_string(),
        ];
        assert_eq!(
            journal_line("14:05", 65, &tools),
            "14:05 call, 2 min, 3 actions: launch_agents, read_pane"
        );
        assert_eq!(
            journal_line("09:00", 5, &[]),
            "09:00 call, 1 min, no actions"
        );
    }
}
