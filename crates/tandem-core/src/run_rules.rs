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
use crate::shellquote::ShellDialect;

/// What cmd.exe expands or escapes on top of the POSIX metacharacters: a
/// rule-matched line with `%NAME%` would hand a variable's value to the
/// command as an argument.
const CMD_META: &[char] = &['%', '^', '!', '"'];

/// Fingerprints of a repository's `.env` kept as approved (the newest).
const KEEP_FINGERPRINTS: usize = 16;

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
    /// Main checkout path → fingerprints of what its runs get from `.env`
    /// (`secrets::fingerprint`) that the person allowed. A rule only runs
    /// without asking while the `.env` is one of them.
    #[serde(default)]
    pub approved_env: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuleError {
    #[error("Enter a command, such as pnpm deploy")]
    Empty,
    #[error("A rule can't hold ; & | < > $ ` ( ) % ^ ! \" or line breaks")]
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
    // Rules are kept for every shell, so cmd's specials are refused too.
    if !matches_rule(&value, command, ShellDialect::Cmd) {
        return Err(RuleError::Meta);
    }
    Ok(value)
}

/// `command` is the rule's command or the rule's command and arguments,
/// with no metacharacter of `dialect`.
fn matches_rule(rule: &str, command: &str, dialect: ShellDialect) -> bool {
    if dialect == ShellDialect::Cmd && command.contains(CMD_META) {
        return false;
    }
    AllowRule::CommandPrefix {
        value: rule.to_string(),
    }
    .allows(&ApprovalSubject::Command(command.to_string()))
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

    /// A rule of `repo` matches `command` under `dialect`'s shell.
    pub fn matches(&self, repo: &str, command: &str, dialect: ShellDialect) -> bool {
        self.repos
            .get(repo)
            .is_some_and(|rules| rules.iter().any(|r| matches_rule(r, command, dialect)))
    }

    /// Whether `command` may run in `repo` without asking: a rule matches,
    /// and what the run gets from `.env` (`fingerprint`) is what the person
    /// allowed before.
    pub fn allows(
        &self,
        repo: &str,
        command: &str,
        dialect: ShellDialect,
        fingerprint: &str,
    ) -> bool {
        self.matches(repo, command, dialect)
            && self
                .approved_env
                .get(repo)
                .is_some_and(|f| f.iter().any(|x| x == fingerprint))
    }

    /// The person allowed a run in `repo` with this `.env`.
    pub fn approve_env(&mut self, repo: &str, fingerprint: &str) {
        let list = self.approved_env.entry(repo.to_string()).or_default();
        list.retain(|f| f != fingerprint);
        list.push(fingerprint.to_string());
        if list.len() > KEEP_FINGERPRINTS {
            let drop = list.len() - KEEP_FINGERPRINTS;
            list.drain(..drop);
        }
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
        use ShellDialect::Posix;
        let mut r = RunRules::default();
        r.set("/w/shop", &["pnpm  deploy".into()]).expect("set");
        r.approve_env("/w/shop", "fp1");
        let ok = |c: &str| r.allows("/w/shop", c, Posix, "fp1");
        assert!(ok("pnpm deploy"));
        assert!(ok("pnpm deploy --prod"));
        assert!(!ok("pnpm deploy; rm -rf x"));
        assert!(!ok("pnpm deploy && rm -rf x"));
        assert!(!ok("pnpm deploy $(id)"));
        assert!(!ok("pnpm deployer"));
        assert!(!r.allows("/w/other", "pnpm deploy", Posix, "fp1"));
    }

    #[test]
    fn f_tl_12_9_a_rule_holds_only_for_the_env_the_person_allowed() {
        use ShellDialect::Posix;
        let mut r = RunRules::default();
        r.set("/w/shop", &["pnpm deploy".into()]).expect("set");
        // Never allowed with any .env yet: it asks.
        assert!(r.matches("/w/shop", "pnpm deploy", Posix));
        assert!(!r.allows("/w/shop", "pnpm deploy", Posix, "fp1"));
        r.approve_env("/w/shop", "fp1");
        assert!(r.allows("/w/shop", "pnpm deploy", Posix, "fp1"));
        // The .env changed: it asks again.
        assert!(!r.allows("/w/shop", "pnpm deploy", Posix, "fp2"));
        for n in 0..20 {
            r.approve_env("/w/shop", &format!("x{n}"));
        }
        assert_eq!(r.approved_env["/w/shop"].len(), KEEP_FINGERPRINTS);
        assert!(!r.allows("/w/shop", "pnpm deploy", Posix, "fp1"));
    }

    #[test]
    fn f_tl_12_9_under_cmd_a_rule_never_matches_cmds_specials() {
        use ShellDialect::{Cmd, Posix};
        let mut r = RunRules::default();
        r.set("C:/w/shop", &["npm run deploy".into()]).expect("set");
        r.approve_env("C:/w/shop", "fp");
        assert!(r.allows("C:/w/shop", "npm run deploy --prod", Cmd, "fp"));
        for bad in [
            "npm run deploy %API_KEY%",
            "npm run deploy ^& calc",
            "npm run deploy !API_KEY!",
            "npm run deploy \"x\"",
        ] {
            assert!(!r.allows("C:/w/shop", bad, Cmd, "fp"), "{bad}");
        }
        // POSIX doesn't expand %…%, so it's an argument there.
        assert!(r.allows("C:/w/shop", "npm run deploy %x", Posix, "fp"));
        assert_eq!(rule("npm %X%"), Err(RuleError::Meta));
        assert_eq!(rule("make !x"), Err(RuleError::Meta));
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
