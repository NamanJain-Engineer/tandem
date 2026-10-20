//! Agent-proof secrets (F-TL-12): secret names, `vibezeo-secret://` references
//! and the `.env` files that hold them. The values live in the keychain
//! (`NS.secrets/<name>`); nothing here reads or stores one.

use std::collections::BTreeMap;

/// The scheme a `.env` value names a secret with.
pub const SCHEME: &str = "vibezeo-secret://";

/// Longest name.
pub const MAX_NAME: usize = 64;

/// Why a name can't be used.
pub const NAME_RULES: &str =
    "Use lowercase letters, digits, - and _, up to 64, starting with a letter or digit";

/// Lowercase letters, digits, `-` and `_`, up to 64, starting with a letter
/// or digit.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    name.len() <= MAX_NAME
        && (first.is_ascii_lowercase() || first.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// `vibezeo-secret://stripe-test` → `stripe-test`; `None` for anything else
/// (a plain value, or a reference with a name that isn't valid).
pub fn reference(value: &str) -> Option<&str> {
    value.strip_prefix(SCHEME).filter(|n| valid_name(n))
}

/// One variable of a `.env` file, in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

/// Parses a `.env` file: `KEY=value` lines, an optional `export `, single or
/// double quotes (double quotes take `\n`, `\"` and `\\`), and `#` comments
/// (whole lines, or after an unquoted value's whitespace). Lines that aren't
/// assignments are skipped; a later line for the same name wins.
pub fn parse_env(text: &str) -> Vec<EnvVar> {
    let mut out: Vec<EnvVar> = Vec::new();
    for line in text.lines() {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((name, rest)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if !valid_var(name) {
            continue;
        }
        let value = unquote(rest.trim_start());
        out.retain(|v| v.name != name);
        out.push(EnvVar {
            name: name.to_string(),
            value,
        });
    }
    out
}

fn valid_var(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn unquote(rest: &str) -> String {
    if let Some(body) = rest.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = body.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => match chars.next() {
                    Some('n') => out.push('\n'),
                    Some(other) => out.push(other),
                    None => break,
                },
                c => out.push(c),
            }
        }
        out
    } else if let Some(body) = rest.strip_prefix('\'') {
        body.split('\'').next().unwrap_or_default().to_string()
    } else {
        // An unquoted value ends at a ` #` comment.
        let end = rest
            .char_indices()
            .find(|&(i, c)| c == '#' && i > 0 && rest[..i].ends_with([' ', '\t']))
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        rest[..end].trim_end().to_string()
    }
}

/// What a run would use: each variable, and the secret it refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub name: String,
    /// The secret's name, for a reference.
    pub secret: Option<String>,
}

pub fn plan(vars: &[EnvVar]) -> Vec<Planned> {
    vars.iter()
        .map(|v| Planned {
            name: v.name.clone(),
            secret: reference(&v.value).map(str::to_string),
        })
        .collect()
}

/// A reference to a secret that doesn't exist.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("No secret named {0}")]
pub struct Missing(pub String);

/// The environment a run gets, references replaced by `lookup`'s values, and
/// the values to mask in its pane.
pub struct Resolved {
    pub env: BTreeMap<String, String>,
    pub secrets: Vec<String>,
}

pub fn resolve(
    vars: &[EnvVar],
    mut lookup: impl FnMut(&str) -> Option<String>,
) -> Result<Resolved, Missing> {
    let mut env = BTreeMap::new();
    let mut secrets = Vec::new();
    for v in vars {
        let value = match reference(&v.value) {
            Some(secret) => {
                let value = lookup(secret).ok_or_else(|| Missing(secret.to_string()))?;
                secrets.push(value.clone());
                value
            }
            None => v.value.clone(),
        };
        env.insert(v.name.clone(), value);
    }
    Ok(Resolved { env, secrets })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_tl_12_names_follow_the_rules() {
        for ok in ["stripe-test", "db_url", "a", "9lives"] {
            assert!(valid_name(ok), "{ok}");
        }
        for bad in ["", "Stripe", "-x", "_x", "a b", "a/b", &"x".repeat(65)] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn f_tl_12_a_reference_names_its_secret() {
        assert_eq!(
            reference("vibezeo-secret://stripe-test"),
            Some("stripe-test")
        );
        assert_eq!(reference("vibezeo-secret://Bad"), None);
        assert_eq!(reference("sk_live_123"), None);
        assert_eq!(reference(" vibezeo-secret://x"), None);
    }

    #[test]
    fn f_tl_12_env_files_parse_like_dotenv() {
        let text = "# keys\nexport API_KEY=vibezeo-secret://stripe-test\nPORT = 3000 # dev\n\
                    QUOTED=\"a b\\n\\\"c\\\"\" \nSINGLE='x # y'\nURL=http://h/#frag\nnot a line\n\
                    1BAD=x\nPORT=4000\n";
        let vars = parse_env(text);
        let got: Vec<(&str, &str)> = vars
            .iter()
            .map(|v| (v.name.as_str(), v.value.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("API_KEY", "vibezeo-secret://stripe-test"),
                ("QUOTED", "a b\n\"c\""),
                ("SINGLE", "x # y"),
                ("URL", "http://h/#frag"),
                ("PORT", "4000"),
            ]
        );
    }

    #[test]
    fn f_tl_12_2_references_resolve_and_plain_values_pass_through() {
        let vars = parse_env("API_KEY=\"vibezeo-secret://stripe-test\"\nPORT=3000\n");
        assert_eq!(
            plan(&vars),
            vec![
                Planned {
                    name: "API_KEY".into(),
                    secret: Some("stripe-test".into())
                },
                Planned {
                    name: "PORT".into(),
                    secret: None
                },
            ]
        );
        let r = resolve(&vars, |n| {
            (n == "stripe-test").then(|| "sk_test_42".to_string())
        })
        .expect("resolved");
        assert_eq!(r.env.get("API_KEY").map(String::as_str), Some("sk_test_42"));
        assert_eq!(r.env.get("PORT").map(String::as_str), Some("3000"));
        assert_eq!(r.secrets, vec!["sk_test_42".to_string()]);
    }

    #[test]
    fn f_tl_12_4_a_missing_secret_is_named() {
        let vars = parse_env("A=vibezeo-secret://gone\n");
        let err = resolve(&vars, |_| None).err().expect("missing");
        assert_eq!(err.to_string(), "No secret named gone");
    }
}
