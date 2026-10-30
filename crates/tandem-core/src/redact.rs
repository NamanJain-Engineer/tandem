//! The single shared redactor (F-TL-02, F-SH-11). Logs, Auto scrollback,
//! Conductor tool results, handoff briefs, Dashboard previews, notification
//! text, memory writes and diagnostics all go through it.

use regex::{Regex, RegexSet};
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
type Extractor = fn(&str, &regex::Captures<'_>) -> Option<(usize, usize)>;

#[derive(Clone, Copy)]
enum Extract {
    /// Redact the whole match.
    Whole,
    /// Redact capture group `v` when the validator accepts it.
    Value(Option<Validator>),
    /// Custom span logic.
    Custom(Extractor),
}

struct Rule {
    re: Regex,
    extract: Extract,
    /// Whether the rule opens and closes on a word boundary, which
    /// [`tolerant`] writes as ASCII's (D-3172).
    edges: (bool, bool),
}

/// Whether `pattern` opens and closes on `\b`, past a flag group.
fn edges(pattern: &str) -> (bool, bool) {
    let body = pattern.strip_prefix("(?i)").unwrap_or(pattern);
    (body.starts_with(r"\b"), body.ends_with(r"\b"))
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
        // `--token abc…`, `--api-key=…`
        (
            r#"(?i)--[A-Za-z0-9-]*(?:password|passwd|secret|token|api-?key|access-?key|auth-?key)[A-Za-z0-9-]*(?:=|[ \t]+)(?P<v>[^\s"'`]{8,})"#.to_string(),
            Extract::Value(Some(cli_value_ok)),
        ),
        // High-entropy strings near key-like words.
        value_rule(
            r"(?i)(?:key|secret|token|passw(?:or)?d|auth|credential|session|signature)[^\n]{0,24}?(?P<v>\b[A-Za-z0-9+/_-]{24,}={0,2})",
            Some(high_entropy),
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
fn assignment_span(text: &str, caps: &regex::Captures<'_>) -> Option<(usize, usize)> {
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
    if clean.len() < 8 || descriptive_key(&key) || !assignment_value_ok(&clean, quoted) {
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
/// `password_hint`, `secret_name`.
fn descriptive_key(key: &str) -> bool {
    const SUFFIXES: [&str; 22] = [
        "type", "types", "name", "kind", "pattern", "url", "uri", "id", "field", "label", "file",
        "path", "count", "length", "len", "prefix", "header", "endpoint", "hint", "policy",
        "format", "mode",
    ];
    let lower = key.to_ascii_lowercase();
    lower
        .rsplit(['_', '-'])
        .next()
        .is_some_and(|last| lower.contains(['_', '-']) && SUFFIXES.contains(&last))
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

fn high_entropy(v: &str) -> bool {
    let v = v.trim_end_matches('=');
    let all_hex = v.chars().all(|c| c.is_ascii_hexdigit());
    !all_hex && shannon_entropy(v) >= 3.9 && looks_random(v)
}

struct SecretFilter {
    /// Bloom filter over SHA-256 hashes of registered secrets; `exact` confirms.
    bits: Vec<u64>,
    exact: HashSet<[u8; 32]>,
}

const BLOOM_BITS: usize = 1 << 16;

impl SecretFilter {
    fn new() -> Self {
        Self {
            bits: vec![0; BLOOM_BITS / 64],
            exact: HashSet::new(),
        }
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
    prefilter: RegexSet,
    rules: Vec<Rule>,
    /// The rules as [`tolerant`] writes them, for
    /// [`Redactor::untrusted_spans`].
    written_prefilter: RegexSet,
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
        let prefilter = RegexSet::new(specs.iter().map(|(p, _)| unanchored(p)))
            .expect("redaction patterns compile");
        // A rule `tolerant` can't write keeps its own pattern here, and a
        // test pins that none does.
        let written_specs: Vec<(String, Extract)> = specs
            .iter()
            .map(|(pattern, extract)| {
                let written = tolerant(pattern).filter(|t| Regex::new(t).is_ok());
                (written.unwrap_or_else(|| pattern.clone()), *extract)
            })
            .collect();
        let written_prefilter = RegexSet::new(written_specs.iter().map(|(p, _)| unanchored(p)))
            .expect("written patterns compile");
        let written_rules = written_specs
            .into_iter()
            .zip(&specs)
            .map(|((pattern, extract), (plain, _))| Rule {
                re: Regex::new(&pattern).expect("written pattern compiles"),
                extract,
                edges: edges(plain),
            })
            .collect();
        let rules = specs
            .into_iter()
            .map(|(pattern, extract)| Rule {
                re: Regex::new(&pattern).expect("redaction pattern compiles"),
                extract,
                edges: edges(&pattern),
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
        self.register_secret_hash(Sha256::digest(secret.as_bytes()).into());
    }

    /// Byte spans to redact, sorted and merged.
    pub(crate) fn spans(&self, s: &str) -> Vec<(usize, usize)> {
        let hits: Vec<usize> = self.prefilter.matches(s).into_iter().collect();
        self.spans_by(s, &hits, &self.rules, false)
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
    /// anywhere else, `read` says the same. Where its rule opens or closes
    /// on a word boundary, that edge has a mark or Unicode's boundary there,
    /// so ASCII's doesn't add a word that a letter outside ASCII runs into.
    /// Validators read a value without its marks, and a value with a mark
    /// in it is also read cut at that mark, where the text as written ended
    /// it.
    ///
    /// Each rule is one scan of each form it reads. One prefilter scan of
    /// `marked` serves `read` too, since a rule that matches `read` matches
    /// `marked` as `tolerant` writes it.
    pub(crate) fn untrusted_spans(&self, read: &str, written: &str, marked: &str) -> [Spans; 3] {
        let hits: Vec<usize> = self.written_prefilter.matches(marked).into_iter().collect();
        let plain: Vec<usize> = self
            .prefilter
            .matches(written)
            .into_iter()
            .filter(|&idx| {
                self.rules
                    .get(idx)
                    .is_some_and(|r| matches!(r.extract, Extract::Whole | Extract::Value(None)))
            })
            .collect();
        [
            self.spans_by(read, &hits, &self.rules, false),
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
        // Whether a match's edges are word boundaries where its rule wants
        // them, with a mark or as Unicode reads them.
        let edged = |rule: &Rule, a: usize, b: usize| {
            if !marked {
                return true;
            }
            let opens = !rule.edges.0
                || a == 0
                || at_mark(a - 1)
                || !s[..a].chars().next_back().is_some_and(word);
            let closes = !rule.edges.1
                || b == s.len()
                || at_mark(b)
                || (b > a && at_mark(b - 1))
                || !s[b..].chars().next().is_some_and(word);
            opens && closes
        };
        let mut spans = Vec::new();
        let mut push = |span: Option<(usize, usize)>| {
            if let Some((start, end)) = span.filter(|(start, end)| end > start) {
                spans.push((start, end));
            }
        };
        let value = |caps: &regex::Captures<'_>, validator: Option<Validator>| {
            caps.name("v")
                .filter(|v| validator.is_none_or(|ok| ok(&unmarked(v.as_str()))))
                .map(|v| (v.start(), v.end()))
        };
        for rule in hits.iter().filter_map(|&idx| rules.get(idx)) {
            let span = |s: &str, caps: &regex::Captures<'_>| match rule.extract {
                Extract::Value(validator) => value(caps, validator),
                Extract::Custom(extract) => extract(s, caps),
                Extract::Whole => caps.get(0).map(|m| (m.start(), m.end())),
            };
            match rule.extract {
                // A whole match needs no groups: its bounds come from the
                // DFA, without the slower engine that captures run.
                Extract::Whole => {
                    for m in rule.re.find_iter(s) {
                        let (a, b) = (m.start(), m.end());
                        push((touches(a, b) && edged(rule, a, b)).then_some((a, b)));
                    }
                }
                // As written, only a match that counts runs the captures,
                // over itself: a long one elsewhere costs a DFA scan, not
                // the slower engine's.
                Extract::Value(_) | Extract::Custom(_) if marked => {
                    for m in rule.re.find_iter(s) {
                        let (a, b) = (m.start(), m.end());
                        if a == b || !touches(a, b) || !edged(rule, a, b) {
                            continue;
                        }
                        let Some(caps) = rule.re.captures_at(s, a) else {
                            continue;
                        };
                        push(span(s, &caps));
                        // Where the text as written ended the value: at its
                        // first mark (the review of 8e1d58af).
                        let cut = ["v", "vq", "vu"]
                            .iter()
                            .find_map(|g| caps.name(g))
                            .and_then(|v| v.as_str().find(MARK).map(|at| v.start() + at));
                        if let Some(cut) = cut {
                            // The same match, cut: one that starts later
                            // hasn't had its edges read.
                            let head = &s[..cut];
                            let same = rule
                                .re
                                .captures_at(head, a)
                                .filter(|c| c.get(0).is_some_and(|m| m.start() == a));
                            if let Some(caps) = same {
                                push(span(head, &caps));
                            }
                        }
                    }
                }
                _ => {
                    for caps in rule.re.captures_iter(s) {
                        push(span(s, &caps));
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
