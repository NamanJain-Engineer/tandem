use regex::Regex;
use std::sync::OnceLock;

const REDACTED: &str = "[REDACTED]";
const HOLDBACK: usize = 512;

fn patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----",
            r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b",
            r"\bsk-ant-[A-Za-z0-9_-]{16,}\b",
            r"\bsk-(?:proj-)?[A-Za-z0-9]{20,}\b",
            r"\bgh[pousr]_[A-Za-z0-9]{20,}\b",
            r"\bgithub_pat_[A-Za-z0-9_]{20,}\b",
            r"\bghu_[A-Za-z0-9]{20,}\b",
            r"\b(?:sk|rk|pk)_(?:live|test)_[A-Za-z0-9]{16,}\b",
            r"\bwhsec_[A-Za-z0-9]{16,}\b",
            r"\bAKIA[0-9A-Z]{16}\b",
            r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b",
            r"\bAIza[0-9A-Za-z\-_]{20,}\b",
            r"(?i)\b(?:password|token|secret|api_key)\s*=\s*[^\s]{16,}",
            r"(?i)\b(?:api[_-]?key|secret|password|authorization)\b[^A-Za-z0-9]{0,8}[A-Za-z0-9_\-+/=]{24,}",
        ]
        .into_iter()
        .map(|pattern| Regex::new(pattern).expect("redaction pattern compiles"))
        .collect()
    })
}

pub fn redact(input: &str) -> String {
    let mut output = input.to_string();
    for pattern in patterns() {
        output = pattern.replace_all(&output, REDACTED).into_owned();
    }
    output
}

/// Holds the last 512 bytes so a secret split across chunks is still removed.
pub struct StreamRedactor {
    hold: Vec<u8>,
    emitted: Vec<u8>,
}

impl StreamRedactor {
    pub fn new() -> Self {
        Self {
            hold: Vec::new(),
            emitted: Vec::new(),
        }
    }

    pub fn push(&mut self, chunk: &[u8]) {
        self.hold.extend_from_slice(chunk);
        self.flush_ready();
    }

    fn flush_ready(&mut self) {
        if self.hold.len() <= HOLDBACK {
            return;
        }
        let safe_end = self.hold.len() - HOLDBACK;
        let text = String::from_utf8_lossy(&self.hold);
        let mut cut = safe_end;
        for pattern in patterns() {
            for found in pattern.find_iter(&text) {
                if found.end() > safe_end && found.start() < cut {
                    cut = found.start();
                }
            }
        }
        while cut > 0 && (self.hold[cut] & 0b1100_0000) == 0b1000_0000 {
            cut -= 1;
        }
        if cut == 0 {
            return;
        }
        let prefix = String::from_utf8_lossy(&self.hold[..cut]);
        self.emitted.extend_from_slice(redact(&prefix).as_bytes());
        self.hold.drain(..cut);
    }

    pub fn finish(mut self) -> Vec<u8> {
        let rest = String::from_utf8_lossy(&self.hold);
        self.emitted.extend_from_slice(redact(&rest).as_bytes());
        self.emitted
    }
}

impl Default for StreamRedactor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secrets() -> Vec<String> {
        let mut out = Vec::new();
        let samples = [
            "sk-ant-api03_ABCDEFGHIJKLMNOPQRSTUVWXYZ012345",
            "sk-projABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789",
            "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij",
            "github_pat_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcd",
            "ghu_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij",
            "sk_live_ABCDEFGHIJKLMNOPQRST",
            "sk_test_ABCDEFGHIJKLMNOPQRST",
            "whsec_ABCDEFGHIJKLMNOPQRSTUV",
            "AKIAIOSFODNN7EXAMPLE",
            "xoxb-123456789012-ABCDEFGHIJKLMNOP",
            "AIzaSyA-ABCDEFGHIJKLMNOPQRSTUVWX12",
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
            "password=supersecretvalue123456",
            "token=anothersecretvalue1234567",
        ];
        for sample in samples {
            out.push(sample.to_string());
        }
        while out.len() < 60 {
            let n = out.len();
            out.push(format!("ghp_{n:036}"));
        }
        out
    }

    #[test]
    fn f_tl_02_1_corpus_redacted_no_false_positives() {
        for secret in secrets() {
            let wrapped = format!("before {secret} after");
            let cleaned = redact(&wrapped);
            assert!(!cleaned.contains(&secret), "leaked {secret}");
            assert!(
                cleaned.contains("[REDACTED]"),
                "missing marker for {secret}"
            );
        }
        let mut benign = String::new();
        for i in 0..1000 {
            benign.push_str(&format!("fn benign_{i}() {{ let value = {i}; }}\n"));
        }
        assert_eq!(redact(&benign), benign);
    }

    #[test]
    fn f_tl_02_2_split_secret_every_offset() {
        let secret = "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij";
        for offset in 0..=secret.len() {
            let mut stream = StreamRedactor::new();
            stream.push(&secret.as_bytes()[..offset]);
            stream.push(&secret.as_bytes()[offset..]);
            let out = stream.finish();
            let text = String::from_utf8_lossy(&out);
            assert!(!text.contains(secret), "leaked at offset {offset}");
            assert!(text.contains("[REDACTED]"));
        }
    }
}
