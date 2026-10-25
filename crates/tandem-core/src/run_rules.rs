//! Commands agents may run with secrets without asking (F-TL-12 slice 2):
//! `$DATA/run-rules.json`, one list per repository (its main checkout, so
//! its worktrees share them). A rule is a command and its arguments, matched
//! like a thread's "Allow for this thread" rule: `pnpm deploy` allows
//! `pnpm deploy --prod`, never `pnpm deploy; rm -rf x`. Rules are the only
//! patterns; an approval from a pane covers its exact command only.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::allow_rules::{normalize, AllowRule, ApprovalSubject};

/// The rules file's name in the data folder.
pub const FILE: &str = "run-rules.json";

/// A larger file isn't read.
const MAX_BYTES: u64 = 256 * 1024;

/// Longest rule.
pub const MAX_RULE: usize = 512;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRules {
    /// Main checkout path → its commands, normalized.
    #[serde(default)]
    pub repos: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuleError {
    #[error("Enter a command, such as pnpm deploy")]
    Empty,
    #[error("A rule can't hold ; & | < > $ ` ( ) or line breaks")]
    Meta,
    #[error("A rule can be up to 512 characters")]
    TooLong,
}

/// A rule as it's stored, or why it can't be one.
pub fn rule(command: &str) -> Result<String, RuleError> {
    let value = normalize(command);
    if value.is_empty() {
        return Err(RuleError::Empty);
    }
    if value.len() > MAX_RULE {
        return Err(RuleError::TooLong);
    }
    // A rule with a metacharacter would never match (they always ask), so
    // it's refused rather than kept as dead weight.
    let itself = AllowRule::CommandPrefix {
        value: value.clone(),
    };
    if !itself.allows(&ApprovalSubject::Command(command.to_string())) {
        return Err(RuleError::Meta);
    }
    Ok(value)
}

impl RunRules {
    /// The file in `data`; missing, unreadable or too large reads as none.
    pub fn load(data: &Path) -> Self {
        let path = data.join(FILE);
        let fits = std::fs::metadata(&path).is_ok_and(|m| m.len() <= MAX_BYTES);
        if !fits {
            return Self::default();
        }
        std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, data: &Path) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        crate::atomic::atomic_write(&data.join(FILE), &bytes, Some(0o600))
    }

    /// Whether `command` may run in `repo` without asking.
    pub fn allows(&self, repo: &str, command: &str) -> bool {
        let subject = ApprovalSubject::Command(command.to_string());
        self.repos.get(repo).is_some_and(|rules| {
            rules.iter().any(|value| {
                AllowRule::CommandPrefix {
                    value: value.clone(),
                }
                .allows(&subject)
            })
        })
    }

    /// Replaces `repo`'s rules (none removes it), each checked.
    pub fn set(&mut self, repo: &str, commands: &[String]) -> Result<(), RuleError> {
        let mut values: Vec<String> = Vec::new();
        for c in commands {
            let v = rule(c)?;
            if !values.contains(&v) {
                values.push(v);
            }
        }
        if values.is_empty() {
            self.repos.remove(repo);
        } else {
            self.repos.insert(repo.to_string(), values);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_tl_12_9_a_rule_allows_its_command_with_arguments_in_its_repository_only() {
        let mut r = RunRules::default();
        r.set("/w/shop", &["pnpm  deploy".into()]).expect("set");
        assert!(r.allows("/w/shop", "pnpm deploy"));
        assert!(r.allows("/w/shop", "pnpm deploy --prod"));
        assert!(!r.allows("/w/shop", "pnpm deploy; rm -rf x"));
        assert!(!r.allows("/w/shop", "pnpm deploy && rm -rf x"));
        assert!(!r.allows("/w/shop", "pnpm deploy $(id)"));
        assert!(!r.allows("/w/shop", "pnpm deployer"));
        assert!(!r.allows("/w/other", "pnpm deploy"));
    }

    #[test]
    fn f_tl_12_9_rules_are_checked_and_saved() {
        assert_eq!(rule("  "), Err(RuleError::Empty));
        assert_eq!(rule("make; ls"), Err(RuleError::Meta));
        assert_eq!(rule("echo $X"), Err(RuleError::Meta));
        assert_eq!(rule(&"x".repeat(513)), Err(RuleError::TooLong));
        assert_eq!(rule(" pnpm   test "), Ok("pnpm test".into()));
        let dir = tempfile::tempdir().expect("dir");
        assert_eq!(RunRules::load(dir.path()), RunRules::default());
        let mut r = RunRules::default();
        r.set("/w/a", &["make".into(), "make".into(), "pnpm test".into()])
            .expect("set");
        r.save(dir.path()).expect("save");
        let back = RunRules::load(dir.path());
        assert_eq!(back.repos["/w/a"], vec!["make", "pnpm test"]);
        let mut r = back;
        r.set("/w/a", &[]).expect("clear");
        assert!(r.repos.is_empty());
        std::fs::write(dir.path().join(FILE), "not json").expect("write");
        assert_eq!(RunRules::load(dir.path()), RunRules::default());
    }
}
