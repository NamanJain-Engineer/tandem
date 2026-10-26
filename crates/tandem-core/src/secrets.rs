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
/// (whole lines, or after an unquoted value's whitespace). A leading BOM is
/// dropped. A quoted value may run over several lines up to its closing
/// quote; one that never closes is skipped and the lines after it are read
/// as usual. Lines that aren't assignments are skipped; a later line for the
/// same name wins.
pub fn parse_env(text: &str) -> Vec<EnvVar> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<EnvVar> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim_start();
        i += 1;
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
        let rest = rest.trim_start();
        let value = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => {
                let mut body = rest[1..].to_string();
                let mut next = i;
                loop {
                    if let Some(v) = closed(&body, q) {
                        i = next;
                        break Some(v);
                    }
                    let Some(more) = lines.get(next) else {
                        break None;
                    };
                    body.push('\n');
                    body.push_str(more);
                    next += 1;
                }
            }
            _ => Some(unquoted(rest)),
        };
        let Some(value) = value else {
            continue;
        };
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

/// A quoted value's text up to its closing `quote`; `None` while it's open.
fn closed(body: &str, quote: char) -> Option<String> {
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c == quote => return Some(out),
            '\\' if quote == '"' => match chars.next() {
                Some('n') => out.push('\n'),
                Some(other) => out.push(other),
                None => return None,
            },
            c => out.push(c),
        }
    }
    None
}

/// An unquoted value ends at a ` #` comment.
fn unquoted(rest: &str) -> String {
    let end = rest
        .char_indices()
        .find(|&(i, c)| c == '#' && i > 0 && rest[..i].ends_with([' ', '\t']))
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    rest[..end].trim_end().to_string()
}

/// A value that looks like a reference but isn't one (a bad name, spaces, a
/// different case): a run refuses it rather than pass the text through.
pub fn malformed(value: &str) -> bool {
    reference(value).is_none()
        && value
            .trim()
            .get(..SCHEME.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(SCHEME))
}

/// What a run would use: each variable, and the secret it refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub name: String,
    /// The secret's name, for a reference.
    pub secret: Option<String>,
    /// The value looks like a reference but isn't a valid one.
    pub malformed: bool,
}

pub fn plan(vars: &[EnvVar]) -> Vec<Planned> {
    vars.iter()
        .map(|v| Planned {
            name: v.name.clone(),
            secret: reference(&v.value).map(str::to_string),
            malformed: malformed(&v.value),
        })
        .collect()
}

/// Variables an agent's `vibezeo run` never gets from a `.env`: ones that
/// make a loader, shell or interpreter run code or look elsewhere for it
/// (F-TL-12 slice 2). Checked without case, for Windows.
const AGENT_REFUSED: &[&str] = &[
    "PATH",
    "IFS",
    "ENV",
    "BASH_ENV",
    "ZDOTDIR",
    "PS4",
    "PROMPT_COMMAND",
    "SHELLOPTS",
    "BASHOPTS",
    "SHELL",
    "HOME",
    "TMPDIR",
    "COMSPEC",
    "PATHEXT",
    // On Windows, lets cmd.exe find a bare command in the run's folder again.
    "NODEFAULTCURRENTDIRECTORYINEXEPATH",
    "NODE_OPTIONS",
    "NODE_PATH",
    "PYTHONSTARTUP",
    "PYTHONPATH",
    "PYTHONHOME",
    "PYTHONINSPECT",
    "PYTHONUSERBASE",
    "PERL5OPT",
    "PERL5LIB",
    "PERLLIB",
    "RUBYOPT",
    "RUBYLIB",
    "JAVA_TOOL_OPTIONS",
    "_JAVA_OPTIONS",
    "JDK_JAVA_OPTIONS",
    "CLASSPATH",
    "LUA_INIT",
    "LUA_PATH",
    "LUA_CPATH",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "CURL_CA_BUNDLE",
    "REQUESTS_CA_BUNDLE",
    "NODE_EXTRA_CA_CERTS",
    "EDITOR",
    "VISUAL",
    "PAGER",
];

/// Prefixes refused the same way (`LD_PRELOAD`, `DYLD_INSERT_LIBRARIES`,
/// `GIT_SSH_COMMAND`, `NPM_CONFIG_SCRIPT_SHELL`).
const AGENT_REFUSED_PREFIXES: &[&str] = &["LD_", "DYLD_", "GIT_", "NPM_CONFIG_", "BASH_FUNC_"];

