//! Intent radar (F-WT-10): the symbols a worktree changed since its base,
//! and the names its new lines use, from tree-sitter parses of TypeScript
//! and TSX, Rust, Python, Go and Java. Two worktrees share a symbol when
//! both changed (or removed, or renamed) its definition, or one removed,
//! renamed or changed it and the other's new lines use it. A rename is
//! found only when nothing else could explain the pair ([`compare`]).
//! Reads only.

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
/// A definition with less text than this (non-space characters, its name
/// set aside) never pairs as a rename by its text alone: two stubs alike
/// (`def a(): pass`) are as likely two different functions.
pub const MIN_RENAME_TEXT: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    TypeScript,
    Tsx,
    Rust,
    Python,
    Go,
    Java,
}

impl Lang {
    pub fn of(path: &str) -> Option<Self> {
        let ext = Path::new(path).extension()?.to_str()?;
        Some(match ext {
            "ts" | "mts" | "cts" => Self::TypeScript,
            "tsx" => Self::Tsx,
            "rs" => Self::Rust,
            "py" => Self::Python,
            "go" => Self::Go,
            "java" => Self::Java,
            _ => return None,
        })
    }

    fn language(self) -> Language {
        match self {
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            Self::Java => tree_sitter_java::LANGUAGE.into(),
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
    /// Removed, with a definition added in its place that differs only by
    /// its name ([`ChangedSymbol::renamed_to`]).
    Renamed,
    /// Its new or changed lines use the name.
    Used,
}

impl SymbolChange {
    /// Which change wins when one name changed several ways (a plain
    /// removal outranks a rename: it's the less certain reading).
    fn weight(self) -> u8 {
        match self {
            Self::Removed => 4,
            Self::Renamed => 3,
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
    /// The text's hash with each whole-word use of its own name set aside:
    /// equal for two definitions that differ only by their names.
    shape: u64,
    /// Non-space characters in the text, its name set aside.
    size: usize,
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
    /// For [`SymbolChange::Renamed`]: the qualified name it has now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub renamed_to: Option<String>,
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
    /// When a side renamed it: the qualified name that side gives it now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub renamed_to: Option<String>,
}

impl SharedSymbol {
    /// How a notice names it: `parse → parseLine` for a rename.
    pub fn display(&self) -> String {
        match &self.renamed_to {
            Some(to) => format!("{} → {to}", self.qualified),
            None => self.qualified.clone(),
        }
    }
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
        (Lang::Go, k) => match k {
            "function_declaration" => named("function"),
            "method_declaration" => named("method"),
            // `type Server struct { … }`: the spec names it.
            "type_spec" | "type_alias" => {
                let kind = match node.child_by_field_name("type").map(|t| t.kind()) {
                    Some("struct_type") => "struct",
                    Some("interface_type") => "interface",
                    _ => "type",
                };
                named(kind)
            }
            _ => None,
        },
        (Lang::Java, k) => match k {
            "class_declaration" => named("class"),
            "interface_declaration" | "annotation_type_declaration" => named("interface"),
            "enum_declaration" => named("enum"),
            "record_declaration" => named("record"),
            "method_declaration" => named("method"),
            "constructor_declaration" => named("constructor"),
            _ => None,
        },
    }
}

/// The type a Go method's receiver names (`func (s *Server) Handle()` →
/// `Server`): its methods live under that name.
fn go_receiver(node: Node<'_>, source: &str) -> Option<String> {
    let receiver = node.child_by_field_name("receiver")?;
    let mut stack = vec![receiver];
    while let Some(n) = stack.pop() {
        if n.kind() == "type_identifier" {
            let name = text(n, source).trim();
            return (!name.is_empty()).then(|| name.to_string());
        }
        let mut cursor = n.walk();
        stack.extend(n.named_children(&mut cursor));
    }
    None
}

/// `text` with every whole-word `name` (letters, digits and `_` around it
/// end the word) set aside.
fn without_name(text: &str, name: &str) -> String {
    if name.is_empty() {
        return text.to_string();
    }
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(name) {
        let before = rest[..i].chars().next_back();
        let after = rest[i + name.len()..].chars().next();
        out.push_str(&rest[..i]);
        if before.is_some_and(word) || after.is_some_and(word) {
            out.push_str(name);
        } else {
            out.push('\u{0}');
        }
        rest = &rest[i + name.len()..];
    }
    out.push_str(rest);
    out
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
            // A Go method lives under its receiver's type.
            if lang == Lang::Go && kind == "method" {
                if let Some(owner) = go_receiver(node, source) {
                    qualified = format!("{owner}.{name}");
                }
            }
            let whole = text(node, source);
            let unnamed = without_name(whole, &name);
            out.push(Definition {
                name: name.clone(),
                qualified,
                kind,
                line: u32::try_from(node.start_position().row + 1).unwrap_or(u32::MAX),
                text: hash(whole.as_bytes()),
                shape: hash(unnamed.as_bytes()),
                size: unnamed
                    .chars()
                    .filter(|c| !c.is_whitespace() && *c != '\u{0}')
                    .count(),
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
        Lang::Go => matches!(
            kind,
            "identifier" | "type_identifier" | "field_identifier" | "package_identifier"
        ),
        Lang::Java => matches!(kind, "identifier" | "type_identifier"),
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

/// The scope part of a qualified name (`Session` for `Session.refresh`).
fn scope_of<'a>(qualified: &'a str, sep: &str) -> &'a str {
    qualified.rsplit_once(sep).map(|(s, _)| s).unwrap_or("")
}

/// Renames among the definitions `removed` from a file and those `added`
/// to it, as (removed qualified, added qualified). A pair is a rename only
/// when nothing else could explain it: the same kind, the same scope (once
/// the scope's own rename, found first, is applied), and the same text
/// once each one's own name is set aside; and each is the other's only such
/// match. A type's members that moved with it unchanged keep their names, so
/// `Session.refresh` → `UserSession.refresh` is renamed with `Session`.
/// Anything less certain (a rename with an edit, two that could pair either
/// way) isn't a rename: a false one is worse than none.
fn renames(lang: Lang, removed: &[&Definition], added: &[&Definition]) -> Vec<(String, String)> {
    let sep = lang.separator();
    let mut by_depth: Vec<&Definition> = removed.to_vec();
    by_depth.sort_by_key(|d| (d.qualified.matches(sep).count(), d.qualified.clone()));
    let mut found: BTreeMap<String, String> = BTreeMap::new();
    let mut taken: BTreeSet<String> = BTreeSet::new();
    let mapped = |scope: &str, found: &BTreeMap<String, String>| -> String {
        found
            .get(scope)
            .cloned()
            .unwrap_or_else(|| scope.to_string())
    };
    // How many removed definitions each added one could pair with.
    let matches = |r: &Definition, a: &Definition, found: &BTreeMap<String, String>| {
        r.kind == a.kind
            && r.shape == a.shape
            && mapped(scope_of(&r.qualified, sep), found) == scope_of(&a.qualified, sep)
    };
    for r in &by_depth {
        let candidates: Vec<&&Definition> = added
            .iter()
            .filter(|a| !taken.contains(&a.qualified) && matches(r, a, &found))
            .collect();
        let [a] = candidates.as_slice() else {
            continue;
        };
        // And `r` is the only removed one that `a` could pair with.
        let rivals = by_depth
            .iter()
            .filter(|o| !found.contains_key(&o.qualified) && matches(o, a, &found))
            .count();
        if rivals != 1 {
            continue;
        }
        // A member that moved with its renamed type keeps its name and its
        // text; anything else needs enough text to tell it apart.
        let moved = r.name == a.name;
        if moved && r.text != a.text {
            continue;
        }
        if !moved && r.size < MIN_RENAME_TEXT {
            continue;
        }
        if r.qualified != a.qualified {
            taken.insert(a.qualified.clone());
            found.insert(r.qualified.clone(), a.qualified.clone());
        }
    }
    found.into_iter().collect()
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
    let symbol = |d: &Definition, change, renamed_to: Option<String>| ChangedSymbol {
        name: d.name.clone(),
        qualified: d.qualified.clone(),
        kind: d.kind.to_string(),
        path: path.to_string(),
        line: d.line,
        change,
        renamed_to,
    };
    // Renames pair one definition removed with one added (a name that's
    // defined more than once on either side never pairs).
    let removed: Vec<&Definition> = before
        .iter()
        .filter(|(q, v)| !after.contains_key(*q) && v.len() == 1)
        .map(|(_, v)| &v[0])
        .collect();
    let added: Vec<&Definition> = after
        .iter()
        .filter(|(q, v)| !before.contains_key(*q) && v.len() == 1)
        .map(|(_, v)| &v[0])
        .collect();
    let renamed: BTreeMap<String, String> = renames(lang, &removed, &added).into_iter().collect();
    for (q, now_defs) in &after {
        match before.get(q) {
            None => out.extend(
                now_defs
                    .iter()
                    .take(1)
                    .map(|d| symbol(d, SymbolChange::Added, None)),
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
                            .map(|d| symbol(d, SymbolChange::Changed, None)),
                    );
                }
            }
        }
    }
    for (q, was) in &before {
        if !after.contains_key(q) {
            out.extend(was.iter().take(1).map(|d| match renamed.get(q) {
                Some(to) => symbol(d, SymbolChange::Renamed, Some(to.clone())),
                None => symbol(d, SymbolChange::Removed, None),
            }));
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

/// What `of` did to `name`, its strongest change: the symbol.
fn change_of<'a>(of: &'a SymbolIndex, name: &str) -> Option<&'a ChangedSymbol> {
    of.symbols
        .iter()
        .filter(|s| s.name == name)
        .max_by_key(|s| s.change.weight())
}

fn touches(c: SymbolChange) -> bool {
    matches!(
        c,
        SymbolChange::Changed | SymbolChange::Removed | SymbolChange::Renamed
    )
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
            (Some(m), Some(t)) if touches(m.change) && touches(t.change) => Some((
                m,
                m.change,
                t.change,
                m.renamed_to.clone().or(t.renamed_to.clone()),
            )),
            (Some(m), _) if touches(m.change) && there.uses.contains(name) => {
                Some((m, m.change, SymbolChange::Used, m.renamed_to.clone()))
            }
            (_, Some(t)) if touches(t.change) && here.uses.contains(name) => {
                Some((t, SymbolChange::Used, t.change, t.renamed_to.clone()))
            }
            _ => None,
        };
        if let Some((def, a, b, renamed_to)) = pair {
            out.push(SharedSymbol {
                name: name.to_string(),
                qualified: def.qualified.clone(),
                here: a,
                there: b,
                renamed_to,
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
                    renamed_to: None,
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

    fn changes(lang: Lang, base: &str, now: &str) -> Vec<(String, SymbolChange, Option<String>)> {
        let mut out: Vec<_> = compare(lang, "f", Some(base), Some(now))
            .into_iter()
            .map(|s| (s.qualified, s.change, s.renamed_to))
            .collect();
        out.sort();
        out
    }

    #[test]
    fn f_wt_10_5_go_and_java_definitions_with_qualified_names() {
        let go = "package shop\n\ntype Server struct{ n int }\n\ntype Store interface{ Get() int }\n\nfunc (s *Server) Handle() int { return s.n }\n\nfunc NewServer() *Server { return &Server{} }\n";
        assert_eq!(
            names(&definitions(Lang::Go, go)),
            [
                ("Server", "struct"),
                ("Store", "interface"),
                ("Server.Handle", "method"),
                ("NewServer", "function"),
            ]
        );
        let java = "package com.shop;\n\npublic class Cart {\n  private int n;\n  public Cart() {}\n  public int total() { return n; }\n  interface Line { int price(); }\n}\n\nenum Size { S, M }\n";
        assert_eq!(
            names(&definitions(Lang::Java, java)),
            [
                ("Cart", "class"),
                ("Cart.Cart", "constructor"),
                ("Cart.total", "method"),
                ("Cart.Line", "interface"),
                ("Cart.Line.price", "method"),
                ("Size", "enum"),
            ]
        );
        assert_eq!(Lang::of("cmd/main.go"), Some(Lang::Go));
        assert_eq!(Lang::of("src/Cart.java"), Some(Lang::Java));
        use SymbolChange::*;
        // A changed method body, a new function, a deleted one.
        let base = "package shop\n\ntype Server struct{}\n\nfunc (s *Server) Handle() int { return 1 }\n\nfunc Old() {}\n";
        let now = "package shop\n\ntype Server struct{}\n\nfunc (s *Server) Handle() int { return 2 }\n\nfunc New() int { return 3 }\n";
        assert_eq!(
            changes(Lang::Go, base, now),
            [
                ("New".to_string(), Added, None),
                ("Old".to_string(), Removed, None),
                ("Server.Handle".to_string(), Changed, None),
            ]
        );
        let base = "class Cart {\n  int total() { return 1; }\n}\n";
        let now = "class Cart {\n  int total() { return 2; }\n}\n";
        assert_eq!(
            changes(Lang::Java, base, now),
            [
                ("Cart".to_string(), Changed, None),
                ("Cart.total".to_string(), Changed, None),
            ]
        );
        // Uses on new lines, in both.
        assert!(uses(Lang::Go, "func f() { s.Handle() }\n", None).contains("Handle"));
        assert!(
            uses(Lang::Java, "class A { void f() { cart.total(); } }\n", None).contains("total")
        );
    }

    #[test]
    fn f_wt_10_6_a_rename_is_found_only_when_nothing_else_explains_it() {
        use SymbolChange::*;
        // A function renamed with the same body.
        let base = "export function parse(s: string): string {\n  return s.trim();\n}\n";
        let now = "export function parseLine(s: string): string {\n  return s.trim();\n}\n";
        assert_eq!(
            changes(Lang::TypeScript, base, now),
            [
                ("parse".to_string(), Renamed, Some("parseLine".to_string())),
                ("parseLine".to_string(), Added, None),
            ]
        );
        // A class renamed, its methods moving with it unchanged; a use of its
        // own name inside it (a static factory) is set aside too.
        let base = "export class Session {\n  static make(): Session { return new Session(); }\n  refresh(): void {}\n}\n";
        let now = "export class UserSession {\n  static make(): UserSession { return new UserSession(); }\n  refresh(): void {}\n}\n";
        let got = changes(Lang::TypeScript, base, now);
        assert!(
            got.contains(&(
                "Session".to_string(),
                Renamed,
                Some("UserSession".to_string())
            )),
            "{got:?}"
        );
        assert!(got.contains(&(
            "Session.refresh".to_string(),
            Renamed,
            Some("UserSession.refresh".to_string())
        )));
        // `make` names the class in its text, so it differs: removed, not
        // renamed (its owner's rename doesn't excuse an edit).
        assert!(
            got.contains(&("Session.make".to_string(), Removed, None)),
            "{got:?}"
        );
        // A rename that also edits the body: removed and added.
        let base = "function parse(s: string) { return s.trim(); }\n";
        let now = "function parseLine(s: string) { return s.trim().toLowerCase(); }\n";
        assert_eq!(
            changes(Lang::TypeScript, base, now),
            [
                ("parse".to_string(), Removed, None),
                ("parseLine".to_string(), Added, None),
            ]
        );
        // Two removed and two added that could pair either way: neither.
        let base = "function a1() { return 1; }\nfunction b1() { return 1; }\n";
        let now = "function a2() { return 1; }\nfunction b2() { return 1; }\n";
        assert!(changes(Lang::TypeScript, base, now)
            .iter()
            .all(|(_, c, to)| *c != Renamed && to.is_none()));
        // Another kind, or another scope, doesn't pair.
        let base = "class A { run() { return 1; } }\nfunction go() { return 1; }\n";
        let now = "class A { }\nclass B { run() { return 1; } }\nfunction go2() { return 2; }\n";
        assert!(changes(Lang::TypeScript, base, now)
            .iter()
            .all(|(_, c, _)| *c != Renamed));
        // Two stubs alike never pair: `def gone(): pass` → `def fresh(): pass`.
        let got = changes(
            Lang::Python,
            "def gone():\n    pass\n",
            "def fresh():\n    pass\n",
        );
        assert!(got.iter().all(|(_, c, _)| *c != Renamed), "{got:?}");
        // A name inside a longer word isn't set aside: `parse` in `parser`.
        assert_eq!(without_name("parse(parser)", "parse"), "\u{0}(parser)");
        // Rust and Go renames too.
        let got = changes(
            Lang::Rust,
            "fn load(path: &str) -> usize { path.len() * 2 }\n",
            "fn load_all(path: &str) -> usize { path.len() * 2 }\n",
        );
        assert!(got.contains(&("load".to_string(), Renamed, Some("load_all".to_string()))));
        let got = changes(
            Lang::Go,
            "package a\nfunc Load(path string) int { return len(path) * 2 }\n",
            "package a\nfunc LoadAll(path string) int { return len(path) * 2 }\n",
        );
        assert!(got.contains(&("Load".to_string(), Renamed, Some("LoadAll".to_string()))));
    }

    #[test]
    fn f_wt_10_6_a_renamed_symbol_is_shared_with_its_new_name() {
        use SymbolChange::*;
        let mut renamed = idx(&[("parse", Renamed), ("parseLine", Added)], &[]);
        renamed.symbols[0].renamed_to = Some("parseLine".into());
        let caller = idx(&[], &["parse"]);
        let got = shared_symbols(&caller, &renamed);
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].here, got[0].there), (Used, Renamed));
        assert_eq!(got[0].display(), "parse → parseLine");
        // A plain removal of the same name elsewhere outranks a rename.
        let mut both = idx(&[("parse", Renamed), ("parse", Removed)], &[]);
        both.symbols[0].renamed_to = Some("parseLine".into());
        assert_eq!(shared_symbols(&both, &caller)[0].here, Removed);
    }
}
