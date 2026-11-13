//! The single shared redactor (F-TL-02, F-SH-11). Logs, Auto scrollback,
//! Conductor tool results, handoff briefs, Dashboard previews, notification
//! text, memory writes and diagnostics all go through it.

mod engine;

use engine::{Caps, Pattern, Set};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::{OnceLock, RwLock};

pub const REDACTED: &str = "[REDACTED]";

/// Bytes held back by [`StreamRedactor`] so a secret split across chunks is
/// still seen whole.
pub const WINDOW: usize = 512;

/// A held-back buffer never grows past this; beyond it the buffer is flushed
/// through the redactor (an unterminated private-key block is redacted to the
/// end, so nothing leaks).
const MAX_PENDING: usize = 64 * 1024;

type Validator = fn(&str) -> bool;
/// Byte spans, start and end.
type Spans = Vec<(usize, usize)>;
type Extractor = fn(&str, &Caps<'_>) -> Option<(usize, usize)>;

#[derive(Clone, Copy)]
enum Extract {
    /// Redact the whole match.
    Whole,
    /// Redact capture group `v` when the validator accepts it.
    Value(Option<Validator>),
    /// Custom span logic.
    Custom(Extractor),
    /// Redact capture group `v` when [`high_entropy`] accepts it. A value
    /// that what's removable cut inside one camelCase name ([`one_name`])
    /// isn't also read cut (D-2573).
    Entropy,
}

struct Rule {
    re: Pattern,
    extract: Extract,
    /// Whether the rule opens on a word boundary, which [`tolerant`]
    /// writes as ASCII's (D-3172).
    opens_on_boundary: bool,
}

impl Rule {
    /// Whether the rule is a token's, with no validator: its matches are
    /// searched as [`Pattern::tokens`] says.
    fn token(&self) -> bool {
        matches!(self.extract, Extract::Whole | Extract::Value(None))
    }
}

/// Whether `pattern` opens on `\b`, past a flag group.
fn opens_on_boundary(pattern: &str) -> bool {
    pattern
        .strip_prefix("(?i)")
        .unwrap_or(pattern)
        .starts_with(r"\b")
}

/// `pattern` with ASCII's word boundaries, so its lazy DFA reads text
/// outside ASCII rather than quitting to the slow engine at its first
/// letter. [`Redactor::spans_by`] holds a match's start to Unicode's
/// boundary, or a letter of a script written without spaces (D-2572).
fn ascii_bounds(pattern: &str) -> String {
    pattern.replace(r"\b", r"(?-u:\b)")
}

/// Whether a rule that opens on a word boundary may open after `c`:
/// where Unicode's boundary holds, or after a letter of a script written
/// without spaces between words, as Chinese, Japanese and Thai are, so
/// `使用ghp_…推送` holds a token and `Gdańsk-…` doesn't (D-2572).
fn opens_after(c: char) -> bool {
    static UNSPACED: OnceLock<Regex> = OnceLock::new();
    !word(c)
        || UNSPACED
            .get_or_init(|| {
                Regex::new(
                    r"\A[\p{scx=Han}\p{scx=Hiragana}\p{scx=Katakana}\p{scx=Hangul}\p{scx=Bopomofo}\p{scx=Thai}\p{scx=Lao}\p{scx=Khmer}\p{scx=Myanmar}]\z",
                )
                .expect("script pattern compiles")
            })
            .is_match(c.encode_utf8(&mut [0; 4]))
}

/// Whether `c` is a word character as Unicode's `\b` reads it.
fn word(c: char) -> bool {
    static WORD: OnceLock<Regex> = OnceLock::new();
    WORD.get_or_init(|| Regex::new(r"\A\w\z").expect("word pattern compiles"))
        .is_match(c.encode_utf8(&mut [0; 4]))
}

/// Where the removal cut, in the copies of the readable text that
/// [`Redactor::untrusted_spans`] reads: one byte for what's removable
/// (`untrusted::removable`), which this is itself, so it's in no text as
/// read (D-3172).
pub(crate) const MARK: char = '\u{1}';

/// `s` without [`MARK`]s, as a validator reads a value.
fn unmarked(s: &str) -> Cow<'_, str> {
    if s.contains(MARK) {
        Cow::Owned(s.replace(MARK, ""))
    } else {
        Cow::Borrowed(s)
    }
}

/// `pattern` without its word boundaries, for a prefilter: it matches all
/// that `pattern` does, and more, which the rule's own pattern then sorts
/// out. A set of patterns with Unicode `\b`s falls back from its DFA to a
/// slow engine on any text that isn't ASCII (1 s for 64 KB in a debug
/// build, against 14 ms), and a set without them doesn't (D-3172).
fn unanchored(pattern: &str) -> String {
    pattern.replace(r"(?-u:\b)", "").replace(r"\b", "")
}

/// `pattern` with a [`MARK`] allowed after every character it matches, for
/// the copy where the removal cut, or `None` for syntax this doesn't know.
/// The rules use literals, escapes, classes, groups (`(?:`, `(?P<v>`, a flag
/// group) with alternatives, and quantifiers. A word boundary is ASCII's,
/// so a mark is a boundary too, and the DFA keeps it on any text (D-3172).
fn tolerant(pattern: &str) -> Option<String> {
    let atom = |a: &str| format!("(?:{a}\\x01?)");
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len() * 3);
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                let next = *chars.get(i + 1)?;
                if matches!(next, 'b' | 'B') {
                    // ASCII's: where a rule's edge is an ASCII letter or
                    // digit, as every token's is, it holds wherever
                    // Unicode's would, and next to a mark or a letter
                    // outside ASCII too.
                    out.push_str(&format!("(?-u:\\{next})"));
                    // Inside a pattern, the boundary can fall before a mark
                    // that nothing else may take: a key, then a mark, then
                    // its `=`.
                    if !matches!(out.as_str(), "(?-u:\\b)" | "(?i)(?-u:\\b)") {
                        out.push_str("\\x01?");
                    }
                } else if matches!(next, 'A' | 'z') {
                    out.push('\\');
                    out.push(next);
                } else {
                    out.push_str(&atom(&format!("\\{next}")));
                }
                i += 2;
            }
            '[' => {
                let start = i;
                i += 1;
                if chars.get(i) == Some(&'^') {
                    i += 1;
                }
                if chars.get(i) == Some(&']') {
                    i += 1;
                }
                while *chars.get(i)? != ']' {
                    if chars[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
                out.push_str(&atom(&chars[start..i].iter().collect::<String>()));
            }
            '(' => {
                let rest: String = chars[i..].iter().collect();
                let header = if rest.starts_with("(?:") {
                    "(?:".to_string()
                } else if rest.starts_with("(?P<") {
                    rest[..=rest.find('>')?].to_string()
                } else if rest.starts_with("(?") {
                    let close = rest.find(')')?;
                    if !rest[2..close].chars().all(|f| f.is_ascii_alphabetic()) {
                        return None;
                    }
                    rest[..=close].to_string()
                } else {
                    "(".to_string()
                };
                i += header.chars().count();
                out.push_str(&header);
            }
            '{' => {
                let close = i + chars[i..].iter().position(|&c| c == '}')?;
                out.extend(&chars[i..=close]);
                i = close + 1;
            }
            c @ (')' | '|' | '*' | '+' | '?' | '^' | '$') => {
                out.push(c);
                i += 1;
            }
            '.' => {
                out.push_str(&atom("."));
                i += 1;
            }
            c => {
                out.push_str(&atom(&regex::escape(&c.to_string())));
                i += 1;
            }
        }
    }
    Some(out)
}

type Spec = (String, Extract);

fn rule(pattern: &str) -> Spec {
    (pattern.to_string(), Extract::Whole)
}

fn value_rule(pattern: &str, validator: Option<Validator>) -> Spec {
    (pattern.to_string(), Extract::Value(validator))
}

/// Words that mark the value after them as a credential.
const KEY_WORDS: &str = r"[A-Za-z0-9_]*(?:password|passwd|passphrase|secret|token|api_?key|apikey|access_?key|private_?key|client_?secret|auth_?key|credentials?)[A-Za-z0-9_]*";

