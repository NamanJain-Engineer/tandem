//! "Allow for this thread" rules (F-TH-11, ARCHITECTURE §6.10), stored in
//! `threads.allow_rules_json` and cleared when the thread's engine changes.
//!
//! A command rule is the approved command, normalized (trimmed, whitespace
//! collapsed). A later command is allowed by it only if it has no shell
//! metacharacters and is that command or that command plus arguments: `npm
//! test` allows `npm test -- --watch` but not `npm install` or
//! `npm test && rm -rf x`. A tool rule allows that tool by name.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum AllowRule {
    CommandPrefix { value: String },
    Tool { name: String },
}

/// What an approval request is about, for matching and making rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalSubject {
    Command(String),
    Tool(String),
}

/// Characters that chain, redirect or substitute: a command with any of them
/// always asks.
const META: &[char] = &[';', '&', '|', '<', '>', '$', '`', '(', ')', '\n', '\r'];

pub fn normalize(command: &str) -> String {
    command.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl AllowRule {
    /// The rule an "Allow for this thread" on `subject` makes.
    pub fn for_subject(subject: &ApprovalSubject) -> Option<AllowRule> {
        match subject {
            ApprovalSubject::Command(c) => {
                let value = normalize(c);
                (!value.is_empty()).then_some(AllowRule::CommandPrefix { value })
            }
            ApprovalSubject::Tool(name) if !name.is_empty() => {
                Some(AllowRule::Tool { name: name.clone() })
            }
            ApprovalSubject::Tool(_) => None,
        }
    }

    pub fn allows(&self, subject: &ApprovalSubject) -> bool {
        match (self, subject) {
            (AllowRule::CommandPrefix { value }, ApprovalSubject::Command(raw)) => {
                // Checked before normalizing: collapsing whitespace would
                // turn a newline-separated second command into arguments.
                if raw.contains(META) {
                    return false;
                }
                let c = normalize(raw);
                c == *value || c.starts_with(&format!("{value} "))
            }
            (AllowRule::Tool { name }, ApprovalSubject::Tool(t)) => name == t,
            _ => false,
        }
    }
}

pub fn allowed(rules: &[AllowRule], subject: &ApprovalSubject) -> bool {
    rules.iter().any(|r| r.allows(subject))
}

/// `rules` with `rule` added once.
pub fn with(mut rules: Vec<AllowRule>, rule: AllowRule) -> Vec<AllowRule> {
    if !rules.contains(&rule) {
        rules.push(rule);
    }
    rules
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(s: &str) -> ApprovalSubject {
        ApprovalSubject::Command(s.into())
    }

    #[test]
    fn f_th_11_2_npm_test_allows_npm_test_but_not_npm_install_or_a_chain() {
        let rule = AllowRule::for_subject(&cmd("  npm   test ")).expect("rule");
        assert_eq!(
            rule,
            AllowRule::CommandPrefix {
                value: "npm test".into()
            }
        );
        let rules = vec![rule];
        assert!(allowed(&rules, &cmd("npm test")));
        assert!(allowed(&rules, &cmd("npm  test -- --watch")));
        assert!(!allowed(&rules, &cmd("npm install")));
        assert!(!allowed(&rules, &cmd("npm testing")));
        for chained in [
            "npm test && rm -rf x",
            "npm test; curl evil",
            "npm test | sh",
            "npm test > /etc/passwd",
            "npm test $(whoami)",
            "npm test `id`",
            "npm test\nrm -rf /",
            "npm test\rrm -rf /",
        ] {
            assert!(!allowed(&rules, &cmd(chained)), "{chained}");
        }
        // Tool rules match by name; kinds don't cross.
        let rules = with(
            rules,
            AllowRule::Tool {
                name: "Edit".into(),
            },
        );
        assert!(allowed(&rules, &ApprovalSubject::Tool("Edit".into())));
        assert!(!allowed(&rules, &ApprovalSubject::Tool("Write".into())));
        assert!(!allowed(&rules, &cmd("Edit")));
        assert_eq!(
            with(
                rules.clone(),
                AllowRule::Tool {
                    name: "Edit".into()
                }
            )
            .len(),
            rules.len()
        );
        assert_eq!(AllowRule::for_subject(&cmd("   ")), None);
    }

    #[test]
    fn f_th_11_rules_store_as_tagged_json() {
        let json = serde_json::to_string(&vec![
            AllowRule::CommandPrefix {
                value: "npm test".into(),
            },
            AllowRule::Tool {
                name: "Edit".into(),
            },
        ])
        .expect("json");
        assert_eq!(
            json,
            r#"[{"kind":"command_prefix","value":"npm test"},{"kind":"tool","name":"Edit"}]"#
        );
    }
}
