//! A tiny language server for tests (F-CD-23, feature `fixtures`): enough
//! LSP over stdio for the editor's code intelligence, without a real server.
//!
//! - `initialize` offers hover, definition and references.
//! - Opened and changed documents are kept; each line containing `ERROR`
//!   gets an error diagnostic ("fake: error here").
//! - `textDocument/definition` finds `def NAME`, `fn NAME`, `function NAME`
//!   or `class NAME` in the open documents and the files under the root.
//! - `textDocument/references` finds every whole-word use of the name.
//! - `textDocument/hover` answers "fake hover: NAME".
//! - `shutdown` answers; `exit` stops.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

/// Reads one `Content-Length` framed message.
pub async fn read_message<R: AsyncRead + Unpin>(r: &mut BufReader<R>) -> Option<Value> {
    let mut len = None;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).await.ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(v) = line.strip_prefix("Content-Length:") {
            len = v.trim().parse::<usize>().ok();
        }
    }
    let mut body = vec![0u8; len?];
    r.read_exact(&mut body).await.ok()?;
    serde_json::from_slice(&body).ok()
}

/// Writes one framed message.
pub async fn write_message<W: AsyncWrite + Unpin>(w: &mut W, v: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(v)?;
    w.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
        .await?;
    w.write_all(&body).await?;
    w.flush().await
}

fn uri_to_path(uri: &str) -> PathBuf {
    PathBuf::from(uri.strip_prefix("file://").unwrap_or(uri))
}

fn path_to_uri(p: &Path) -> String {
    format!("file://{}", p.to_string_lossy())
}

fn word_at(text: &str, line: usize, character: usize) -> Option<String> {
    let l: Vec<char> = text.lines().nth(line)?.chars().collect();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut start = character.min(l.len());
    while start > 0 && is_word(l[start - 1]) {
        start -= 1;
    }
    let mut end = character.min(l.len());
    while end < l.len() && is_word(l[end]) {
        end += 1;
    }
    (end > start).then(|| l[start..end].iter().collect())
}

/// The files under `root` (skipping hidden folders), at most 500.
fn files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            let hidden = e.file_name().to_string_lossy().starts_with('.');
            if hidden {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
            } else if out.len() < 500 {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

struct Server {
    root: Option<PathBuf>,
    docs: BTreeMap<String, String>,
}

impl Server {
    /// Every document: the open ones, then the files on disk not open.
    fn all(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = self
            .docs
            .iter()
            .map(|(u, t)| (u.clone(), t.clone()))
            .collect();
        if let Some(root) = &self.root {
            for p in files(root) {
                let uri = path_to_uri(&p);
                if !self.docs.contains_key(&uri) {
                    if let Ok(t) = std::fs::read_to_string(&p) {
                        out.push((uri, t));
                    }
                }
            }
        }
        out
    }

    fn location(uri: &str, line: usize, col: usize, len: usize) -> Value {
        json!({ "uri": uri, "range": {
            "start": { "line": line, "character": col },
            "end": { "line": line, "character": col + len },
        }})
    }

    fn definition(&self, name: &str) -> Vec<Value> {
        let mut out = Vec::new();
        for (uri, text) in self.all() {
            for (i, l) in text.lines().enumerate() {
                for kw in ["def ", "fn ", "function ", "class "] {
                    let pat = format!("{kw}{name}");
                    if let Some(at) = l.find(&pat) {
                        let rest = &l[at + pat.len()..];
                        if !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
                            let col = l[..at + kw.len()].chars().count();
                            out.push(Self::location(&uri, i, col, name.chars().count()));
                        }
                    }
                }
            }
        }
        out
    }

    fn references(&self, name: &str) -> Vec<Value> {
        let mut out = Vec::new();
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        for (uri, text) in self.all() {
            for (i, l) in text.lines().enumerate() {
                let chars: Vec<char> = l.chars().collect();
                let n: Vec<char> = name.chars().collect();
                let mut j = 0;
                while j + n.len() <= chars.len() {
                    let before = j == 0 || !is_word(chars[j - 1]);
                    let after = j + n.len() == chars.len() || !is_word(chars[j + n.len()]);
                    if before && after && chars[j..j + n.len()] == n[..] {
                        out.push(Self::location(&uri, i, j, n.len()));
                        j += n.len();
                    } else {
                        j += 1;
                    }
                }
            }
        }
        out
    }

    fn diagnostics(uri: &str, text: &str) -> Value {
        let items: Vec<Value> = text
            .lines()
            .enumerate()
            .filter(|(_, l)| l.contains("ERROR"))
            .map(|(i, l)| {
                json!({
                    "range": { "start": { "line": i, "character": 0 }, "end": { "line": i, "character": l.chars().count() } },
                    "severity": 1,
                    "message": "fake: error here",
                })
            })
            .collect();
        json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": { "uri": uri, "diagnostics": items } })
    }

    fn name_at(&self, params: &Value) -> Option<String> {
        let uri = params["textDocument"]["uri"].as_str()?;
        let text = self
            .docs
            .get(uri)
            .cloned()
            .or_else(|| std::fs::read_to_string(uri_to_path(uri)).ok())?;
        let line = params["position"]["line"].as_u64()? as usize;
        let character = params["position"]["character"].as_u64()? as usize;
        word_at(&text, line, character)
    }
}

/// Serves one client until `exit` or end of input.
pub async fn serve<R, W>(reader: R, mut writer: W)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut s = Server {
        root: None,
        docs: BTreeMap::new(),
    };
    while let Some(msg) = read_message(&mut reader).await {
        let method = msg["method"].as_str().unwrap_or("").to_string();
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let result = match method.as_str() {
            "initialize" => {
                s.root = params["rootUri"].as_str().map(uri_to_path);
                Some(json!({ "capabilities": {
                    "textDocumentSync": 1,
                    "hoverProvider": true,
                    "definitionProvider": true,
                    "referencesProvider": true,
                }}))
            }
            "textDocument/didOpen" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or("").to_string();
                let text = params["textDocument"]["text"].as_str().unwrap_or("").to_string();
                let _ = write_message(&mut writer, &Server::diagnostics(&uri, &text)).await;
                s.docs.insert(uri, text);
                None
            }
            "textDocument/didChange" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or("").to_string();
                if let Some(text) = params["contentChanges"][0]["text"].as_str() {
                    let _ = write_message(&mut writer, &Server::diagnostics(&uri, text)).await;
                    s.docs.insert(uri, text.to_string());
                }
                None
            }
            "textDocument/definition" => Some(json!(s.name_at(&params).map(|n| s.definition(&n)).unwrap_or_default())),
            "textDocument/references" => Some(json!(s.name_at(&params).map(|n| s.references(&n)).unwrap_or_default())),
            "textDocument/hover" => Some(
                s.name_at(&params)
                    .map(|n| json!({ "contents": { "kind": "plaintext", "value": format!("fake hover: {n}") } }))
                    .unwrap_or(Value::Null),
            ),
            "shutdown" => Some(Value::Null),
            "exit" => return,
            _ => id.as_ref().map(|_| Value::Null),
        };
        if let (Some(id), Some(result)) = (id, result) {
            let _ = write_message(
                &mut writer,
                &json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            )
            .await;
        }
    }
}
