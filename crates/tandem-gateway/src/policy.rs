//! The approval policy (ARCHITECTURE §11.7, F-PL-06): a pure decision from
//! the tool's risk class, how the session runs and the plugin's options.
//! Prompts never reach it, so nothing an agent reads or writes can change
//! a decision ("an instruction in a prompt is not authorization").

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::lease::{Attended, SessionDescriptor};
use crate::vendors::{Cost, PluginOptions, RiskClass};
use tandem_core::access::Access;

/// How the calling session runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// A person is there and the session has Full access.
    InteractiveFull,
    /// A person is there, at Edits or Read.
    Interactive,
    /// A routine, task, mail or Auto run: nobody can be asked.
    Unattended,
}

impl Mode {
    pub fn of(session: &SessionDescriptor) -> Self {
        match (&session.attended, session.access) {
            (Attended::Unattended { .. }, _) => Mode::Unattended,
            (Attended::Interactive, Access::Full) => Mode::InteractiveFull,
            (Attended::Interactive, _) => Mode::Interactive,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Run without asking.
    Allow,
    /// Ask the user; `conversation` offers "Allow for this conversation".
    Ask { conversation: bool },
    /// Keep it as a held draft for the user to review (F-PL-08).
    Hold,
}

/// Whether "Allow for this conversation" may be offered for a class:
/// only writes and publishing (never spend, credits, deletes or unknown).
pub fn offers_conversation(class: RiskClass) -> bool {
    matches!(class, RiskClass::Write | RiskClass::ExternalPublish)
}

/// The decision table (ARCHITECTURE §11.7).
pub fn decide(class: RiskClass, mode: Mode, options: &PluginOptions) -> Decision {
    let ask = Decision::Ask {
        conversation: offers_conversation(class),
    };
    match (class, mode) {
        (RiskClass::Read, _) => Decision::Allow,
        (_, Mode::Unattended) => Decision::Hold,
        (RiskClass::Write, Mode::InteractiveFull) if options.allow_writes_in_full => {
            Decision::Allow
        }
        (RiskClass::ExternalPublish, Mode::InteractiveFull) if options.allow_publish_in_full => {
            Decision::Allow
        }
        _ => ask,
    }
}

/// `{"b":[1],"a":{"c":"x"}}` → `a.c:string,b[]:number`: the argument
/// shape "Allow for this conversation" is bound to.
pub fn shape(args: &Value) -> String {
    fn walk(prefix: &str, v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(map) => {
                if map.is_empty() {
                    out.push(format!("{prefix}:object"));
                }
                for (k, v) in map {
                    let p = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{prefix}.{k}")
                    };
                    walk(&p, v, out);
                }
            }
            Value::Array(items) => {
                let p = format!("{prefix}[]");
                if items.is_empty() {
                    out.push(format!("{p}:empty"));
                }
                for item in items {
                    walk(&p, item, out);
                }
            }
            Value::String(_) => out.push(format!("{prefix}:string")),
            Value::Number(_) => out.push(format!("{prefix}:number")),
            Value::Bool(_) => out.push(format!("{prefix}:bool")),
            Value::Null => out.push(format!("{prefix}:null")),
        }
    }
    let mut out = Vec::new();
    walk("", args, &mut out);
    out.sort();
    out.dedup();
    out.join(",")
}

/// What an "Allow for this conversation" remembers: the conversation, the
/// exact tool, the argument shape and the literal target (`owner/repo`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RememberKey {
    pub conversation: String,
    pub plugin: String,
    pub tool: String,
    pub shape: String,
    pub target: Option<String>,
}

/// Remembered "for this conversation" approvals. In memory only, so a quit
/// clears them; sign-out and reconnect clear them too.
#[derive(Default)]
pub struct Remembered {
    keys: std::sync::Mutex<std::collections::HashSet<RememberKey>>,
}

impl Remembered {
    pub fn add(&self, key: RememberKey) {
        if let Ok(mut k) = self.keys.lock() {
            k.insert(key);
        }
    }

    pub fn has(&self, key: &RememberKey) -> bool {
        self.keys.lock().map(|k| k.contains(key)).unwrap_or(false)
    }

    pub fn clear_all(&self) {
        if let Ok(mut k) = self.keys.lock() {
            k.clear();
        }
    }

    pub fn clear_plugin(&self, plugin: &str) {
        if let Ok(mut k) = self.keys.lock() {
            k.retain(|key| key.plugin != plugin);
        }
    }