fn rule_specs() -> Vec<Spec> {
    vec![
        // Private key blocks, complete or still streaming in.
        rule(r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----[\s\S]*?-----END [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----"),
        rule(r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----(?:\r?\n(?:[A-Za-z0-9+/=]+|[A-Za-z-]+: [^\r\n]*))*(?:\r?\n)?"),
        // JWTs.
        rule(r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}"),
        // Anthropic, OpenAI (project, service-account, admin), OpenRouter.
        rule(r"\bsk-ant-[A-Za-z0-9]{2,8}-[A-Za-z0-9_-]{20,}"),
        rule(r"\bsk-(?:proj-|svcacct-|admin-|or-v1-)?[A-Za-z0-9_-]{20,}"),
        // GitHub.
        rule(r"\bgh[pousr]_[A-Za-z0-9]{30,}"),
        rule(r"\bgithub_pat_[A-Za-z0-9_]{30,}"),
        // Stripe secret and restricted keys, webhook secrets.
        rule(r"\b(?:sk|rk)_(?:live|test)_[A-Za-z0-9]{16,}"),
        rule(r"\bwhsec_[A-Za-z0-9+/=]{16,}"),
        // AWS access key ids; secret keys are caught by the assignment rule.
        rule(r"\b(?:AKIA|ASIA|ABIA|ACCA)[0-9A-Z]{16}\b"),
        // Slack tokens and webhooks.
        rule(r"\bxox[abposre]-[A-Za-z0-9-]{10,}"),
        rule(r"\bxapp-[0-9]+-[A-Za-z0-9-]{10,}"),
        rule(r"https://hooks\.slack\.com/services/[A-Za-z0-9_/-]{20,}"),
        // Google API keys, OAuth client secrets and access tokens.
        rule(r"\bAIza[0-9A-Za-z_-]{35}"),
        rule(r"\bGOCSPX-[A-Za-z0-9_-]{20,}"),
        rule(r"\bya29\.[0-9A-Za-z_-]{20,}"),
        // Other common vendor formats.
        rule(r"\bnpm_[A-Za-z0-9]{36}\b"),
        rule(r"\bhf_[A-Za-z0-9]{30,}\b"),
        rule(r"\bxai-[A-Za-z0-9]{30,}\b"),
        rule(r"\bgsk_[A-Za-z0-9]{40,}\b"),
        rule(r"\bSG\.[A-Za-z0-9_-]{16,}\.[A-Za-z0-9_-]{16,}"),
        rule(r"\blin_(?:api|oauth)_[A-Za-z0-9]{32,}\b"),
        rule(r"\bsbp_[a-f0-9]{40}\b"),
        rule(r"\b(?:ntn|secret)_[A-Za-z0-9]{40,}\b"),
        rule(r"\bphx_[A-Za-z0-9]{30,}\b"),
        rule(r"\bglpat-[A-Za-z0-9_-]{20,}"),
        // Bearer tokens in headers.
        value_rule(r"(?i)\bbearer\s+(?P<v>[A-Za-z0-9._~+/=-]{20,})", None),
        // Passwords embedded in URLs: scheme://user:password@host
        value_rule(r"[A-Za-z][A-Za-z0-9+.-]*://[^/\s:@]+:(?P<v>[^/\s@]{3,})@", Some(url_password_ok)),
        // `password = "…"`, `token: …`, `export API_KEY=…`, `?access_token=…`
        (
            format!(
                r#"(?i)\b(?P<k>{KEY_WORDS})\b["']?[ \t]*(?:=|:|=>)[ \t]*(?:(?P<q>["'`])(?P<vq>[^"'`\s]{{8,}})|(?P<vu>[^\s"'`,;)]{{8,}}))"#
            ),
            Extract::Custom(assignment_span),
        ),
        // `?token_type=bearer&access_token=…`: each query parameter on its
        // own. The rule above reads a value up to a space, so a parameter it
        // refused (descriptive, a time, too short) took the rest of the query
        // with it, and a short token after it went unread (the review of
        // 66e0dcd9, Nit 8).
        (
            format!(r#"(?i)[?&](?P<k>{KEY_WORDS})=(?P<vu>[^\s"'`,;)&#]{{8,}})"#),
            Extract::Custom(assignment_span),
        ),
        // `--token abc…`, `--api-key=…`
        (
            r#"(?i)--[A-Za-z0-9-]*(?:password|passwd|secret|token|api-?key|access-?key|auth-?key)[A-Za-z0-9-]*(?:=|[ \t]+)(?P<v>[^\s"'`]{8,})"#.to_string(),
            Extract::Value(Some(cli_value_ok)),
        ),
        // High-entropy strings near key-like words.
        (
            r"(?i)(?:key|secret|token|passw(?:or)?d|auth|credential|session|signature)[^\n]{0,24}?(?P<v>\b[A-Za-z0-9+/_-]{24,}={0,2})".to_string(),
            Extract::Entropy,
        ),
    ]
}

fn url_password_ok(v: &str) -> bool {
    // `${PASSWORD}` and similar placeholders carry no secret.
    !(v.starts_with('$') || v.starts_with('{') || v.starts_with('<') || v.starts_with('%'))
}

fn is_identifier_path(v: &str) -> bool {
    let v = v.trim_end_matches(['(', ')', '!', '?', ']', '[', '}', '{']);
    let mut parts = v.split(['.', ':']).filter(|p| !p.is_empty()).peekable();
    if parts.peek().is_none() {
        return false;
    }
    parts.all(|p| {
        let mut chars = p.chars();
        chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    })
}

/// The value span of a `key = value` match, or `None` when the value is code
/// rather than a literal (`token = getToken()`).
fn assignment_span(text: &str, caps: &Caps<'_>) -> Option<(usize, usize)> {
    let key = caps.name("k")?;
    let quoted = caps.name("vq").is_some();
    let v = caps.name("vq").or_else(|| caps.name("vu"))?;
    let mut value = v.as_str();
    if !quoted {
        // `?access_token=abc&page=2`: a query parameter ends at `&`. A mark
        // before the key is where the removal cut, not the query's.
        let in_query = text[..key.start()]
            .trim_end_matches(MARK)
            .ends_with(['?', '&']);
        if in_query {
            if let Some(i) = value.find('&') {
                value = &value[..i];
            }
        }
        // `token:abc@github.com/…`: credentials in a URL end at `@host`.
        if let Some(i) = value.find('@') {
            if value[i + 1..].contains('.') {
                value = &value[..i];
            }
        }
    }
    // Read without the marks the copy as written has (D-3172).
    let (clean, key) = (unmarked(value), unmarked(key.as_str()));
    if clean.len() < 8
        || descriptive_key(&key)
        || (time_key(&key) && time_like(&clean))
        || !assignment_value_ok(&clean, quoted)
    {
        return None;
    }
    Some((v.start(), v.start() + value.len()))
}

fn cli_value_ok(v: &str) -> bool {
    !v.starts_with('-') && assignment_value_ok(v, false)
}

/// `getToken(`, `self.secret[`, `new Foo(`: the value is an expression.
fn starts_like_code(v: &str) -> bool {
    let v = v.strip_prefix("new ").unwrap_or(v);
    let end = v
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '.'))
        .unwrap_or(v.len());
    end > 0 && is_identifier_path(&v[..end]) && matches!(v[end..].chars().next(), Some('(' | '['))
}

/// Keys that describe a credential rather than hold one: `token_type`,
/// `password_hint`, `secret_name`. A credential's times (`token_expires_at`,
/// `tokenExpiresAt`) are read by their key and value together ([`time_key`],
/// [`time_like`]): as key suffixes, `at` and `expiry` let any value
/// through, and a camelCase key's last word let `secretId: "<a UUID>"`
/// through, a Vault SecretID (the review of 0e20d5b7, SF-2). `secret_id`
/// is one too: an id that is the secret (the review of e00668ee, S1).
fn descriptive_key(key: &str) -> bool {
    const SUFFIXES: [&str; 22] = [
        "type", "types", "name", "kind", "pattern", "url", "uri", "id", "field", "label", "file",
        "path", "count", "length", "len", "prefix", "header", "endpoint", "hint", "policy",
        "format", "mode",
    ];
    let lower = key.to_ascii_lowercase();
    let mut words = lower.rsplit(['_', '-']);
    let (Some(last), before) = (words.next(), words.next()) else {
        return false;
    };
    lower.contains(['_', '-'])
        && SUFFIXES.contains(&last)
        && !(last == "id" && before == Some("secret"))
}

/// Whether a key's last word names a time: `token_expires_at`,
/// `tokenExpiresAt`, `SESSION_TOKEN_EXPIRY`, `secret_rotated`. Only under
/// such a key is a time ([`time_like`]) the value, not the credential: under
/// every key, `password=2024-01-01` and `api_key=1730311200.5` went through
/// in clear (the review of e00668ee, S1).
fn time_key(key: &str) -> bool {
    const TIME_WORDS: [&str; 18] = [
        "at",
        "expires",
        "expire",
        "expiry",
        "expiration",
        "exp",
        "iat",
        "nbf",
        "created",
        "updated",
        "issued",
        "rotated",
        "refreshed",
        "since",
        "until",
        "date",
        "time",
        "timestamp",
    ];
    let key = key.trim_end_matches(['_', '-']);
    let word = key.rsplit(['_', '-']).next().unwrap_or(key);
    // camelCase: the last word starts at the last capital, unless the word
    // is all capitals (`SESSION_TOKEN_AT`).
    let last = if word.bytes().any(|b| b.is_ascii_lowercase()) {
        word.rfind(|c: char| c.is_ascii_uppercase())
            .map_or(word, |i| &word[i..])
    } else {
        word
    };
    TIME_WORDS.contains(&last.to_ascii_lowercase().as_str())
}

/// A date or a time as RFC 3339 and ISO 8601 write them (`2025-10-30`,
/// `2025-10-30T18:00:00.000Z`, `2025-10-30 18:00+05:30`), or a number with
/// a fraction (`1730311200.5`): what a credential's time holds, never the
/// credential (D-3168).
fn time_like(v: &str) -> bool {
    // Read as bytes: a cut into `str` inside a character would panic.
    let digits = |s: &[u8]| !s.is_empty() && s.iter().all(u8::is_ascii_digit);
    if let Some((whole, fraction)) = v.split_once('.') {
        if digits(whole.as_bytes()) && digits(fraction.as_bytes()) {
            return true;
        }
    }
    let b = v.as_bytes();
    let date = b.len() >= 10
        && digits(&b[..4])
        && b[4] == b'-'
        && digits(&b[5..7])
        && b[7] == b'-'
        && digits(&b[8..10]);
    if !date {
        return false;
    }
    let Some((&sep, t)) = b[10..].split_first() else {
        return true;
    };
    if !matches!(sep, b'T' | b't' | b' ')
        || t.len() < 5
        || !digits(&t[..2])
        || t[2] != b':'
        || !digits(&t[3..5])
    {
        return false;
    }
    let mut rest = &t[5..];
    if let Some((b':', seconds)) = rest.split_first() {
        if seconds.len() < 2 || !digits(&seconds[..2]) {
            return false;
        }
        rest = &seconds[2..];
        if let Some((b'.', fraction)) = rest.split_first() {
            let n = fraction.iter().take_while(|c| c.is_ascii_digit()).count();
            if n == 0 {
                return false;
            }
            rest = &fraction[n..];
        }
    }
    match rest {
        [] | [b'Z' | b'z'] => true,
        [b'+' | b'-', zone @ ..] => match zone.len() {
            2 | 4 => digits(zone),
            5 => digits(&zone[..2]) && zone[2] == b':' && digits(&zone[3..]),
            _ => false,
        },
        _ => false,
    }
}

fn assignment_value_ok(v: &str, quoted: bool) -> bool {
    let lower = v.to_ascii_lowercase();
    const NOT_SECRETS: [&str; 9] = [
        "null",
        "none",
        "undefined",
        "true",
        "false",
        "redacted",
        "[redacted]",
        "required",
        "optional",
    ];
    if NOT_SECRETS.contains(&lower.as_str()) || lower.contains("[redacted]") {
        return false;
    }
    if v.starts_with('$') || v.starts_with('{') || v.starts_with('<') || v.starts_with('%') {
        return false;
    }
    if v.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let has_digit_or_symbol = v.chars().any(|c| !c.is_ascii_alphabetic());
    if quoted {
        // 'content-type', 'addr-spec': lowercase words are labels, not secrets.
        let wordish = v
            .chars()
            .all(|c| c.is_ascii_lowercase() || matches!(c, '-' | '_' | '.' | '/'));
        return v.len() >= 20 || (has_digit_or_symbol && !wordish);
    }
    if starts_like_code(v) {
        return false;
    }
    // Unquoted identifiers are usually code (`getToken()`, `self.secret`,
    // `req.headers.authorization`), unless they read like a password:
    // one segment mixing letters with two or more digits, or random.
    if is_identifier_path(v) {
        let digits = v.chars().filter(char::is_ascii_digit).count();
        let single = !v.contains(['.', ':', '(', '[']);
        return looks_random(v) || (single && digits >= 2);
    }
    has_digit_or_symbol || v.len() >= 20
}

fn shannon_entropy(s: &str) -> f64 {
    let mut counts = [0usize; 256];
    for b in s.bytes() {
        counts[b as usize] += 1;
    }
    let len = s.len() as f64;
    counts
        .iter()
        .filter(|c| **c > 0)
        .map(|c| {
            let p = *c as f64 / len;
            -p * p.log2()
        })
        .sum()
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Class {
    Lower,
    Upper,
    Digit,
    Other,
}

fn class(c: char) -> Class {
    if c.is_ascii_lowercase() {
        Class::Lower
    } else if c.is_ascii_uppercase() {
        Class::Upper
    } else if c.is_ascii_digit() {
        Class::Digit
    } else {
        Class::Other
    }
}

/// Random tokens switch character class often; identifiers and words do not.
fn looks_random(v: &str) -> bool {
    let chars: Vec<char> = v.chars().collect();
    if chars.len() < 16 {
        return false;
    }
    let transitions = chars
        .windows(2)
        .filter(|w| class(w[0]) != class(w[1]))
        .count();
    let has_lower = chars.iter().any(|c| c.is_ascii_lowercase());
    let has_upper = chars.iter().any(|c| c.is_ascii_uppercase());
    let has_digit = chars.iter().any(|c| c.is_ascii_digit());
    has_digit && (has_lower || has_upper) && transitions * 10 >= chars.len() * 3
}

/// Whether `v` reads as one camelCase name: a letter and lower-case
/// letters, then words of a capital and lower-case letters or runs of
/// digits, perhaps ending on one capital, with a vowel in every run of
/// three or more lower-case letters (`handleSocks5AuthenticationNoAuth`).
/// Of random letters and digits that pass [`high_entropy`], about one in
/// 20,000 of 24 reads so, and none of 40 in 600,000 (D-2573).
fn one_name(v: &str) -> bool {
    let b = v.as_bytes();
    let lower = |i: usize| b[i..].iter().take_while(|c| c.is_ascii_lowercase()).count();
    if b.len() < 2 || !b[0].is_ascii_alphabetic() || lower(1) == 0 {
        return false;
    }
    let mut i = 1 + lower(1);
    while i < b.len() {
        if b[i].is_ascii_uppercase() {
            let n = lower(i + 1);
            if n == 0 && i + 1 < b.len() {
                return false;
            }
            i += 1 + n;
        } else if b[i].is_ascii_digit() {
            i += b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        } else {
            return false;
        }
    }
    v.split(|c: char| !c.is_ascii_lowercase())
        .filter(|w| w.len() >= 3)
        .all(|w| w.contains(['a', 'e', 'i', 'o', 'u', 'y']))
}

fn high_entropy(v: &str) -> bool {
    let v = v.trim_end_matches('=');
    let all_hex = v.chars().all(|c| c.is_ascii_hexdigit());
    !all_hex && shannon_entropy(v) >= 3.9 && looks_random(v)
}

struct SecretFilter {
    /// Bloom filter over SHA-256 hashes of registered secrets; `exact` confirms.
    bits: Vec<u64>,
    exact: HashSet<[u8; 32]>,
    /// Each secret registered as text, by its length and [`fingerprint`],
    /// and those lengths: for one that what's removable both split and
    /// set apart ([`SecretFilter::split_and_set_apart`]).
    prints: HashSet<(usize, u64)>,
    lengths: Vec<usize>,
}

/// The base of [`fingerprint`]'s polynomial, odd (FNV's prime).
const PRINT_BASE: u64 = 0x0000_0100_0000_01b3;

/// A polynomial hash of `bytes`, which a prefix table gives for any slice
/// in constant time. Only a hit is hashed with SHA-256, which confirms.
fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0, |h: u64, &b| {
        h.wrapping_mul(PRINT_BASE).wrapping_add(u64::from(b) + 1)
    })
}

const BLOOM_BITS: usize = 1 << 16;

impl SecretFilter {
    fn new() -> Self {
        Self {
            bits: vec![0; BLOOM_BITS / 64],
            exact: HashSet::new(),
            prints: HashSet::new(),
            lengths: Vec::new(),
        }
    }

    fn print(&mut self, secret: &[u8]) {
        if secret.is_empty() {
            return;
        }
        self.prints.insert((secret.len(), fingerprint(secret)));
        if let Err(at) = self.lengths.binary_search(&secret.len()) {
            self.lengths.insert(at, secret.len());
        }
    }

    /// The spans in `marked` (the copy with a [`MARK`] for each run of
    /// what's removable) of a registered secret that a mark both splits
    /// and sets apart: in a run of token bytes and marks with a mark in
    /// it, the secret read without its marks from a place a piece can
    /// start (the run's start, after a mark or a separator) to one it can
    /// end (the run's end, before a mark or a separator). Each start is
    /// read once a registered length, from a prefix table of
    /// [`fingerprint`]s, so the scan is in line with the text (D-3172, amended).
    fn split_and_set_apart(&self, marked: &[u8]) -> Spans {
        let mut spans = Vec::new();
        if self.prints.is_empty() {
            return spans;
        }
        // `PRINT_BASE` to each length's power, for a slice's fingerprint.
        let powers: Vec<u64> = self
            .lengths
            .iter()
            .map(|&len| (0..len).fold(1_u64, |p, _| p.wrapping_mul(PRINT_BASE)))
            .collect();
        let mark = MARK as u8;
        let in_run = |b: u8| b == mark || b.is_ascii_alphanumeric() || b"+/_.~-".contains(&b);
        let separator = |b: u8| matches!(b, b'.' | b'/' | b'~');
        let mut i = 0;
        while i < marked.len() {
            let start = i;
            while i < marked.len() && in_run(marked[i]) {
                i += 1;
            }
            if i == start {
                i += 1;
                continue;
            }
            let run = &marked[start..i];
            if !run.contains(&mark) {
                continue;
            }
            // The run without its marks: each byte's place in `marked`,
            // whether a mark came just before it, and the prefix table.
            let mut places = Vec::with_capacity(run.len());
            let mut after_mark = Vec::with_capacity(run.len() + 1);
            let mut table = Vec::with_capacity(run.len() + 1);
            table.push(0_u64);
            let mut pending = false;
            for (j, &b) in run.iter().enumerate() {
                if b == mark {
                    pending = true;
                    continue;
                }
                places.push(start + j);
                after_mark.push(std::mem::take(&mut pending));
                let h = table.last().copied().unwrap_or(0);
                table.push(h.wrapping_mul(PRINT_BASE).wrapping_add(u64::from(b) + 1));
            }
            after_mark.push(pending);
            let n = places.len();
            let bytes: Vec<u8> = places.iter().map(|&at| marked[at]).collect();
            let opens = |k: usize| k == 0 || after_mark[k] || separator(bytes[k - 1]);
            let closes = |e: usize| e == n || after_mark[e] || separator(bytes[e]);
            for k in (0..n).filter(|&k| opens(k)) {
                for (&len, &power) in self.lengths.iter().zip(&powers) {
                    let e = k + len;
                    if e > n {
                        break;
                    }
                    if !closes(e) {
                        continue;
                    }
                    let print = table[e].wrapping_sub(table[k].wrapping_mul(power));
                    if !self.prints.contains(&(len, print)) {
                        continue;
                    }
                    let hash: [u8; 32] = Sha256::digest(&bytes[k..e]).into();
                    if self.contains(&hash) {
                        spans.push((places[k], places[e - 1] + 1));
                    }
                }
            }
        }
        spans
    }

    fn slots(hash: &[u8; 32]) -> [usize; 3] {
        let take = |i: usize| {
            (u32::from_le_bytes([hash[i], hash[i + 1], hash[i + 2], hash[i + 3]]) as usize)
                % BLOOM_BITS
        };
        [take(0), take(4), take(8)]
    }

    fn insert(&mut self, hash: [u8; 32]) {
        for slot in Self::slots(&hash) {
            self.bits[slot / 64] |= 1 << (slot % 64);
        }
        self.exact.insert(hash);
    }

    fn contains(&self, hash: &[u8; 32]) -> bool {
        Self::slots(hash)
            .iter()
            .all(|slot| self.bits[slot / 64] & (1 << (slot % 64)) != 0)
            && self.exact.contains(hash)
    }
}

pub struct Redactor {
    prefilter: Set,
    rules: Vec<Rule>,
    /// The rules as [`tolerant`] writes them, for
    /// [`Redactor::untrusted_spans`].
    written_prefilter: Set,
    written_rules: Vec<Rule>,
    token_re: Regex,
    secrets: RwLock<SecretFilter>,
    home: Option<String>,
}

impl Default for Redactor {
    fn default() -> Self {
        Self::new()
    }
}

impl Redactor {
    pub fn new() -> Self {
        let specs = rule_specs();
        let prefilter = Set::new(&specs.iter().map(|(p, _)| unanchored(p)).collect::<Vec<_>>())
            .expect("redaction patterns compile");
        // A rule `tolerant` can't write keeps its own pattern here, and a
        // test pins that none does.
        let written: Vec<(String, Pattern)> = specs
            .iter()
            .map(|(pattern, _)| {
                tolerant(pattern)
                    .and_then(|t| Pattern::new(&t).ok().map(|re| (t, re)))
                    .unwrap_or_else(|| {
                        let re = Pattern::new(pattern).expect("redaction pattern compiles");
                        (pattern.clone(), re)
                    })
            })
            .collect();
        let written_prefilter = Set::new(
            &written
                .iter()
                .map(|(p, _)| unanchored(p))
                .collect::<Vec<_>>(),
        )
        .expect("written patterns compile");
        let written_rules = written
            .into_iter()
            .zip(&specs)
            .map(|((_, re), (plain, extract))| Rule {
                re,
                extract: *extract,
                opens_on_boundary: opens_on_boundary(plain),
            })
            .collect();
        let rules = specs
            .into_iter()
            .map(|(pattern, extract)| Rule {
                re: Pattern::new(&ascii_bounds(&pattern)).expect("redaction pattern compiles"),
                extract,
                opens_on_boundary: opens_on_boundary(&pattern),
            })
            .collect();
        let token_re = Regex::new(r"[A-Za-z0-9+/_.~-]{8,}").expect("token pattern compiles");
        Self {
            prefilter,
            rules,
            written_prefilter,
            written_rules,
            token_re,
            secrets: RwLock::new(SecretFilter::new()),
            home: crate::paths::user_home()
                .map(|h| {
                    h.to_string_lossy()
                        .trim_end_matches(['/', '\\'])
                        .to_string()
                })
                .filter(|h| h.len() > 1),
        }
    }

    pub fn global() -> &'static Redactor {
        static GLOBAL: OnceLock<Redactor> = OnceLock::new();
        GLOBAL.get_or_init(Redactor::new)
    }

    /// Registers a secret the user owns (e.g. a plugin credential) by its
    /// SHA-256 hash. Exact occurrences are redacted from then on.
    pub fn register_secret_hash(&self, sha256: [u8; 32]) {
        let mut filter = self.secrets.write().unwrap_or_else(|e| e.into_inner());
        filter.insert(sha256);
    }

    pub fn register_secret(&self, secret: &str) {
        let mut filter = self.secrets.write().unwrap_or_else(|e| e.into_inner());
        filter.insert(Sha256::digest(secret.as_bytes()).into());
        filter.print(secret.as_bytes());
    }

    /// Byte spans to redact, sorted and merged.
    pub(crate) fn spans(&self, s: &str) -> Vec<(usize, usize)> {
        let hits = self.prefilter.matches(s);
        self.read_spans(s, &hits)
    }

    /// The plain rules' spans in `s`, and in its copy with full-width
    /// letters, digits and token punctuation as ASCII ([`folded`]), mapped
    /// back to `s`: NFKC makes `ｇｈｐ＿…` a token, and a model reads it as
    /// one (D-3170, amended).
    fn read_spans(&self, s: &str, hits: &[usize]) -> Spans {
        let mut spans = self.spans_by(s, hits, &self.rules, false);
        if let Some((copy, at)) = folded(s) {
            let hits = self.prefilter.matches(&copy);
            spans.extend(
                self.spans_by(&copy, &hits, &self.rules, false)
                    .into_iter()
                    .map(|(a, b)| (at[a], at[b])),
            );
            spans = merge(spans);
        }
        spans
    }

    /// The spans `untrusted::redacted` hides, in each of three forms of the
    /// text, and in that form's offsets (D-3172):
    /// - `read`, the readable text: every rule as it is.
    /// - `written`, the same with one byte where each removable character
    ///   was, a space for whitespace and a [`MARK`] for the rest: the rules
    ///   with no validator (the tokens and `Bearer`) as they are. So what
    ///   the text as written kept apart is found apart: a token a decoy
    ///   runs into, or `Bearer` and its value across a line separator.
    /// - `marked`, `read` with one [`MARK`] for each run of what's
    ///   removable: every rule as [`tolerant`] writes it, stepping over a
    ///   mark and reading one as a word boundary. So a credential the
    ///   removal split, set apart from a letter, or both, is found.
    ///
    /// In `marked`, a match counts only with a mark in it or next to it:
    /// anywhere else, `read` says the same. Where its rule opens on a word
    /// boundary, its start has a mark or Unicode's boundary there, so
    /// ASCII's doesn't make a token of a word that a letter outside ASCII
    /// runs into. Its end isn't held to that: a token that runs into a
    /// letter outside ASCII is still a token.
    /// Validators read a value without its marks, and a value with a mark
    /// in it is also read cut at that mark, where the text as written ended
    /// it.
    ///
    /// Each rule is one scan of each form it reads. One prefilter scan of
    /// `marked` serves `read` too, since a rule that matches `read` matches
    /// `marked` as `tolerant` writes it.
    pub(crate) fn untrusted_spans(&self, read: &str, written: &str, marked: &str) -> [Spans; 3] {
        let hits = self.written_prefilter.matches(marked);
        let plain: Vec<usize> = self
            .prefilter
            .matches(written)
            .into_iter()
            .filter(|&idx| self.rules.get(idx).is_some_and(Rule::token))
            .collect();
        [
            self.read_spans(read, &hits),
            self.spans_by(written, &plain, &self.rules, false),
            self.spans_by(marked, &hits, &self.written_rules, true),
        ]
    }

    fn spans_by(
        &self,
        s: &str,
        hits: &[usize],
        rules: &[Rule],
        marked: bool,
    ) -> Vec<(usize, usize)> {
        let bytes = s.as_bytes();
        let at_mark = |i: usize| bytes.get(i) == Some(&(MARK as u8));
        // Whether a match touches a mark, when the text has them.
        let touches = |a: usize, b: usize| {
            !marked
                || bytes[a..b].contains(&(MARK as u8))
                || (a > 0 && at_mark(a - 1))
                || at_mark(b)
        };
        // Whether a match opens on a word boundary where its rule wants one:
        // after a mark, where Unicode's holds, or after a letter of a script
        // written without spaces ([`opens_after`]). The rules' own are
        // ASCII's, which also hold after `ń`. Only its start: a token that
        // runs into `é` or `中` is still hidden, as 7f61c86a hid it (the
        // review of d2b219f1; D-2572).
        let edged = |rule: &Rule, a: usize| {
            !rule.opens_on_boundary
                || a == 0
                || (marked && at_mark(a - 1))
                || s[..a].chars().next_back().is_some_and(opens_after)
        };
        let mut spans = Vec::new();
        let mut push = |span: Option<(usize, usize)>| {
            if let Some((start, end)) = span.filter(|(start, end)| end > start) {
                spans.push((start, end));
            }
        };
        let value = |caps: &Caps<'_>, validator: Option<Validator>| {
            caps.name("v")
                .filter(|v| validator.is_none_or(|ok| ok(&unmarked(v.as_str()))))
                .map(|v| (v.start(), v.end()))
        };
        for rule in hits.iter().filter_map(|&idx| rules.get(idx)) {
            let span = |s: &str, caps: &Caps<'_>| match rule.extract {
                Extract::Value(validator) => value(caps, validator),
                Extract::Entropy => value(caps, Some(high_entropy)),
                Extract::Custom(extract) => extract(s, caps),
                Extract::Whole => caps.get(0).map(|m| (m.start(), m.end())),
            };
            // A token's search after a match starts at its last word, so a
            // token that a decoy ran into is read whole (D-2574).
            let found = || -> Vec<_> {
                if rule.token() {
                    rule.re.tokens(s).collect()
                } else {
                    rule.re.find_iter(s).collect()
                }
            };
            match rule.extract {
                // A whole match needs no groups: its bounds come from the
                // DFA, without the slower engine that captures run.
                Extract::Whole => {
                    for m in found() {
                        let (a, b) = (m.start(), m.end());
                        push((touches(a, b) && edged(rule, a)).then_some((a, b)));
                    }
                }
                // As written, only a match that counts runs the captures,
                // over itself: a long one elsewhere costs a DFA scan, not
                // the slower engine's. So does a token's value as read.
                Extract::Value(_) | Extract::Custom(_) | Extract::Entropy
                    if marked || rule.token() =>
                {
                    for m in found() {
                        let (a, b) = (m.start(), m.end());
                        if a == b || !touches(a, b) || !edged(rule, a) {
                            continue;
                        }
                        let Some(caps) = rule.re.captures_at(s, a) else {
                            continue;
                        };
                        let whole = span(s, &caps);
                        push(whole);
                        // Where the text as written ended the value: at its
                        // first mark (the review of 8e1d58af). Not inside
                        // one camelCase name, which the mark only split
                        // (D-2573).
                        let one = matches!(rule.extract, Extract::Entropy)
                            && caps
                                .name("v")
                                .is_some_and(|v| one_name(&unmarked(v.as_str())));
                        let cut = ["v", "vq", "vu"]
                            .iter()
                            .find_map(|g| caps.name(g))
                            .and_then(|v| v.as_str().find(MARK).map(|at| v.start() + at))
                            .filter(|_| !one);
                        if let Some(cut) = cut {
                            // The same match, cut: one that starts later
                            // hasn't had its edges read.
                            let head = &s[..cut];
                            let same = rule
                                .re
                                .captures_at(head, a)
                                .filter(|c| c.get(0).is_some_and(|m| m.start() == a));
                            // It ends no later than the whole value's span: a
                            // cut can drop what that span's end was read from,
                            // such as the host after a URL's `@` (the review
                            // of d2b219f1).
                            if let Some(caps) = same {
                                push(span(head, &caps).map(|(start, end)| {
                                    (start, whole.map_or(end, |(_, limit)| end.min(limit)))
                                }));
                            }
                        }
                    }
                }
                _ => {
                    for caps in rule.re.captures_iter(s) {
                        let opens = caps.get(0).is_some_and(|m| edged(rule, m.start()));
                        push(span(s, &caps).filter(|_| opens));
                    }
                }
            }
        }
        {
            let filter = self.secrets.read().unwrap_or_else(|e| e.into_inner());
            if !filter.exact.is_empty() {
                let mut check = |start: usize, end: usize| {
                    let hash: [u8; 32] = Sha256::digest(&s.as_bytes()[start..end]).into();
                    if touches(start, end) && filter.contains(&hash) {
                        spans.push((start, end));
                    }
                };
                for m in self.token_re.find_iter(s) {
                    check(m.start(), m.end());
                    // Also check the pieces between separators (`a.b`, `x/y`).
                    let mut piece_start = m.start();
                    for (i, ch) in m.as_str().char_indices() {
                        if matches!(ch, '.' | '/' | '~') {
                            if m.start() + i > piece_start {
                                check(piece_start, m.start() + i);
                            }
                            piece_start = m.start() + i + 1;
                        }
                    }
                    if piece_start > m.start() && piece_start < m.end() {
                        check(piece_start, m.end());
                    }
                }
                if marked {
                    spans.extend(filter.split_and_set_apart(bytes));
                }
            }
        }
        merge(spans)
    }

    /// Replaces every secret in `s` with `[REDACTED]`.
    pub fn redact<'a>(&self, s: &'a str) -> Cow<'a, str> {
        let spans = self.spans(s);
        if spans.is_empty() {
            return Cow::Borrowed(s);
        }
        Cow::Owned(apply(s, &spans))
    }

    pub fn stream(&self) -> StreamRedactor<'_> {
        StreamRedactor {
            redactor: self,
            pending: String::new(),
        }
    }

    /// Replaces the user's home directory with `~`.
    pub fn redact_home(&self, s: &str) -> String {
        match &self.home {
            Some(home) => s.replace(home.as_str(), "~"),
            None => s.to_string(),
        }
    }
}