/// Whether an agent's run may get `name` from a `.env`.
pub fn agent_may_set(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    !AGENT_REFUSED.contains(&upper.as_str())
        && !AGENT_REFUSED_PREFIXES.iter().any(|p| upper.starts_with(p))
}

/// What an agent's `vibezeo run` gets from a `.env`: its references only,
/// never plain values, and none under a refused name.
pub fn for_agent(vars: &[EnvVar]) -> Result<Vec<EnvVar>, AgentEnvError> {
    let mut out = Vec::new();
    for v in vars {
        if malformed(&v.value) {
            return Err(AgentEnvError::Malformed(v.name.clone()));
        }
        if reference(&v.value).is_none() {
            continue;
        }
        if !agent_may_set(&v.name) {
            return Err(AgentEnvError::Refused(v.name.clone()));
        }
        out.push(v.clone());
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentEnvError {
    #[error("{0} isn't a valid vibezeo-secret:// reference")]
    Malformed(String),
    #[error("vibezeo run doesn't set {0}: it can change what a program loads or runs")]
    Refused(String),
}

/// A fingerprint of what a run gets: each name and the secret it refers to,
/// sorted. An allowance or a rule holds only while it's the same.
pub fn fingerprint(vars: &[EnvVar]) -> String {
    use sha2::{Digest, Sha256};
    let mut lines: Vec<String> = vars
        .iter()
        .map(|v| format!("{}={}", v.name, v.value))
        .collect();
    lines.sort();
    hex::encode(Sha256::digest(lines.join("\n").as_bytes()))
}

/// Why a `.env` can't be resolved.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    /// A reference to a secret that doesn't exist.
    #[error("No secret named {0}")]
    Missing(String),
    /// A variable whose value looks like a reference but isn't one.
    #[error("{0} isn't a valid vibezeo-secret:// reference")]
    Malformed(String),
}

/// The environment a run gets, references replaced by `lookup`'s values, and
/// the values to mask in its pane.
/// Values are wiped from memory when they're dropped.
pub struct Resolved {
    pub env: BTreeMap<String, zeroize::Zeroizing<String>>,
    pub secrets: Vec<zeroize::Zeroizing<String>>,
}

pub fn resolve(
    vars: &[EnvVar],
    mut lookup: impl FnMut(&str) -> Option<zeroize::Zeroizing<String>>,
) -> Result<Resolved, ResolveError> {
    let mut env = BTreeMap::new();
    let mut secrets = Vec::new();
    for v in vars {
        let value = match reference(&v.value) {
            Some(secret) => {
                let value =
                    lookup(secret).ok_or_else(|| ResolveError::Missing(secret.to_string()))?;
                secrets.push(value.clone());
                value
            }
            None if malformed(&v.value) => return Err(ResolveError::Malformed(v.name.clone())),
            None => zeroize::Zeroizing::new(v.value.clone()),
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
                    secret: Some("stripe-test".into()),
                    malformed: false,
                },
                Planned {
                    name: "PORT".into(),
                    secret: None,
                    malformed: false,
                },
            ]
        );
        let r = resolve(&vars, |n| {
            (n == "stripe-test").then(|| zeroize::Zeroizing::new("sk_test_42".to_string()))
        })
        .expect("resolved");
        assert_eq!(r.env.get("API_KEY").map(|v| v.as_str()), Some("sk_test_42"));
        assert_eq!(r.env.get("PORT").map(|v| v.as_str()), Some("3000"));
        assert_eq!(r.secrets.len(), 1);
        assert_eq!(r.secrets[0].as_str(), "sk_test_42");
    }

    #[test]
    fn f_tl_12_4_a_missing_secret_is_named() {
        let vars = parse_env("A=vibezeo-secret://gone\n");
        let err = resolve(&vars, |_| None).err().expect("missing");
        assert_eq!(err.to_string(), "No secret named gone");
    }

    #[test]
    fn f_tl_12_a_bom_is_dropped_and_quoted_values_may_span_lines() {
        let text = "\u{feff}FIRST=1\nKEY=\"-----BEGIN\nabc\n-----END\"\nSINGLE='a\nb'\nAFTER=2\n";
        let got: Vec<(String, String)> = parse_env(text)
            .into_iter()
            .map(|v| (v.name, v.value))
            .collect();
        assert_eq!(
            got,
            vec![
                ("FIRST".into(), "1".into()),
                ("KEY".into(), "-----BEGIN\nabc\n-----END".into()),
                ("SINGLE".into(), "a\nb".into()),
                ("AFTER".into(), "2".into()),
            ]
        );
    }

    #[test]
    fn f_tl_12_an_unclosed_quote_skips_only_its_variable() {
        let got: Vec<String> = parse_env("A=\"open\nB=2\nC='3'\n")
            .into_iter()
            .map(|v| format!("{}={}", v.name, v.value))
            .collect();
        assert_eq!(got, vec!["B=2".to_string(), "C=3".to_string()]);
    }

    #[test]
    fn f_tl_12_4_a_malformed_reference_is_flagged_and_refused() {
        for bad in [
            "vibezeo-secret://Stripe",
            "vibezeo-secret://",
            "VIBEZEO-SECRET://stripe",
            " vibezeo-secret://stripe",
            "vibezeo-secret://a b",
        ] {
            assert!(malformed(bad), "{bad}");
        }
        for fine in ["vibezeo-secret://stripe", "sk_live", "http://x"] {
            assert!(!malformed(fine), "{fine}");
        }
        let vars = parse_env("KEY=\"vibezeo-secret://Stripe\"\n");
        assert!(plan(&vars)[0].malformed);
        assert_eq!(
            resolve(&vars, |_| Some(zeroize::Zeroizing::new("x".into()))).err(),
            Some(ResolveError::Malformed("KEY".into()))
        );
    }

    #[test]
    fn f_tl_12_7_an_agents_run_gets_references_only_and_never_a_loader_variable() {
        let vars = parse_env(
            "API_KEY=vibezeo-secret://stripe-test\nPORT=3000\nDB_URL=vibezeo-secret://db\n",
        );
        let got: Vec<String> = for_agent(&vars)
            .expect("ok")
            .into_iter()
            .map(|v| v.name)
            .collect();
        assert_eq!(got, vec!["API_KEY", "DB_URL"]);
        for name in [
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
            "DYLD_INSERT_LIBRARIES",
            "NODE_OPTIONS",
            "NoDefaultCurrentDirectoryInExePath",
            "PYTHONSTARTUP",
            "PYTHONPATH",
            "BASH_ENV",
            "ENV",
            "PATH",
            "Path",
            "GIT_SSH_COMMAND",
            "GIT_DIR",
            "PERL5OPT",
            "RUBYOPT",
            "JAVA_TOOL_OPTIONS",
            "NPM_CONFIG_SCRIPT_SHELL",
            "PROMPT_COMMAND",
            "IFS",
        ] {
            let vars = parse_env(&format!("{name}=vibezeo-secret://x\n"));
            assert_eq!(
                for_agent(&vars),
                Err(AgentEnvError::Refused(name.to_string())),
                "{name}"
            );
        }
        // A plain value under a refused name isn't passed at all.
        let vars = parse_env("LD_PRELOAD=/tmp/evil.so\nNODE_OPTIONS=--require ./x\n");
        assert_eq!(for_agent(&vars), Ok(Vec::new()));
        let vars = parse_env("K=vibezeo-secret://Bad\n");
        assert_eq!(for_agent(&vars), Err(AgentEnvError::Malformed("K".into())));
    }

    #[test]
    fn f_tl_12_9_the_fingerprint_changes_with_any_name_or_reference_and_not_with_order() {
        let a = parse_env("A=vibezeo-secret://x\nB=vibezeo-secret://y\n");
        let b = parse_env("B=vibezeo-secret://y\nA=vibezeo-secret://x\n");
        assert_eq!(fingerprint(&a), fingerprint(&b));
        let c = parse_env("A=vibezeo-secret://x\nB=vibezeo-secret://z\n");
        let d = parse_env("A=vibezeo-secret://x\nB=vibezeo-secret://y\nC=vibezeo-secret://y\n");
        assert_ne!(fingerprint(&a), fingerprint(&c));
        assert_ne!(fingerprint(&a), fingerprint(&d));
    }
}
