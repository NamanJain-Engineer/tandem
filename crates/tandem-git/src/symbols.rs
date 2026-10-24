//! Intent radar (F-WT-10): the symbols a worktree changed since its base,
//! and the names its new lines use, from tree-sitter parses of TypeScript
//! and TSX, Rust and Python. Two worktrees share a symbol when both changed
//! (or removed) its definition, or one removed or changed it and the
//! other's new lines use it. Reads only.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::hash::{Hash, Hasher};
use std::path::Path;

use serde::{Deserialize, Serialize};
use tree_sitter::{Language, Node, Parser};
use ts_rs::TS;

use crate::cli::{Git, Mode};
use crate::overlap::changed_files;
use crate::GitError;

/// Files bigger than this aren't parsed.
pub const MAX_FILE_BYTES: usize = 512 * 1024;
/// At most this many changed files are parsed per worktree.
pub const MAX_FILES: usize = 300;
/// Shorter names aren't matched between worktrees (`id`, `x`, `fn`).
pub const MIN_NAME: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    TypeScript,
    Tsx,
    Rust,
    Python,
}

impl Lang {
    pub fn of(path: &str) -> Option<Self> {
        let ext = Path::new(path).extension()?.to_str()?;
        Some(match ext {
            "ts" | "mts" | "cts" => Self::TypeScript,
            "tsx" => Self::Tsx,
            "rs" => Self::Rust,
            "py" => Self::Python,
            _ => return None,
        })
    }

    fn language(self) -> Language {
        match self {
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
        }
    }

    fn separator(self) -> &'static str {
        match self {
            Self::Rust => "::",
            _ => ".",
        }
    }
}

/// What a worktree did to a symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SymbolChange {
    Added,
    Changed,
    Removed,
    /// Its new or changed lines use the name.
    Used,
}

impl SymbolChange {
    /// Which change wins when one name changed several ways.
    fn weight(self) -> u8 {
        match self {
            Self::Removed => 3,
            Self::Changed => 2,
            Self::Added => 1,
            Self::Used => 0,
        }
    }
}

/// A definition in one version of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub name: String,
    pub qualified: String,
    pub kind: &'static str,
    /// 1-based.
    pub line: u32,
    text: u64,
}

/// A definition the worktree added, changed or removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ChangedSymbol {
    pub name: String,
    pub qualified: String,
    pub kind: String,
    pub path: String,
    pub line: u32,
    pub change: SymbolChange,
}

/// A worktree's symbol index.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SymbolIndex {
    pub symbols: Vec<ChangedSymbol>,
    /// Names the worktree's new or changed lines use.
    pub uses: BTreeSet<String>,
    /// Changed files not parsed: other languages, too big, or past the cap.
    pub unparsed: u32,
}

/// A symbol two worktrees share.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SharedSymbol {
    pub name: String,
    /// `here`'s qualified name when it has a definition, else `there`'s.
    pub qualified: String,
    pub here: SymbolChange,
    pub there: SymbolChange,
}

fn hash(bytes: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

fn parse(lang: Lang, source: &str) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    parser.set_language(&lang.language()).ok()?;
    parser.parse(source, None)
}

fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("")
}

/// The kind a node defines, and the node holding its name.
fn definition<'t>(lang: Lang, node: Node<'t>) -> Option<(&'static str, Node<'t>)> {
    let named = |kind: &'static str| node.child_by_field_name("name").map(|n| (kind, n));
    match (lang, node.kind()) {
        (Lang::TypeScript | Lang::Tsx, k) => match k {
            "function_declaration" | "generator_function_declaration" | "function_signature" => {
                named("function")
            }
            "class_declaration" | "abstract_class_declaration" => named("class"),
            "interface_declaration" => named("interface"),
            "type_alias_declaration" => named("type"),
            "enum_declaration" => named("enum"),
            "method_definition" | "method_signature" | "abstract_method_signature" => {
                named("method")
            }
            "internal_module" | "module" => named("module"),
            // `const f = () => …` and `const f = function …`.
            "variable_declarator" => {
                let value = node.child_by_field_name("value")?;
                matches!(
                    value.kind(),
                    "arrow_function" | "function_expression" | "function"
                )
                .then(|| named("function"))
                .flatten()
            }
            _ => None,
        },
        (Lang::Rust, k) => match k {
            "function_item" | "function_signature_item" => named("function"),
            "struct_item" => named("struct"),
            "enum_item" => named("enum"),
            "union_item" => named("union"),
            "trait_item" => named("trait"),
            "type_item" => named("type"),
            "mod_item" => named("module"),
            "macro_definition" => named("macro"),
            "const_item" | "static_item" => named("constant"),
            _ => None,
        },
        (Lang::Python, k) => match k {
            "function_definition" => named("function"),
            "class_definition" => named("class"),
            _ => None,
        },
    }
}