/// `s` with its full-width letters, digits and token punctuation
/// (`＿－．／＋＝～`) as ASCII, and for each byte of that copy and its end,
/// the place in `s` it came from; `None` when there are none. The
/// punctuation that full-width text uses in prose (`：，（）`) stays as it
/// is, so a Chinese sentence doesn't turn into an assignment (D-3170, amended).
fn folded(s: &str) -> Option<(String, Vec<usize>)> {
    let ascii = |c: char| {
        let folded = char::from_u32((c as u32).checked_sub(0xFEE0)?)?;
        (('\u{FF01}'..='\u{FF5E}').contains(&c)
            && (folded.is_ascii_alphanumeric() || "_-./+=~".contains(folded)))
        .then_some(folded)
    };
    if !s.chars().any(|c| ascii(c).is_some()) {
        return None;
    }
    let mut copy = String::with_capacity(s.len());
    let mut at = Vec::with_capacity(s.len() + 1);
    for (i, c) in s.char_indices() {
        let c = ascii(c).unwrap_or(c);
        at.extend((0..c.len_utf8()).map(|_| i));
        copy.push(c);
    }
    at.push(s.len());
    Some((copy, at))
}

pub(crate) fn merge(mut spans: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    spans.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for (start, end) in spans {
        match out.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => out.push((start, end)),
        }
    }
    out
}