    pub fn len(&self) -> usize {
        self.keys.lock().map(|k| k.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// JSON with object keys sorted, so equal requests hash equally.
pub fn canonical(v: &Value) -> String {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let inner: Vec<String> = keys
                .into_iter()
                .map(|k| {
                    format!(
                        "{}:{}",
                        Value::String(k.clone()),
                        canonical(map.get(k).unwrap_or(&Value::Null))
                    )
                })
                .collect();
            format!("{{{}}}", inner.join(","))
        }
        Value::Array(items) => {
            format!(
                "[{}]",
                items.iter().map(canonical).collect::<Vec<_>>().join(",")
            )
        }
        other => other.to_string(),
    }
}

/// What a spend approval is bound to: the tool, the exact arguments and the
/// amount and currency the user saw (F-PL-06 AC2).
pub fn approval_hash(tool: &str, args: &Value, cost: Option<&Cost>) -> String {
    let mut h = Sha256::new();
    h.update(tool.as_bytes());
    h.update([0]);
    h.update(canonical(args).as_bytes());
    h.update([0]);
    if let Some(c) = cost {
        h.update(format!("{}:{}", c.amount, c.currency).as_bytes());
    }
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use RiskClass::*;

    /// F-PL-06 AC1: every class × {interactive Full, interactive Edits,
    /// unattended}, 21 cases, with the default options.
    #[test]
    fn f_pl_06_1_policy_table() {
        let ask_conv = Decision::Ask { conversation: true };
        let ask_once = Decision::Ask {
            conversation: false,
        };
        let table: [(RiskClass, [Decision; 3]); 7] = [
            (Read, [Decision::Allow, Decision::Allow, Decision::Allow]),
            (Write, [Decision::Allow, ask_conv, Decision::Hold]),
            (ExternalPublish, [ask_conv, ask_conv, Decision::Hold]),
            (Spend, [ask_once, ask_once, Decision::Hold]),
            (CreditConsuming, [ask_once, ask_once, Decision::Hold]),
            (Destructive, [ask_once, ask_once, Decision::Hold]),
            (Unclassified, [ask_once, ask_once, Decision::Hold]),
        ];
        let opts = PluginOptions::default();
        let mut cases = 0;
        for (class, expected) in table {
            for (mode, want) in [Mode::InteractiveFull, Mode::Interactive, Mode::Unattended]
                .into_iter()
                .zip(expected)
            {
                assert_eq!(decide(class, mode, &opts), want, "{class:?} × {mode:?}");
                cases += 1;
            }
        }
        assert_eq!(cases, 21);
    }

    #[test]
    fn f_pl_06_1_plugin_options_only_change_full_access_sessions() {
        let publish = PluginOptions {
            allow_publish_in_full: true,
            ..PluginOptions::default()
        };
        assert_eq!(
            decide(ExternalPublish, Mode::InteractiveFull, &publish),
            Decision::Allow
        );
        assert_eq!(
            decide(ExternalPublish, Mode::Interactive, &publish),
            Decision::Ask { conversation: true }
        );
        assert_eq!(
            decide(ExternalPublish, Mode::Unattended, &publish),
            Decision::Hold
        );
        let strict = PluginOptions {
            allow_writes_in_full: false,
            ..PluginOptions::default()
        };
        assert_eq!(
            decide(Write, Mode::InteractiveFull, &strict),
            Decision::Ask { conversation: true }
        );
        // Nothing makes spend, deletes or unknown tools run unasked.
        let all = PluginOptions {
            allow_publish_in_full: true,
            allow_writes_in_full: true,
        };
        for class in [Spend, CreditConsuming, Destructive, Unclassified] {
            assert_eq!(
                decide(class, Mode::InteractiveFull, &all),
                Decision::Ask {
                    conversation: false
                }
            );
        }
    }

    #[test]
    fn f_pl_06_3_allow_for_this_conversation_isnt_offered_for_destructive() {
        assert!(!offers_conversation(Destructive));
        assert!(!offers_conversation(Spend));
        assert!(!offers_conversation(Unclassified));
        assert!(!offers_conversation(CreditConsuming));
        assert!(offers_conversation(Write));
        assert!(offers_conversation(ExternalPublish));
    }

    #[test]
    fn f_pl_06_remember_keys_bind_the_argument_shape() {
        assert_eq!(
            shape(&json!({"title": "a", "labels": ["x"], "meta": {"n": 1}})),
            "labels[]:string,meta.n:number,title:string"
        );
        assert_eq!(shape(&json!({"title": "a"})), shape(&json!({"title": "b"})));
        assert_ne!(shape(&json!({"title": "a"})), shape(&json!({"title": 1})));
        let r = Remembered::default();
        let key = RememberKey {
            conversation: "c1".into(),
            plugin: "github".into(),
            tool: "create_issue".into(),
            shape: shape(&json!({"title": "a"})),
            target: Some("o/r".into()),
        };
        r.add(key.clone());
        assert!(r.has(&key));
        assert!(!r.has(&RememberKey {
            conversation: "c2".into(),
            ..key.clone()
        }));
        r.clear_plugin("github");
        assert!(!r.has(&key));
    }

    #[test]
    fn f_pl_06_2_the_spend_hash_moves_with_the_amount() {
        let args = json!({"b": 1, "a": "x"});
        let five = Cost {
            amount: 500,
            currency: "usd".into(),
        };
        let seven = Cost {
            amount: 700,
            currency: "usd".into(),
        };
        assert_eq!(
            approval_hash("charge", &args, Some(&five)),
            approval_hash("charge", &json!({"a": "x", "b": 1}), Some(&five))
        );
        assert_ne!(
            approval_hash("charge", &args, Some(&five)),
            approval_hash("charge", &args, Some(&seven))
        );
        assert_ne!(
            approval_hash("charge", &args, Some(&five)),
            approval_hash("charge", &json!({"a": "y", "b": 1}), Some(&five))
        );
    }
}