/// The type an `impl` block is for: its methods live under that name.
fn impl_owner(node: Node<'_>, source: &str) -> Option<String> {
    let ty = node.child_by_field_name("type")?;
    let mut name = text(ty, source);
    // `Session<T>` → `Session`; `crate::a::Session` → `Session`.
    if let Some((head, _)) = name.split_once('<') {
        name = head;
    }
    let name = name.rsplit("::").next().unwrap_or(name).trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// The definitions in `source`, with qualified names.
pub fn definitions(lang: Lang, source: &str) -> Vec<Definition> {
    let Some(tree) = parse(lang, source) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut scope: Vec<String> = Vec::new();
    walk_definitions(lang, tree.root_node(), source, &mut scope, &mut out);
    out
}

fn walk_definitions(
    lang: Lang,
    node: Node<'_>,
    source: &str,
    scope: &mut Vec<String>,
    out: &mut Vec<Definition>,
) {
    let mut pushed = false;
    if let Some((kind, name_node)) = definition(lang, node) {
        let name = text(name_node, source).to_string();
        if !name.is_empty() {
            let mut qualified = scope.join(lang.separator());
            if !qualified.is_empty() {
                qualified.push_str(lang.separator());
            }
            qualified.push_str(&name);
            out.push(Definition {
                name: name.clone(),
                qualified,
                kind,
                line: u32::try_from(node.start_position().row + 1).unwrap_or(u32::MAX),
                text: hash(text(node, source).as_bytes()),
            });
            scope.push(name);
            pushed = true;
        }
    } else if lang == Lang::Rust && node.kind() == "impl_item" {
        if let Some(owner) = impl_owner(node, source) {
            scope.push(owner);
            pushed = true;
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk_definitions(lang, child, source, scope, out);
    }
    if pushed {
        scope.pop();
    }
}

/// Identifier-like node kinds whose text is a name in use.
fn is_name(lang: Lang, kind: &str) -> bool {
    match lang {
        Lang::TypeScript | Lang::Tsx => matches!(
            kind,
            "identifier"
                | "type_identifier"
                | "property_identifier"
                | "shorthand_property_identifier"
                | "nested_type_identifier"
        ),
        Lang::Rust => matches!(kind, "identifier" | "type_identifier" | "field_identifier"),
        Lang::Python => kind == "identifier",
    }
}

/// The names used on `lines` (1-based) of `source`; every line when `None`.
pub fn uses(lang: Lang, source: &str, lines: Option<&BTreeSet<u32>>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(tree) = parse(lang, source) else {
        return out;
    };
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        let row = u32::try_from(node.start_position().row + 1).unwrap_or(u32::MAX);
        let end = u32::try_from(node.end_position().row + 1).unwrap_or(u32::MAX);
        // Skip subtrees with none of the wanted lines.
        if let Some(lines) = lines {
            if lines.range(row..=end).next().is_none() {
                continue;
            }
        }
        if is_name(lang, node.kind()) {
            let name = text(node, source);
            if name.chars().count() >= MIN_NAME {
                out.insert(name.to_string());
            }
            continue;
        }
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
    out
}

/// What changed between a file's base and worktree versions.
pub fn compare(
    lang: Lang,
    path: &str,
    base: Option<&str>,
    now: Option<&str>,
) -> Vec<ChangedSymbol> {
    let index = |source: Option<&str>| {
        let mut by: BTreeMap<String, Vec<Definition>> = BTreeMap::new();
        for d in source.map(|s| definitions(lang, s)).unwrap_or_default() {
            by.entry(d.qualified.clone()).or_default().push(d);
        }
        by
    };
    let (before, after) = (index(base), index(now));
    let mut out = Vec::new();
    let symbol = |d: &Definition, change| ChangedSymbol {
        name: d.name.clone(),
        qualified: d.qualified.clone(),
        kind: d.kind.to_string(),
        path: path.to_string(),
        line: d.line,
        change,
    };
    for (q, now_defs) in &after {
        match before.get(q) {
            None => out.extend(
                now_defs
                    .iter()
                    .take(1)
                    .map(|d| symbol(d, SymbolChange::Added)),
            ),
            Some(was) => {
                let texts = |v: &Vec<Definition>| {
                    let mut t: Vec<u64> = v.iter().map(|d| d.text).collect();
                    t.sort_unstable();
                    t
                };
                if texts(was) != texts(now_defs) {
                    out.extend(
                        now_defs
                            .iter()
                            .take(1)
                            .map(|d| symbol(d, SymbolChange::Changed)),
                    );
                }
            }
        }
    }
    for (q, was) in &before {
        if !after.contains_key(q) {
            out.extend(was.iter().take(1).map(|d| symbol(d, SymbolChange::Removed)));
        }
    }
    out
}

/// The new side's line numbers in a `git diff -U0` of one file.
pub fn added_lines(diff: &str) -> BTreeSet<u32> {
    let mut out = BTreeSet::new();
    for line in diff.lines().filter(|l| l.starts_with("@@")) {
        // `@@ -a,b +c,d @@`: lines c..c+d (d defaults to 1).
        let Some(new) = line.split_whitespace().find(|w| w.starts_with('+')) else {
            continue;
        };
        let mut parts = new[1..].split(',');
        let start: u32 = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
        let count: u32 = parts.next().and_then(|n| n.parse().ok()).unwrap_or(1);
        out.extend(start..start.saturating_add(count));
    }
    out
}

async fn show(git: &Git, root: &Path, base: &str, path: &str) -> Result<Option<String>, GitError> {
    let spec = format!("{base}:{path}");
    let out = git
        .run(Mode::Read, root, &[OsStr::new("show"), OsStr::new(&spec)])
        .await?;
    Ok(out.success().then(|| out.text().to_string()))
}

/// `git diff -U0 <base> -- <path>`: the worktree's file against the base.
async fn diff(git: &Git, root: &Path, base: &str, path: &str) -> Result<String, GitError> {
    let out = git
        .ok(
            Mode::Read,
            root,
            &[
                OsStr::new("diff"),
                OsStr::new("-U0"),
                OsStr::new("--no-color"),
                OsStr::new("--no-ext-diff"),
                OsStr::new(base),
                OsStr::new("--"),
                OsStr::new(path),
            ],
        )
        .await?;
    Ok(out.text().to_string())
}

/// One changed file's parse, kept until its content changes.
#[derive(Debug, Clone)]
struct Parsed {
    /// What the worktree's file held when parsed (`None`: missing).
    content: Option<u64>,
    symbols: Vec<ChangedSymbol>,
    uses: BTreeSet<String>,
    unparsed: bool,
}

/// A worktree's parses by path, for one base. A refresh parses only the
/// files whose content changed since (in practice, those the last turn
/// touched); the rest are reused.
#[derive(Debug, Clone, Default)]
pub struct ParseCache {
    base: String,
    files: BTreeMap<String, Parsed>,
}

/// What a refresh did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Refreshed {
    pub index: SymbolIndex,
    /// Files parsed this time (the others came from the cache).
    pub parsed: u32,
}

/// The worktree file's bytes, or `None` when it's missing or too big
/// (`Err(())`: too big).
fn read_file(path: &Path) -> Result<Option<String>, ()> {
    match std::fs::metadata(path) {
        Ok(m) if m.len() > MAX_FILE_BYTES as u64 => Err(()),
        Ok(_) => Ok(std::fs::read_to_string(path).ok()),
        Err(_) => Ok(None),
    }
}

/// The worktree at `root`'s index against `base`: its changed files, as
/// F-WT-03 counts them. Files whose content is unchanged since `cache`'s
/// last refresh aren't parsed again; tree-sitter runs on the blocking
/// pool, never on the async runtime's threads.
pub async fn index_cached(
    git: &Git,
    root: &Path,
    base: &str,
    cache: &mut ParseCache,
) -> Result<Refreshed, GitError> {
    if cache.base != base {
        *cache = ParseCache {
            base: base.to_string(),
            files: BTreeMap::new(),
        };
    }
    let files = changed_files(git, root, base).await?;
    let mut out = Refreshed::default();
    let mut kept = BTreeMap::new();
    for (n, path) in files.iter().enumerate() {
        let Some(lang) = Lang::of(path) else {
            out.index.unparsed += 1;
            continue;
        };
        if n >= MAX_FILES {
            out.index.unparsed += 1;
            continue;
        }
        let full = root.join(path);
        let read = tokio::task::spawn_blocking(move || read_file(&full))
            .await
            .unwrap_or(Ok(None));
        let Ok(now) = read else {
            out.index.unparsed += 1;
            continue;
        };
        let content = now.as_ref().map(|t| hash(t.as_bytes()));
        let parsed = match cache.files.get(path) {
            Some(p) if p.content == content => p.clone(),
            _ => {
                out.parsed += 1;
                let was = show(git, root, base, path).await?;
                if was.as_ref().is_some_and(|w| w.len() > MAX_FILE_BYTES) {
                    Parsed {
                        content,
                        symbols: Vec::new(),
                        uses: BTreeSet::new(),
                        unparsed: true,
                    }
                } else {
                    let lines = match (&was, &now) {
                        (Some(_), Some(_)) => {
                            Some(added_lines(&diff(git, root, base, path).await?))
                        }
                        _ => None,
                    };
                    let path_owned = path.clone();
                    let (symbols, used) = tokio::task::spawn_blocking(move || {
                        let symbols = compare(lang, &path_owned, was.as_deref(), now.as_deref());
                        let used = now
                            .as_deref()
                            .map(|n| uses(lang, n, lines.as_ref()))
                            .unwrap_or_default();
                        (symbols, used)
                    })
                    .await
                    .unwrap_or_default();
                    Parsed {
                        content,
                        symbols,
                        uses: used,
                        unparsed: false,
                    }
                }
            }
        };
        if parsed.unparsed {
            out.index.unparsed += 1;
        }
        out.index.symbols.extend(parsed.symbols.iter().cloned());
        out.index.uses.extend(parsed.uses.iter().cloned());
        kept.insert(path.clone(), parsed);
    }
    // Files no longer changed are dropped.
    cache.files = kept;
    Ok(out)
}

/// [`index_cached`] from nothing.
pub async fn index(git: &Git, root: &Path, base: &str) -> Result<SymbolIndex, GitError> {
    Ok(index_cached(git, root, base, &mut ParseCache::default())
        .await?
        .index)
}

/// What `of` did to `name`, its strongest change, with the qualified name.
fn change_of<'a>(of: &'a SymbolIndex, name: &str) -> Option<(&'a str, SymbolChange)> {
    of.symbols
        .iter()
        .filter(|s| s.name == name)
        .max_by_key(|s| s.change.weight())
        .map(|s| (s.qualified.as_str(), s.change))
}