pub(crate) fn apply(s: &str, spans: &[(usize, usize)]) -> String {
    let mut out = String::with_capacity(s.len());
    let mut cursor = 0;
    for (start, end) in spans {
        out.push_str(&s[cursor..*start]);
        out.push_str(REDACTED);
        cursor = *end;
    }
    out.push_str(&s[cursor..]);
    out
}

fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// Redacts a stream of text chunks, holding back the last [`WINDOW`] bytes
/// (and any match that reaches into them) until more text arrives.
pub struct StreamRedactor<'r> {
    redactor: &'r Redactor,
    pending: String,
}

impl StreamRedactor<'_> {
    pub fn push(&mut self, chunk: &str) -> String {
        self.pending.push_str(chunk);
        let spans = self.redactor.spans(&self.pending);
        let mut cut = floor_char_boundary(&self.pending, self.pending.len().saturating_sub(WINDOW));
        if self.pending.len() <= MAX_PENDING {
            // Never cut through a match, and never emit the start of a match
            // that reaches the held-back window: it may still be growing.
            for (start, end) in &spans {
                if *end > cut && *start < cut {
                    cut = *start;
                }
            }
        } else {
            cut = self.pending.len();
        }
        if cut == 0 {
            return String::new();
        }
        let head_spans: Vec<(usize, usize)> = spans
            .iter()
            .filter(|(_, end)| *end <= cut)
            .copied()
            .collect();
        let head = apply(&self.pending[..cut], &head_spans);
        self.pending.drain(..cut);
        head
    }

    pub fn finish(self) -> String {
        self.redactor.redact(&self.pending).into_owned()
    }
}

/// Convenience wrapper over the global redactor.
pub fn redact(s: &str) -> String {
    Redactor::global().redact(s).into_owned()
}

/// Test text, the same every run.
#[cfg(test)]
pub(crate) mod sample {
    const ALNUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

    fn step(seed: &mut u64) -> u64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    }

    /// `n` letters and digits from `seed`.
    pub(crate) fn alnum(seed: &mut u64, n: usize) -> String {
        (0..n)
            .map(|_| ALNUM[(step(seed) % ALNUM.len() as u64) as usize] as char)
            .collect()
    }

    /// `head` and then up to `gaps` letters or digits, again and again to
    /// `len` bytes. Where `head` sits inside a rule's counted run, as `AIza`
    /// does in `AIza[0-9A-Za-z_-]{35}`, every `head` starts a count, and a
    /// lazy DFA builds a state for most bytes (the review of e788fc6f). The
    /// gap takes a step of its own: drawn from the last letter's, a gap of 0
    /// stalled the seed into `headhead…`, which a lazy DFA reads in a few
    /// states.
    pub(crate) fn crafted(head: &str, gaps: u64, len: usize) -> String {
        assert!(!head.is_empty(), "a run needs a head");
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut out = String::with_capacity(len + head.len() + gaps as usize);
        while out.len() < len {
            out.push_str(head);
            let gap = (step(&mut seed) % (gaps + 1)) as usize;
            out.push_str(&alnum(&mut seed, gap));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRETS: &str = include_str!("../tests/redaction/secrets.txt");
    const BENIGN: &str = include_str!("../tests/redaction/benign.txt");

    /// Corpus lines mark the secret with «…». `{{}}` splits strings so that
    /// repository secret scanners don't flag the fake fixtures.
    fn corpus() -> Vec<(String, String, String)> {
        SECRETS
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .map(|l| {
                let l = l.replace("{{}}", "").replace("\\n", "\n");
                let (prefix, rest) = l.split_once('«').expect("line has «");
                let (secret, suffix) = rest.split_once('»').expect("line has »");
                (prefix.to_string(), secret.to_string(), suffix.to_string())
            })
            .collect()
    }

    #[test]
    fn f_tk_08_1_every_rule_steps_over_a_mark() {
        // Each rule, as `tolerant` writes it, finds in F-TL-02's corpus
        // with a mark after every character what the rule finds there, at
        // the same place once the marks go. A rule that misses would leave
        // a credential the removal split. Every rule has a sample (D-3172;
        // the review of 8e1d58af, Nit 4).
        let r = Redactor::new();
        let mut samples = vec![0; r.rules.len()];
        for (prefix, secret, suffix) in corpus() {
            let line = format!("{prefix}{secret}{suffix}");
            let marked: String = line.chars().flat_map(|c| [c, MARK]).collect();
            let marks: Vec<usize> = marked.match_indices(MARK).map(|(at, _)| at).collect();
            let to_line = |b: usize| b - marks.partition_point(|&at| at < b);
            for (i, (plain, written)) in r.rules.iter().zip(&r.written_rules).enumerate() {
                let Some(m) = plain.re.find(&line) else {
                    continue;
                };
                samples[i] += 1;
                let covered = written
                    .re
                    .find_iter(&marked)
                    .map(|w| (to_line(w.start()), to_line(w.end())))
                    .any(|(a, b)| a <= m.start() && b >= m.end());
                assert!(covered, "rule {i} {:?} in {line:?}", plain.re.as_str());
            }
        }
        let none: Vec<&str> = r
            .rules
            .iter()
            .zip(&samples)
            .filter(|(_, n)| **n == 0)
            .map(|(rule, _)| rule.re.as_str())
            .collect();
        assert!(none.is_empty(), "rules with no sample: {none:#?}");
    }

    #[test]
    fn f_tk_08_1_the_engine_reads_as_the_regex_crate_does() {
        // The redactor's engine (D-877) finds what the `regex` crate finds:
        // each rule's matches, their groups and what `captures_at` reads at
        // each match, and the rules each prefilter names. In F-TL-02's
        // corpus as it is, with a mark after every character, between
        // letters outside ASCII (where a lazy DFA quits on `\b`), and in
        // text crafted to make a lazy DFA give up.
        let r = Redactor::new();
        let mut texts: Vec<String> = Vec::new();
        for (prefix, secret, suffix) in corpus() {
            let line = format!("{prefix}{secret}{suffix}");
            texts.push(line.chars().flat_map(|c| [c, MARK]).collect());
            texts.push(format!("é{line}中"));
            texts.push(line);
        }
        texts.push(sample::crafted("AIza", 33, 32 * 1024));
        texts.push(sample::crafted("-AIza", 33, 16 * 1024));
        texts.push(sample::crafted("key", 19, 16 * 1024));
        let head = |text: &str| text.chars().take(60).collect::<String>();
        for (set, rules) in [
            (&r.prefilter, &r.rules),
            (&r.written_prefilter, &r.written_rules),
        ] {
            let patterns: Vec<String> = rules
                .iter()
                .map(|rule| unanchored(rule.re.as_str()))
                .collect();
            let reference = regex::RegexSet::new(&patterns).expect("the regex crate builds them");
            for text in &texts {
                let expected: Vec<usize> = reference.matches(text).into_iter().collect();
                assert_eq!(set.matches(text), expected, "in {:?}", head(text));
            }
        }
        for rule in r.rules.iter().chain(&r.written_rules) {
            let reference = Regex::new(rule.re.as_str()).expect("the regex crate builds it");
            let groups = reference.captures_len();
            let ours = |caps: Option<Caps<'_>>| {
                caps.map(|c| {
                    (0..groups)
                        .map(|i| c.get(i).map(|m| (m.start(), m.end())))
                        .collect::<Vec<_>>()
                })
            };
            let theirs = |caps: Option<regex::Captures<'_>>| {
                caps.map(|c| {
                    (0..groups)
                        .map(|i| c.get(i).map(|m| (m.start(), m.end())))
                        .collect::<Vec<_>>()
                })
            };
            for text in &texts {
                let found: Vec<_> = rule
                    .re
                    .find_iter(text)
                    .map(|m| (m.start(), m.end()))
                    .collect();
                let expected: Vec<_> = reference
                    .find_iter(text)
                    .map(|m| (m.start(), m.end()))
                    .collect();
                assert_eq!(found, expected, "{} in {:?}", rule.re.as_str(), head(text));
                let found: Vec<_> = rule.re.captures_iter(text).map(|c| ours(Some(c))).collect();
                let expected: Vec<_> = reference
                    .captures_iter(text)
                    .map(|c| theirs(Some(c)))
                    .collect();
                assert_eq!(found, expected, "{} in {:?}", rule.re.as_str(), head(text));
                for (a, _) in reference.find_iter(text).map(|m| (m.start(), m.end())) {
                    assert_eq!(
                        ours(rule.re.captures_at(text, a)),
                        theirs(reference.captures_at(text, a)),
                        "{} at {a} in {:?}",
                        rule.re.as_str(),
                        head(text)
                    );
                }
            }
        }
    }

    #[test]
    fn f_tl_02_1_a_credentials_time_is_read_by_its_value_not_its_key() {
        // As key suffixes, `at` and `expiry` let any value through, and a
        // camelCase key's last word let a Vault SecretID through (the
        // review of 0e20d5b7, SF-2).
        for key in ["token_type", "password-hint", "secret_name"] {
            assert!(descriptive_key(key), "{key}");
        }
        for key in [
            "token_expires_at",
            "SESSION_TOKEN_EXPIRY",
            "tokenExpiresAt",
            "secretId",
            "authToken",
            "token_at_rest",
        ] {
            assert!(!descriptive_key(key), "{key}");
        }
        for v in [
            "2025-10-30",
            "2025-10-30T18:00:00Z",
            "2025-10-30T18:00:00.000Z",
            "2025-10-30t18:00z",
            "2025-10-30 18:00:00+05:30",
            "2025-10-30T18:00:00-0800",
            "2025-10-30T18:00+01",
            "1730311200.5",
        ] {
            assert!(time_like(v), "{v}");
        }
        for v in [
            "2025-10-30T",
            "2025-10-30Tab:cd",
            "2025-10-30T18:00:00.Z",
            "2025-10-30T18:00:00+1",
            "2025-10-30x",
            "2025-1O-30",
            "Zx81kQ02pLm4wR",
            "6a174c20-f6de-4c0e-9d3a-6c2b8f1e0a42",
            "1.2.3",
            "é025-10-30",
        ] {
            assert!(!time_like(v), "{v}");
        }
    }

    #[test]
    fn f_tl_02_1_a_time_passes_only_under_a_key_that_names_one() {
        // Under every key, a date or a number with a fraction went through
        // in clear, `password=2024-01-01` among them (the review of
        // e00668ee, S1).
        for key in [
            "token_expires_at",
            "tokenExpiresAt",
            "SESSION_TOKEN_AT",
            "SESSION_TOKEN_EXPIRY",
            "refresh_token_expires",
            "secret_rotated",
            "token_created_",
        ] {
            assert!(time_key(key), "{key}");
        }
        for key in [
            "password",
            "DB_PASSWORD",
            "api_key",
            "secret",
            "secretId",
            "token_at_rest",
            "tokenExpiresAT",
        ] {
            assert!(!time_key(key), "{key}");
        }
        // A secret's id is the secret (Vault's SecretID); a token's isn't.
        assert!(!descriptive_key("secret_id"));
        assert!(!descriptive_key("VAULT_SECRET_ID"));
        assert!(descriptive_key("token_id"));
        let r = Redactor::new();
        for line in [
            "password=2024-01-01",
            "DB_PASSWORD=\"1987-05-12\"",
            "password: 31415926.53",
            "api_key=1730311200.5",
            "secret = 2025-10-30T18:00:00Z",
            "mysql --password 2024-01-01",
        ] {
            assert!(r.redact(line).contains(REDACTED), "{line}");
        }
        for line in [
            "token_expires_at: 2025-10-30T18:00:00Z",
            "SESSION_TOKEN_AT=2025-10-30T18:00:00Z",
            "secret_rotated=1730311200.5",
        ] {
            assert_eq!(r.redact(line), line);
        }
    }

    #[test]
    fn f_tl_02_1_corpus_redacted_no_false_positives() {
        let redactor = Redactor::new();
        let cases = corpus();
        assert!(cases.len() >= 60, "corpus has {} secrets", cases.len());
        let mut failures = Vec::new();
        for (prefix, secret, suffix) in &cases {
            let input = format!("{prefix}{secret}{suffix}");
            let out = redactor.redact(&input);
            if out != format!("{prefix}{REDACTED}{suffix}") {
                failures.push(format!("{input:?}\n   -> {out:?}"));
            }
        }
        assert!(
            failures.is_empty(),
            "not redacted exactly:\n{}",
            failures.join("\n")
        );
        let benign: Vec<&str> = BENIGN.lines().collect();
        assert!(
            benign.len() >= 1000,
            "benign corpus has {} lines",
            benign.len()
        );
        let flagged: Vec<&&str> = benign
            .iter()
            .filter(|line| redactor.redact(line) != **line)
            .collect();
        assert!(flagged.is_empty(), "false positives: {flagged:#?}");
    }

    /// What `untrusted::redacted` hides of `text` on redactor `r`, built as
    /// `untrusted::credential_spans` builds it: the three forms, their spans
    /// mapped to the readable text, merged.
    fn hidden(r: &Redactor, text: &str) -> String {
        let removable = crate::untrusted::removable;
        let (mut read, mut written, mut marked) = (String::new(), String::new(), String::new());
        let (mut extra, mut marks, mut cut) = (Vec::new(), Vec::new(), false);
        for c in text.chars() {
            if removable(c) {
                extra.push(written.len());
                written.push(if c.is_whitespace() { ' ' } else { MARK });
                cut = true;
                continue;
            }
            if std::mem::take(&mut cut) {
                marks.push(marked.len());
                marked.push(MARK);
            }
            read.push(c);
            written.push(c);
            marked.push(c);
        }
        if cut {
            marks.push(marked.len());
            marked.push(MARK);
        }
        let [mut spans, as_written, as_marked] = r.untrusted_spans(&read, &written, &marked);
        let to_read = |put: &[usize], b: usize| b - put.partition_point(|&at| at < b);
        spans.extend(
            as_written
                .into_iter()
                .map(|(a, b)| (to_read(&extra, a), to_read(&extra, b))),
        );
        spans.extend(
            as_marked
                .into_iter()
                .map(|(a, b)| (to_read(&marks, a), to_read(&marks, b))),
        );
        spans.retain(|(a, b)| b > a);
        apply(&read, &merge(spans))
    }

    #[test]
    fn f_tl_02_1_a_decoy_glued_to_a_token_hides_the_token_too() {
        // A token rule's match that ran into the next token's start ended
        // inside it, and the next search began after it, so the rest of the
        // real token came back: `eyJaaaaaaaa.bbbbbbbb.<real JWT>` gave the
        // JWT's payload and signature, in the plain redactor and with
        // what's removable elsewhere (D-3172, the review of 8e1d58af, Nit 5).
        let jwt = format!(
            "eyJhbGciOiJIUzI1NiJ9.{}.dozjgNryP4J3jVmNHl0w5N_XgL0",
            "eyJzdWIiOiIxMjM0NTY3ODkwIn0"
        );
        let cases = [
            (format!("eyJaaaaaaaa.bbbbbbbb.{jwt}"), "eyJzdWIiOiIx"),
            (
                format!("eyJaaaaaaaa.bbbbbbbb.cccccccc-{jwt}"),
                "eyJzdWIiOiIx",
            ),
            (
                format!(
                    "Bearer {}-Bearer ZYXWVUTSRQPONMLKJIHGFEDCBA98765",
                    "a".repeat(20)
                ),
                "ZYXWVUTSRQ",
            ),
            (
                format!(
                    "whsec_{}/whsec_wDUHQFuHO5sJDkaJjgbebjy3LvGzrkaL",
                    "a".repeat(16)
                ),
                "wDUHQFuHO5",
            ),
            (
                format!("ya29.{}-ya29.oeWfpdVFvnri-bQ54T79LVtAAdu", "a".repeat(20)),
                "oeWfpdVFvn",
            ),
            (
                format!(
                    "SG.{}.{}-SG.y62WQgQU_9Bvk-EnVc6Pmy.Cq9LahRD0Lz7b-F51WG22Q",
                    "a".repeat(16),
                    "b".repeat(16)
                ),
                "y62WQgQU",
            ),
            (
                format!(
                    "https://hooks.slack.com/services/{}/https://hooks.slack.com/services/{}",
                    "a".repeat(20),
                    "THO45DKX1HT/BKUFG8O3EL0/fSubJvo0jPMQONWuB8E7abwA"
                ),
                "fSubJvo0jP",
            ),
        ];
        let r = Redactor::new();
        for (text, real) in &cases {
            let plain = r.redact(text);
            assert!(!plain.contains(real), "{text:?}: {plain:?}");
            let with = format!("{text} and \u{200b}");
            let safe = hidden(&r, &with);
            assert!(!safe.contains(real), "{with:?}: {safe:?}");
        }
    }

    #[test]
    fn f_tl_02_1_a_token_run_into_cjk_text_is_found_and_a_word_outside_ascii_is_not() {
        // Unicode's word boundary holds none between Chinese text and a
        // token run into it, so `使用ghp_…推送` stayed whole. ASCII's would
        // also hold inside `Gdańsk-…`. A match opens where Unicode's does,
        // or after a letter of a script written without spaces (D-2572).
        let tokens = [
            format!("ghp_{}", "0123456789abcdefghijABCDEFGHIJ012345"),
            format!("AKIA{}", "IABN48B5MPWWYTUG"),
            format!("sk-{}", "proj-0123456789abcdefghijABCDEFGHIJ"),
        ];
        let r = Redactor::new();
        for token in &tokens {
            for (before, after) in [
                ("使用", "推送"),
                ("トークンは", "です"),
                ("토큰", "을"),
                ("ใช้", "ส่ง"),
            ] {
                let text = format!("{before}{token}{after}");
                let want = format!("{before}{REDACTED}{after}");
                assert_eq!(r.redact(&text), want, "{text:?}");
                assert_eq!(hidden(&r, &format!("{text}\u{200b}")), want, "{text:?}");
            }
        }
        for text in [
            "Gda\u{144}sk-Oliwa-Przymorze-Uniwersytet",
            "Mi\u{144}sk-Mazowiecki-Dworzec-Kolejowy",
            "Gda\u{144}sk-Oliwa-Przymorze-Uniwersytet \u{1F468}\u{200D}\u{1F469}",
        ] {
            assert_eq!(r.redact(text), text);
            assert_eq!(hidden(&r, text), crate::untrusted::readable(text));
        }
    }

    #[test]
    fn f_tl_02_1_what_is_removable_inside_a_camel_case_name_makes_no_secret() {
        // A value with a mark in it is also read cut at the mark, where the
        // text as written ended it. Inside one camelCase name, the cut's
        // start passed `high_entropy` (`handleSocks5AuthenticationNoAu`,
        // 96 of 1,888 inputs; D-3172). Joined, it's one name, so it isn't
        // cut (D-2573). A random value that what's removable ends still is.
        let line = "authResult = yield this.handleSocks5AuthenticationNoAuthHandshakeResponse(this.receiveBuffer.get(2));";
        let start = line.find("handle").expect("the name");
        let end = line.find("(this.receive").expect("the name's end");
        let r = Redactor::new();
        let removables = [
            '\u{200b}',
            '\u{200c}',
            '\u{200d}',
            '\u{2060}',
            '\u{feff}',
            '\u{00ad}',
            '\u{2028}',
            '\u{2029}',
            '\u{000b}',
            '\u{000c}',
            '\u{0085}',
            '\u{034f}',
            '\u{fe0f}',
            '\u{180e}',
            '\u{e0001}',
            '\u{202e}',
        ];
        let mut flagged = Vec::new();
        for c in removables {
            for at in start + 1..end {
                let text = format!("{}{c}{}", &line[..at], &line[at..]);
                if hidden(&r, &text) != line {
                    flagged.push(format!("{c:?} at {at}"));
                }
            }
        }
        assert!(flagged.is_empty(), "{} flagged: {flagged:?}", flagged.len());
        for c in removables {
            let text = format!("auth Xk9qL2mP7vR4tY8wZ1nB5cD3fG6hJ0{c}{}", "a".repeat(40));
            assert!(!hidden(&r, &text).contains("Xk9qL2mP7v"), "{text:?}");
            let text = format!("auth Xk9qL2mP7vR4tY8wZ1nB5cD3fG6hJ0{c}Response");
            assert!(!hidden(&r, &text).contains("Xk9qL2mP7v"), "{text:?}");
        }
    }

    #[test]
    fn f_tl_02_1_a_registered_secret_what_is_removable_splits_and_sets_apart_is_hidden() {
        // The registered-secret scan found one that what's removable split,
        // or set apart from a letter, not both (D-3172).
        let r = Redactor::new();
        r.register_secret("plainvalue-0001");
        r.register_secret("Zq81kLm02pNx");
        for text in [
            "abc\u{200b}plain\u{200b}value-0001 now",
            "abc\u{200b}plainvalue\u{ad}-0001\u{200b}xyz",
            "x\u{2060}plain\u{2060}val\u{2060}ue-0001",
            "token:\u{200b}Zq81\u{feff}kLm02pNx.",
            "ab\u{200b}Zq81k\u{200b}Lm02pNx\u{200b}cd and plainvalue-0001",
        ] {
            let safe = hidden(&r, text);
            assert!(
                !safe.contains("lue-0001") && !safe.contains("Lm02pNx"),
                "{text:?}: {safe:?}"
            );
        }
        // Glued to a letter as written, it's not set apart.
        assert_eq!(
            hidden(&r, "abc\u{200b}plain\u{200b}value-0001xyz"),
            "abcplainvalue-0001xyz"
        );
    }

    #[test]
    fn f_tl_02_1_the_registered_secret_scan_takes_time_in_line_with_the_text() {
        // Each place a secret could start is read once a length, from
        // prefix fingerprints, and only a fingerprint's hit is hashed.
        let r = Redactor::new();
        r.register_secret("plainvalue-0001");
        r.register_secret(&"Zq81kLm02pNx".repeat(8));
        let shapes = |size: usize| {
            vec![
                "a\u{200b}".repeat(size / 4),
                "plain\u{200b}value-\u{200b}".repeat(size / 16),
                format!("x\u{200b}{}", "ab-".repeat(size / 3)),
            ]
        };
        let time = |text: &str| {
            let started = std::time::Instant::now();
            let _ = std::hint::black_box(hidden(&r, text));
            started.elapsed()
        };
        for text in shapes(1 << 20) {
            let took = time(&text);
            assert!(took < std::time::Duration::from_secs(4), "{took:?}");
        }
        // Best of three each side, timed again once before failing, as the
        // other growth checks are: under the suite's load one pair read
        // 87 ms, then 516 ms, where alone it reads about twice.
        let linear = |(t1, t2): (std::time::Duration, std::time::Duration)| {
            t2 < t1 * 3 + std::time::Duration::from_millis(30)
        };
        for (a, b) in shapes(1 << 18).iter().zip(&shapes(1 << 19)) {
            let best = |t: &str| (0..3).map(|_| time(t)).min().expect("timed");
            let mut times = (best(a), best(b));
            if !linear(times) {
                times = (best(a), best(b));
            }
            let (t1, t2) = times;
            assert!(
                linear(times),
                "{} bytes took {t1:?}, twice that {t2:?}",
                a.len()
            );
        }
    }

    #[test]
    fn f_tl_02_1_a_full_width_token_is_found() {
        // NFKC makes `ｇｈｐ＿…` a token, and a model reads it as one (the
        // review of a106fbf1, Nit 5; D-3170).
        let full = |s: &str| -> String {
            s.chars()
                .map(|c| match c {
                    '!'..='~' => char::from_u32(c as u32 + 0xFEE0).unwrap_or(c),
                    c => c,
                })
                .collect()
        };
        let r = Redactor::new();
        for token in [
            format!("ghp_{}", "0123456789abcdefghijABCDEFGHIJ012345"),
            format!("AKIA{}", "IABN48B5MPWWYTUG"),
            "password=hunter2hunter2".to_string(),
        ] {
            let all = full(&token);
            let text = format!("pushed with {all} today");
            let out = r.redact(&text);
            assert!(out.contains(REDACTED), "{text:?}: {out:?}");
            assert!(!out.contains(&full("23456789")), "{out:?}");
            assert!(!hidden(&r, &format!("{text}\u{200b}")).contains(&full("23456789")));
            // Partly full width.
            let half = format!("{}{}", &token[..4], full(&token[4..]));
            assert!(!r.redact(&half).contains(&full("23456789")), "{half:?}");
        }
        assert_eq!(
            r.redact(&format!(
                "x ｇｈｐ＿{} y",
                full("0123456789abcdefghijABCDEFGHIJ012345")
            )),
            format!("x {REDACTED} y")
        );
    }

    #[test]
    fn f_tl_02_2_split_secret_every_offset() {
        let redactor = Redactor::new();
        for (prefix, secret, suffix) in corpus() {
            let input = format!("log line before {prefix}{secret}{suffix} and after\n");
            for offset in 0..=input.len() {
                if !input.is_char_boundary(offset) {
                    continue;
                }
                let mut stream = redactor.stream();
                let mut out = stream.push(&input[..offset]);
                out.push_str(&stream.push(&input[offset..]));
                out.push_str(&stream.finish());
                assert!(
                    !out.contains(&secret),
                    "secret leaked when split at {offset}: {input:?}"
                );
                assert!(out.contains(REDACTED));
            }
        }
    }

    #[test]
    fn stream_output_matches_batch_for_long_text() {
        let redactor = Redactor::new();
        let mut text = String::new();
        for i in 0..400 {
            text.push_str(&format!("line {i}: compiling crate number {i}\n"));
            if i % 97 == 0 {
                text.push_str("token used: ghp_0123456789abcdefghijABCDEFGHIJ0123456789\n");
            }
        }
        let mut stream = redactor.stream();
        let mut out = String::new();
        for chunk in text.as_bytes().chunks(37) {
            out.push_str(&stream.push(std::str::from_utf8(chunk).expect("ascii")));
        }
        out.push_str(&stream.finish());
        assert_eq!(out, redactor.redact(&text));
    }

    #[test]
    fn registered_secrets_are_redacted_exactly() {
        let redactor = Redactor::new();
        redactor.register_secret("plainvalue-0001");
        let out = redactor.redact("calling with plainvalue-0001 now");
        assert_eq!(out, "calling with [REDACTED] now");
    }

    #[test]
    fn home_is_replaced_with_tilde() {
        let redactor = Redactor::new();
        if let Some(home) = crate::paths::user_home() {
            let line = format!("opened {}/code/app", home.display());
            assert_eq!(redactor.redact_home(&line), "opened ~/code/app");
        }
    }
}