fn touches(c: SymbolChange) -> bool {
    matches!(c, SymbolChange::Changed | SymbolChange::Removed)
}

/// The symbols `here` and `there` share: both changed or removed a
/// definition of the name, or one changed or removed it and the other's
/// new lines use it.
pub fn shared_symbols(here: &SymbolIndex, there: &SymbolIndex) -> Vec<SharedSymbol> {
    let names: BTreeSet<&str> = here
        .symbols
        .iter()
        .chain(there.symbols.iter())
        .map(|s| s.name.as_str())
        .filter(|n| n.chars().count() >= MIN_NAME)
        .collect();
    let mut out = Vec::new();
    for name in names {
        let mine = change_of(here, name);
        let theirs = change_of(there, name);
        let pair = match (mine, theirs) {
            (Some((q, a)), Some((_, b))) if touches(a) && touches(b) => Some((q, a, b)),
            (Some((q, a)), _) if touches(a) && there.uses.contains(name) => {
                Some((q, a, SymbolChange::Used))
            }
            (_, Some((q, b))) if touches(b) && here.uses.contains(name) => {
                Some((q, SymbolChange::Used, b))
            }
            _ => None,
        };
        if let Some((qualified, a, b)) = pair {
            out.push(SharedSymbol {
                name: name.to_string(),
                qualified: qualified.to_string(),
                here: a,
                there: b,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(defs: &[Definition]) -> Vec<(&str, &str)> {
        defs.iter()
            .map(|d| (d.qualified.as_str(), d.kind))
            .collect()
    }

    #[test]
    fn f_wt_10_2_definitions_in_each_language_with_qualified_names() {
        let ts = "export class Session {\n  refresh(): void {}\n}\nexport function login() {}\nconst logout = () => 1;\ninterface Opts { a: string }\ntype Id = string;\n";
        assert_eq!(
            names(&definitions(Lang::TypeScript, ts)),
            [
                ("Session", "class"),
                ("Session.refresh", "method"),
                ("login", "function"),
                ("logout", "function"),
                ("Opts", "interface"),
                ("Id", "type"),
            ]
        );
        let rs = "mod auth {\n  pub struct Session;\n  impl Session {\n    pub fn refresh(&self) {}\n  }\n  impl<T> Drop for Wrapper<T> { fn drop(&mut self) {} }\n}\nfn main() {}\n";
        assert_eq!(
            names(&definitions(Lang::Rust, rs)),
            [
                ("auth", "module"),
                ("auth::Session", "struct"),
                ("auth::Session::refresh", "function"),
                ("auth::Wrapper::drop", "function"),
                ("main", "function"),
            ]
        );
        let py = "class Session:\n    def refresh(self):\n        pass\n\ndef login():\n    pass\n";
        assert_eq!(
            names(&definitions(Lang::Python, py)),
            [
                ("Session", "class"),
                ("Session.refresh", "function"),
                ("login", "function"),
            ]
        );
        let tsx = "export function App() { return <div className=\"a\" />; }\n";
        assert_eq!(names(&definitions(Lang::Tsx, tsx)), [("App", "function")]);
        assert_eq!(Lang::of("a/b.mts"), Some(Lang::TypeScript));
        assert_eq!(Lang::of("README.md"), None);
    }

    #[test]
    fn f_wt_10_2_added_changed_and_removed() {
        let was =
            "def keep():\n    return 1\n\ndef edit():\n    return 1\n\ndef gone():\n    pass\n";
        let now =
            "def keep():\n    return 1\n\ndef edit():\n    return 2\n\ndef fresh():\n    pass\n";
        let mut got: Vec<(String, SymbolChange)> =
            compare(Lang::Python, "m.py", Some(was), Some(now))
                .into_iter()
                .map(|s| (s.qualified, s.change))
                .collect();
        got.sort();
        assert_eq!(
            got,
            [
                ("edit".to_string(), SymbolChange::Changed),
                ("fresh".to_string(), SymbolChange::Added),
                ("gone".to_string(), SymbolChange::Removed),
            ]
        );
        // A new file adds everything; a deleted one removes everything.
        assert!(compare(Lang::Python, "m.py", None, Some(now))
            .iter()
            .all(|s| s.change == SymbolChange::Added));
        assert!(compare(Lang::Python, "m.py", Some(was), None)
            .iter()
            .all(|s| s.change == SymbolChange::Removed));
    }

    #[test]
    fn f_wt_10_1_uses_are_read_from_the_new_lines_only() {
        let src = "import { Session } from './s';\nconst old = oldThing();\nconst s = new Session();\ns.refresh();\n";
        let lines: BTreeSet<u32> = [3, 4].into();
        let got = uses(Lang::TypeScript, src, Some(&lines));
        assert!(
            got.contains("Session") && got.contains("refresh"),
            "{got:?}"
        );
        assert!(!got.contains("oldThing"), "line 2 isn't new");
        assert_eq!(
            added_lines("@@ -1,0 +2,3 @@\n+a\n@@ -9 +12 @@\n+b\n@@ -20,2 +22,0 @@\n"),
            [2, 3, 4, 12].into()
        );
    }

    fn idx(symbols: &[(&str, SymbolChange)], uses: &[&str]) -> SymbolIndex {
        SymbolIndex {
            symbols: symbols
                .iter()
                .map(|(q, c)| ChangedSymbol {
                    name: q.rsplit('.').next().unwrap_or(q).to_string(),
                    qualified: (*q).to_string(),
                    kind: "class".into(),
                    path: "a.ts".into(),
                    line: 1,
                    change: *c,
                })
                .collect(),
            uses: uses.iter().map(|s| (*s).to_string()).collect(),
            unparsed: 0,
        }
    }

    #[test]
    fn f_wt_10_1_3_what_two_worktrees_share() {
        use SymbolChange::*;
        // A rename against a use.
        let renamed = idx(
            &[
                ("Session", Removed),
                ("UserSession", Added),
                ("Session.refresh", Removed),
                ("UserSession.refresh", Added),
            ],
            &["UserSession"],
        );
        let caller = idx(&[], &["Session", "refresh", "session"]);
        let got = shared_symbols(&renamed, &caller);
        assert_eq!(
            got.iter()
                .map(|s| (s.name.as_str(), s.here, s.there))
                .collect::<Vec<_>>(),
            [("Session", Removed, Used), ("refresh", Removed, Used)]
        );
        assert_eq!(shared_symbols(&caller, &renamed)[0].here, Used);
        // Both changed the same function.
        let a = idx(&[("parse", Changed)], &[]);
        let b = idx(&[("parse", Changed)], &[]);
        assert_eq!(shared_symbols(&a, &b).len(), 1);
        // Unrelated additions, an added definition against a use, short names.
        let a = idx(&[("alpha", Added), ("id", Removed)], &[]);
        let b = idx(&[("beta", Added)], &["alpha", "id"]);
        assert!(shared_symbols(&a, &b).is_empty());
    }
}
