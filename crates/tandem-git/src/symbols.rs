//! Intent radar (F-WT-10): the symbols a worktree changed since its base,
//! and the names its new lines use, from tree-sitter parses of TypeScript
//! and TSX, Rust, Python, Go and Java. Two worktrees share a symbol when
//! both changed (or removed, or renamed) its definition, or one removed,
//! renamed or changed it and the other's new lines use it. A rename is
//! found only when nothing else could explain the pair ([`compare`]).
//! Reads only.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tree_sitter::{Language, Node, ParseOptions, ParseState, Parser};
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
    /// Changed files not parsed: other languages, too big, past the cap,
    /// not regular files, not UTF-8, unreadable, or given up on.
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

/// How long reading one side of a file may take, its parse and the walk
/// over its definitions together: past it the file counts as unparsed. A
/// 512 KiB file of noise took 5 to 7 s in a debug build, and a refresh
/// parses up to [`MAX_FILES`] files (the review of 9a4e93ab).
const PARSE_LIMIT: Duration = Duration::from_secs(3);
/// Definitions nested deeper than this aren't read, nor anything inside
/// them: real code nests a handful. Each level's name joined its whole
/// scope and its text was copied and hashed, so a file built to nest
/// thousands took minutes and gigabytes (the review of 30d5bcad, M-1).
const MAX_DEPTH: usize = 32;
/// A refresh starts no file's parse after this long (the review of
/// 30d5bcad). The one under way still has its own limits ([`PARSE_LIMIT`]
/// for each side, and as long again for its renames), so a refresh can run
/// about 10 s past it (the review of d06b9e3d, N-2). The files past it
/// count as unparsed this time and are read by the next refresh
/// ([`Refreshed::more`]), which reuses what this one parsed.
const REFRESH_BUDGET: Duration = Duration::from_secs(20);
/// A file given up on is parsed again after this long, even unchanged, so
/// a parse cut short by a busy machine isn't kept as unparsed for good.
const RETRY_GIVEN_UP: Duration = Duration::from_secs(5 * 60);
/// One side of a file (the base's or the worktree's) holds at most this
/// many bytes of definitions: each one's qualified name, its name and
/// `each` more ([`definitions_of`]); past it the file counts as unparsed.
/// [`MAX_DEPTH`] bounds how deep scopes go, not what they hold: a changed
/// file of 500 KB, one module named with 200,000 letters around 50,000
/// children, gave each child a qualified name of 200 KB, about 10 GB in a
/// second or two (the review of d06b9e3d, MF-1).
const MAX_SIDE_BYTES: usize = 8 * 1024 * 1024;
/// What a definition holds besides its names, for [`MAX_SIDE_BYTES`] and
/// [`MAX_INDEX_BYTES`].
const DEFINITION_BYTES: usize = 64;
/// A worktree's index holds at most this many bytes of symbols and names
/// in use, counted as they sit in memory ([`Parsed::bytes`]). A file whose
/// parse would take it past counts as unparsed, and is parsed again after
/// [`RETRY_GIVEN_UP`]: each file is bounded on its own, and [`MAX_FILES`]
/// of them could still hold gigabytes.
const MAX_INDEX_BYTES: usize = 32 * 1024 * 1024;
/// What a string's heap block costs beyond its text, at most: the
/// allocator's header and its rounding up. Counting names alone put 75 MB
/// of symbols, 150 MB with the parse cache, under 32 MiB (the review of
/// d3b838de, SF-C).
const STRING_HEAP: usize = 32;
/// What a name in use costs beside its string, in its set's tree.
const USE_NODE: usize = 16;

fn parse(lang: Lang, source: &str) -> Option<tree_sitter::Tree> {
    parse_within(lang, source, PARSE_LIMIT)
}

/// [`parse`], given up (`None`) after `limit`.
fn parse_within(lang: Lang, source: &str, limit: Duration) -> Option<tree_sitter::Tree> {
    parse_until(lang, source, Instant::now() + limit)
}

/// [`parse`], given up (`None`) at `deadline`.
fn parse_until(lang: Lang, source: &str, deadline: Instant) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    parser.set_language(&lang.language()).ok()?;
    let mut late = |_: &ParseState| Instant::now() > deadline;
    let bytes = source.as_bytes();
    parser.parse_with_options(
        &mut |at, _| bytes.get(at..).unwrap_or_default(),
        None,
        Some(ParseOptions::new().progress_callback(&mut late)),
    )
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
/// `Server`): its methods live under that name. Read by field, through a
/// pointer and past a generic type's arguments, so `*Server[T]` names
/// `Server`, not `T` (the review of 9a4e93ab, S4).
fn go_receiver(node: Node<'_>, source: &str) -> Option<String> {
    let receiver = node.child_by_field_name("receiver")?;
    let mut cursor = receiver.walk();
    let parameter = receiver
        .named_children(&mut cursor)
        .find(|n| n.kind() == "parameter_declaration")?;
    let mut ty = parameter.child_by_field_name("type")?;
    loop {
        match ty.kind() {
            "pointer_type" | "parenthesized_type" => ty = ty.named_child(0)?,
            "generic_type" => ty = ty.child_by_field_name("type")?,
            "type_identifier" => {
                let name = text(ty, source).trim();
                return (!name.is_empty()).then(|| name.to_string());
            }
            _ => return None,
        }
    }
}

/// `text` with every whole-word `name` (letters, digits and `_` around it
/// end the word) set aside.
fn without_name(text: &str, name: &str) -> String {
    if name.is_empty() {
        return text.to_string();
    }
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    let mut out = String::with_capacity(text.len());
    // Copied up to `kept`; searched from `at`. What's before a match is read
    // in all of `text`: "abab" has no word "ab" (the second follows a `b`).
    let (mut kept, mut at) = (0, 0);
    while let Some(i) = text[at..].find(name).map(|i| at + i) {
        let end = i + name.len();
        let before = text[..i].chars().next_back();
        let after = text[end..].chars().next();
        if !before.is_some_and(word) && !after.is_some_and(word) {
            out.push_str(&text[kept..i]);
            out.push('\u{0}');
            kept = end;
        }
        at = end;
    }
    out.push_str(&text[kept..]);
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

/// The definitions in `source`, with qualified names: none when there are
/// more than [`MAX_SIDE_BYTES`] of them or reading them takes past
/// [`PARSE_LIMIT`].
pub fn definitions(lang: Lang, source: &str) -> Vec<Definition> {
    definitions_until(lang, source, Instant::now() + PARSE_LIMIT, DEFINITION_BYTES)
        .unwrap_or_default()
}

/// [`definitions`], or `None` past `deadline` or [`MAX_SIDE_BYTES`].
fn definitions_until(
    lang: Lang,
    source: &str,
    deadline: Instant,
    each: usize,
) -> Option<Vec<Definition>> {
    let tree = parse_until(lang, source, deadline)?;
    definitions_of(lang, &tree, source, deadline, each)
}

/// The definitions in `tree`, or `None` past `deadline`, or once they hold
/// more than [`MAX_SIDE_BYTES`], each counting its qualified name, its name
/// and `each` bytes (what a symbol made of it keeps besides: its path).
fn definitions_of(
    lang: Lang,
    tree: &tree_sitter::Tree,
    source: &str,
    deadline: Instant,
    each: usize,
) -> Option<Vec<Definition>> {
    let mut out = Vec::new();
    let mut scope: Vec<String> = Vec::new();
    walk_definitions(
        lang,
        tree.root_node(),
        source,
        &mut scope,
        &mut out,
        Limits {
            deadline,
            each,
            bytes: MAX_SIDE_BYTES,
        },
    )?;
    Some(out)
}

/// When [`walk_definitions`] gives up.
#[derive(Debug, Clone, Copy)]
struct Limits {
    deadline: Instant,
    /// What each definition costs besides its names.
    each: usize,
    /// What the definitions may cost in all.
    bytes: usize,
}

/// Walks the tree in order with a stack of its own: deeply nested source
/// (2,000 `+` terms, or 20,000 parentheses) ran a recursive walk out of a
/// 2 MiB thread's stack, which aborts the app (the review of 9a4e93ab, M1).
/// Definitions past [`MAX_DEPTH`] aren't read, and the walk gives up
/// (`None`) at the deadline or past the bytes `limits` allow.
fn walk_definitions(
    lang: Lang,
    root: Node<'_>,
    source: &str,
    scope: &mut Vec<String>,
    out: &mut Vec<Definition>,
    limits: Limits,
) -> Option<()> {
    enum Step<'t> {
        Enter(Node<'t>),
        /// Leaving a node that pushed a name onto the scope.
        Leave,
    }
    let mut stack = vec![Step::Enter(root)];
    let mut steps: u32 = 0;
    let mut spent: usize = 0;
    while let Some(step) = stack.pop() {
        steps = steps.wrapping_add(1);
        if steps.is_multiple_of(1024) && Instant::now() > limits.deadline {
            return None;
        }
        let node = match step {
            Step::Leave => {
                scope.pop();
                continue;
            }
            Step::Enter(node) => node,
        };
        let mut pushed = false;
        let opens_scope =
            definition(lang, node).is_some() || (lang == Lang::Rust && node.kind() == "impl_item");
        if opens_scope && scope.len() >= MAX_DEPTH {
            continue;
        }
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
                // Names are pieces of the source, but each qualified name
                // repeats its whole scope.
                spent = spent
                    .saturating_add(qualified.len() + name.len())
                    .saturating_add(limits.each);
                if spent > limits.bytes {
                    return None;
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
        if pushed {
            stack.push(Step::Leave);
        }
        // Children last to first, so the first is read first.
        let mut cursor = node.walk();
        let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
        stack.extend(children.into_iter().rev().map(Step::Enter));
    }
    Some(())
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
    parse(lang, source)
        .map(|tree| uses_of(lang, &tree, source, lines))
        .unwrap_or_default()
}

/// [`uses`] in a tree already parsed.
fn uses_of(
    lang: Lang,
    tree: &tree_sitter::Tree,
    source: &str,
    lines: Option<&BTreeSet<u32>>,
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
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

/// The last part of a qualified name (`refresh` for `Session.refresh`).
fn last_of<'a>(qualified: &'a str, sep: &str) -> &'a str {
    qualified
        .rsplit_once(sep)
        .map_or(qualified, |(_, last)| last)
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
fn renames(
    lang: Lang,
    removed: &[&Definition],
    added: &[&Definition],
    deadline: Instant,
) -> Vec<(String, String)> {
    let sep = lang.separator();
    // Each one's depth, counted once: the sort counted both sides' again at
    // every comparison, a whole 200 KB name each time (the review of
    // d06b9e3d, MF-1).
    let mut by_depth: Vec<(usize, &Definition)> = removed
        .iter()
        .map(|d| (d.qualified.matches(sep).count(), *d))
        .collect();
    by_depth.sort_by(|(da, a), (db, b)| (da, &a.qualified).cmp(&(db, &b.qualified)));
    // Each side by what a pair shares, so a removed definition is checked
    // only against its likely matches: every removed one against every
    // added one, with a scope copied for each, took 17 s for 8,000
    // functions (the review of 30d5bcad, S-4). An added one's scope is
    // fixed; a removed one's is read through the renames found so far.
    let mut added_by: HashMap<(&str, u64), HashMap<&str, Vec<&Definition>>> = HashMap::new();
    for a in added {
        added_by
            .entry((a.kind, a.shape))
            .or_default()
            .entry(scope_of(&a.qualified, sep))
            .or_default()
            .push(a);
    }
    let mut found: BTreeMap<String, String> = BTreeMap::new();
    let mut taken: HashSet<&str> = HashSet::new();
    // The removed definitions of the depth being read still unpaired, by
    // kind, shape and scope (read through the renames found so far): a
    // rival is as deep as the one it rivals, since a scope's rename keeps
    // its depth. Counted as each depth begins, when the shallower names its
    // scopes are made of are settled. Each removed one was checked against
    // every other of its kind, shape and depth, quadratic for 4,000 classes
    // each with a method of one shape renamed (the review of d06b9e3d,
    // N-10).
    let mut rivals: HashMap<(&str, u64, String), usize> = HashMap::new();
    let mut level = None;
    for (n, (depth, r)) in by_depth.iter().enumerate() {
        // A rename is a bonus: past the deadline, the rest read as removed
        // and added.
        if n % 256 == 255 && Instant::now() > deadline {
            break;
        }
        if level != Some(*depth) {
            level = Some(*depth);
            rivals.clear();
            for (_, o) in by_depth[n..].iter().take_while(|(d, _)| d == depth) {
                let scope = mapped(scope_of(&o.qualified, sep), &found).to_string();
                *rivals.entry((o.kind, o.shape, scope)).or_default() += 1;
            }
        }
        let scope = mapped(scope_of(&r.qualified, sep), &found);
        let Some(bucket) = added_by
            .get(&(r.kind, r.shape))
            .and_then(|by_scope| by_scope.get(scope))
        else {
            continue;
        };
        let mut free = bucket
            .iter()
            .filter(|a| !taken.contains(a.qualified.as_str()));
        let (Some(a), None) = (free.next(), free.next()) else {
            continue;
        };
        // And `r` is the only removed one that `a` could pair with: the
        // only one unpaired under `a`'s scope (the bucket's).
        let key = (r.kind, r.shape, scope.to_string());
        if rivals.get(&key) != Some(&1) {
            continue;
        }
        // A member that moved with its renamed type keeps its name and its
        // text. A constructor, named after its type, takes the type's new
        // name and keeps the rest: short as it is, it moves with the type
        // (the review of 9a4e93ab, S5). Anything else needs enough text to
        // tell it apart.
        let moved = r.name == a.name;
        if moved && r.text != a.text {
            continue;
        }
        let names_its_type = |d: &Definition| {
            d.kind == "constructor"
                && scope_of(&d.qualified, sep).rsplit(sep).next() == Some(d.name.as_str())
        };
        if !moved && !(names_its_type(r) && names_its_type(a)) && r.size < MIN_RENAME_TEXT {
            continue;
        }
        if r.qualified != a.qualified {
            taken.insert(a.qualified.as_str());
            found.insert(r.qualified.clone(), a.qualified.clone());
            if let Some(left) = rivals.get_mut(&key) {
                *left -= 1;
            }
        }
    }
    found.into_iter().collect()
}

/// `scope` once the renames found so far are applied to it.
fn mapped<'a>(scope: &'a str, found: &'a BTreeMap<String, String>) -> &'a str {
    found.get(scope).map_or(scope, String::as_str)
}

/// A name's definitions as one, for [`renames`]: itself, or its overloads
/// together (their texts as a set); `None` when they're of different kinds.
fn as_one(defs: &[Definition]) -> Option<Definition> {
    let first = defs.first()?;
    if defs.len() == 1 {
        return Some(first.clone());
    }
    if defs.iter().any(|d| d.kind != first.kind) {
        return None;
    }
    let joined = |of: fn(&Definition) -> u64| {
        let mut all: Vec<u64> = defs.iter().map(of).collect();
        all.sort_unstable();
        let bytes: Vec<u8> = all.iter().flat_map(|h| h.to_le_bytes()).collect();
        hash(&bytes)
    };
    Some(Definition {
        text: joined(|d| d.text),
        shape: joined(|d| d.shape),
        size: defs.iter().map(|d| d.size).sum(),
        ..first.clone()
    })
}

/// What changed between a file's base and worktree versions; `None` when
/// either side's read was given up ([`PARSE_LIMIT`]), since a side read as
/// empty would have every definition added or removed.
pub fn compare(
    lang: Lang,
    path: &str,
    base: Option<&str>,
    now: Option<&str>,
) -> Option<Vec<ChangedSymbol>> {
    read_change(lang, path, base, now, None).map(|(symbols, _)| symbols)
}

/// [`compare`], and the names `now` uses on `lines` ([`uses`]) from the
/// same parse of it: each file was parsed a third time for its uses.
fn read_change(
    lang: Lang,
    path: &str,
    base: Option<&str>,
    now: Option<&str>,
    lines: Option<&BTreeSet<u32>>,
) -> Option<(Vec<ChangedSymbol>, BTreeSet<String>)> {
    // Each symbol made of a definition keeps a copy of the path.
    let each = path.len() + DEFINITION_BYTES;
    let before = match base {
        Some(s) => definitions_until(lang, s, Instant::now() + PARSE_LIMIT, each)?,
        None => Vec::new(),
    };
    let deadline = Instant::now() + PARSE_LIMIT;
    let (after, used) = match now {
        Some(s) => {
            let tree = parse_until(lang, s, deadline)?;
            let defs = definitions_of(lang, &tree, s, deadline, each)?;
            (defs, uses_of(lang, &tree, s, lines))
        }
        None => (Vec::new(), BTreeSet::new()),
    };
    Some((
        changed_between(lang, path, before, after, deadline + PARSE_LIMIT),
        used,
    ))
}

/// What changed from `before`'s definitions to `after`'s; renames are
/// looked for until `deadline`.
fn changed_between(
    lang: Lang,
    path: &str,
    before: Vec<Definition>,
    after: Vec<Definition>,
    deadline: Instant,
) -> Vec<ChangedSymbol> {
    let index = |defs: Vec<Definition>| {
        let mut by: BTreeMap<String, Vec<Definition>> = BTreeMap::new();
        for d in defs {
            by.entry(d.qualified.clone()).or_default().push(d);
        }
        by
    };
    let (before, after) = (index(before), index(after));
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
    // Renames pair what a name defines on one side with what another
    // defines on the other: one definition, or a name's overloads together.
    // Overloads never paired, so a renamed class's overloaded constructors
    // read as removed (the review of 30d5bcad, S-1).
    let removed: Vec<Definition> = before
        .iter()
        .filter(|(q, _)| !after.contains_key(*q))
        .filter_map(|(_, v)| as_one(v))
        .collect();
    let added: Vec<Definition> = after
        .iter()
        .filter(|(q, _)| !before.contains_key(*q))
        .filter_map(|(_, v)| as_one(v))
        .collect();
    let renamed: BTreeMap<String, String> = renames(
        lang,
        &removed.iter().collect::<Vec<_>>(),
        &added.iter().collect::<Vec<_>>(),
        deadline,
    )
    .into_iter()
    .collect();
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

/// The type `s` was renamed with, when `s` is a member that moved only
/// because its type was renamed: it kept its own name (`Session.refresh` →
/// `UserSession.refresh`), or it's a constructor named after its type that
/// took the type's new name. A member renamed on its own as well
/// (`Session.refresh` → `UserSession.reload`) has a rename of its own to
/// tell (the review of d06b9e3d, SF-2).
pub fn renamed_with_its_type<'a>(
    all: &'a [ChangedSymbol],
    s: &ChangedSymbol,
) -> Option<&'a ChangedSymbol> {
    let to = s.renamed_to.as_deref()?;
    let sep = Lang::of(&s.path)?.separator();
    let (from, onto) = (scope_of(&s.qualified, sep), scope_of(to, sep));
    if from.is_empty() {
        return None;
    }
    let name = last_of(&s.qualified, sep);
    let kept = name == last_of(to, sep);
    let constructor = s.kind == "constructor"
        && name == last_of(from, sep)
        && last_of(to, sep) == last_of(onto, sep);
    if !kept && !constructor {
        return None;
    }
    all.iter()
        .find(|t| t.qualified == from && t.renamed_to.as_deref() == Some(onto))
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

/// The base's blob sizes at `paths` (none where it has no file), from one
/// `git cat-file --batch-check`, so a blob past the limit is never read:
/// `git show` read the whole of one the worktree had deleted, however big
/// (the review of 9a4e93ab).
async fn base_sizes(
    git: &Git,
    root: &Path,
    base: &str,
    paths: &[&str],
) -> Result<HashMap<String, u64>, GitError> {
    // An object a line: a path with a line break in it can't be asked for
    // in the batch. Those are asked one at a time; left out, they read as
    // new files, every definition added (the review of 30d5bcad). One
    // ending in a carriage return, which git would strip, never gets here:
    // its extension ends in one too, so it isn't parsed.
    let (odd, asked): (Vec<&str>, Vec<&str>) =
        paths.iter().copied().partition(|p| p.contains('\n'));
    let mut sizes = HashMap::new();
    if !asked.is_empty() {
        let input: String = asked.iter().map(|p| format!("{base}:{p}\n")).collect();
        let out = git
            .ok_with_input(
                Mode::Read,
                root,
                &[
                    OsStr::new("cat-file"),
                    OsStr::new("--batch-check=%(objecttype) %(objectsize)"),
                ],
                input.as_bytes(),
            )
            .await?;
        // One line an object, in order: "blob 1234", or "<name> missing".
        let text = out.text();
        sizes.extend(asked.iter().zip(text.lines()).filter_map(|(path, line)| {
            let size = line.strip_prefix("blob ")?.parse().ok()?;
            Some((path.to_string(), size))
        }));
    }
    for path in odd {
        if let Some(size) = blob_size(git, root, base, path).await? {
            sizes.insert(path.to_string(), size);
        }
    }
    Ok(sizes)
}

/// The size of the base's blob at `path`, asked on its own; `None` when the
/// base has no file there.
async fn blob_size(
    git: &Git,
    root: &Path,
    base: &str,
    path: &str,
) -> Result<Option<u64>, GitError> {
    let spec = format!("{base}:{path}");
    let ask = |what: &'static str| {
        [
            OsStr::new("cat-file"),
            OsStr::new(what),
            OsStr::new(spec.as_str()),
        ]
    };
    let kind = git.run(Mode::Read, root, &ask("-t")).await?;
    if !kind.success() || kind.text().trim() != "blob" {
        return Ok(None);
    }
    let size = git.run(Mode::Read, root, &ask("-s")).await?;
    Ok(size
        .success()
        .then(|| size.text().trim().parse().ok())
        .flatten())
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
    /// When its read was given up: it's tried again after
    /// [`RETRY_GIVEN_UP`].
    given_up: Option<Instant>,
}

impl Parsed {
    /// Read and nothing kept: unparsed, and tried again after
    /// [`RETRY_GIVEN_UP`].
    fn given_up(content: Option<u64>) -> Self {
        Self {
            content,
            symbols: Vec::new(),
            uses: BTreeSet::new(),
            unparsed: true,
            given_up: Some(Instant::now()),
        }
    }

    /// What it adds to an index, for [`MAX_INDEX_BYTES`], as it sits in
    /// memory: each symbol's struct and its strings with their heap
    /// blocks, each name in use with its own, twice over, since the index
    /// and the parse cache it's read from each hold a copy.
    fn bytes(&self) -> usize {
        let text = |t: &String| t.len() + STRING_HEAP;
        let symbols = self.symbols.iter().map(|s| {
            std::mem::size_of::<ChangedSymbol>()
                + text(&s.name)
                + text(&s.qualified)
                + text(&s.kind)
                + text(&s.path)
                + s.renamed_to.as_ref().map_or(0, text)
        });
        let uses = self
            .uses
            .iter()
            .map(|u| std::mem::size_of::<String>() + text(u) + USE_NODE);
        2 * symbols.chain(uses).sum::<usize>()
    }
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
    /// Files were left for the next refresh: this one ran out of time
    /// ([`REFRESH_BUDGET`]). Nothing else would read them while the
    /// worktree stays as it is (the review of d06b9e3d, N-2).
    pub more: bool,
}

/// The worktree file's text, or `None` when it's gone (`Err(())`: too big,
/// not a regular file, not UTF-8, or unreadable, so it isn't parsed). Only
/// a file that isn't there is missing: a link loop, a permission or a full
/// file table read as deleted, every definition removed (the review of
/// 30d5bcad). A link isn't followed and a pipe isn't waited on:
/// `metadata` followed a link to a FIFO, whose read then blocked for good,
/// or to `/dev/zero`, read until memory ran out (the review of 9a4e93ab,
/// M2). The read stops past the limit should the file grow meanwhile.
fn read_file(path: &Path) -> Result<Option<String>, ()> {
    use std::io::{ErrorKind, Read};
    let gone =
        |e: &std::io::Error| matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory);
    match std::fs::symlink_metadata(path) {
        Err(e) if gone(&e) => return Ok(None),
        Err(_) => return Err(()),
        Ok(m) if !m.is_file() || m.len() > MAX_FILE_BYTES as u64 => return Err(()),
        Ok(_) => {}
    }
    let file = match open_file(path) {
        Ok(file) => file,
        Err(e) if gone(&e) => return Ok(None),
        Err(_) => return Err(()),
    };
    // Swapped for something else since: still only a regular file.
    if !file.metadata().is_ok_and(|m| m.is_file()) {
        return Err(());
    }
    let mut bytes = Vec::new();
    if file
        .take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return Err(());
    }
    if bytes.len() > MAX_FILE_BYTES {
        return Err(());
    }
    String::from_utf8(bytes).map(Some).map_err(|_| ())
}

/// Opens `path` for reading, not through a link and without waiting on a
/// pipe's writer.
#[cfg(unix)]
fn open_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

/// Opens `path` for reading, not through a link: a link swapped in after
/// the check opens as itself, which isn't a regular file. Any other
/// reparse point is the file itself (a OneDrive placeholder, a
/// deduplicated file), and opened as itself it reads as its stub, every
/// definition removed: that one is opened again, normally (the review of
/// d06b9e3d, N-7), and kept only when it's the same file. Opened by its
/// name, the second open followed a link swapped in between the two (the
/// review of d3b838de). The standard library gives no file id on Windows,
/// so the file's creation time and size stand for it: a file the link
/// leads to has its own, unless whoever swapped it set them, which takes a
/// file they could write anyway.
#[cfg(windows)]
fn open_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let meta = file.metadata()?;
    // A link is a name surrogate, which `is_symlink` reads from the tag.
    if meta.file_type().is_symlink() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
        return Ok(file);
    }
    let again = std::fs::OpenOptions::new().read(true).open(path)?;
    let now = again.metadata()?;
    let same = |m: &std::fs::Metadata| (m.creation_time(), m.file_size());
    if !now.is_file() || same(&now) != same(&meta) {
        return Err(std::io::Error::other("the file changed as it was opened"));
    }
    Ok(again)
}

#[cfg(not(any(unix, windows)))]
fn open_file(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
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
    index_within(git, root, base, cache, REFRESH_BUDGET, MAX_INDEX_BYTES).await
}

/// [`index_cached`], starting no file past `budget` but the first (so each
/// refresh gets further), with an index of at most `bytes`.
async fn index_within(
    git: &Git,
    root: &Path,
    base: &str,
    cache: &mut ParseCache,
    budget: Duration,
    bytes: usize,
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
    let started = Instant::now();
    // What the index holds so far, for `bytes`.
    let mut held: usize = 0;
    // The base's sizes, asked at the first file parsed for those after it.
    let mut sizes: Option<HashMap<String, u64>> = None;
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
        // A read that panicked or was cancelled is unreadable, not a
        // deleted file with every definition removed (the review of
        // d06b9e3d, N-4).
        let read = tokio::task::spawn_blocking(move || read_file(&full))
            .await
            .unwrap_or(Err(()));
        let Ok(now) = read else {
            out.index.unparsed += 1;
            continue;
        };
        let content = now.as_ref().map(|t| hash(t.as_bytes()));
        let fresh = |p: &Parsed| {
            p.content == content && p.given_up.is_none_or(|at| at.elapsed() < RETRY_GIVEN_UP)
        };
        let parsed = match cache.files.get(path) {
            Some(p) if fresh(p) => p.clone(),
            // Past the budget: read at the next refresh, not kept now.
            _ if out.parsed > 0 && started.elapsed() > budget => {
                out.index.unparsed += 1;
                out.more = true;
                continue;
            }
            _ => {
                out.parsed += 1;
                if sizes.is_none() {
                    let rest: Vec<&str> = files
                        .iter()
                        .take(MAX_FILES)
                        .skip(n)
                        .map(String::as_str)
                        .filter(|p| Lang::of(p).is_some())
                        .collect();
                    sizes = Some(base_sizes(git, root, base, &rest).await?);
                }
                let size = sizes.as_ref().and_then(|s| s.get(path).copied());
                if size.is_some_and(|s| s > MAX_FILE_BYTES as u64) {
                    Parsed {
                        content,
                        symbols: Vec::new(),
                        uses: BTreeSet::new(),
                        unparsed: true,
                        given_up: None,
                    }
                } else {
                    // No blob in the base (an added file): nothing to read.
                    let was = match size {
                        Some(_) => show(git, root, base, path).await?,
                        None => None,
                    };
                    let lines = match (&was, &now) {
                        (Some(_), Some(_)) => {
                            Some(added_lines(&diff(git, root, base, path).await?))
                        }
                        _ => None,
                    };
                    let path_owned = path.clone();
                    let done = tokio::task::spawn_blocking(move || {
                        read_change(
                            lang,
                            &path_owned,
                            was.as_deref(),
                            now.as_deref(),
                            lines.as_ref(),
                        )
                    })
                    .await
                    .ok()
                    .flatten();
                    match done {
                        Some((symbols, used)) => Parsed {
                            content,
                            symbols,
                            uses: used,
                            unparsed: false,
                            given_up: None,
                        },
                        // Given up: unparsed, not every definition removed.
                        None => Parsed::given_up(content),
                    }
                }
            }
        };
        // Past what an index holds: unparsed, and read again later.
        let size = parsed.bytes();
        let parsed = if held.saturating_add(size) > bytes {
            Parsed::given_up(content)
        } else {
            held += size;
            parsed
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

/// What `of` did to each name, its strongest change: the symbol. Of two as
/// strong, the outer one (a type rather than its constructor), and of two
/// alike the last. In one pass: each name was looked up through every
/// symbol, 256 million comparisons a pair for 16,000 changed symbols (the
/// review of d06b9e3d, SF-1).
fn changes_by_name(of: &SymbolIndex) -> HashMap<&str, &ChangedSymbol> {
    let rank = |s: &ChangedSymbol| (s.change.weight(), std::cmp::Reverse(s.qualified.len()));
    let mut out: HashMap<&str, &ChangedSymbol> = HashMap::new();
    for s in &of.symbols {
        match out.get(s.name.as_str()) {
            Some(best) if rank(best) > rank(s) => {}
            _ => {
                out.insert(s.name.as_str(), s);
            }
        }
    }
    out
}

fn touches(c: SymbolChange) -> bool {
    matches!(
        c,
        SymbolChange::Changed | SymbolChange::Removed | SymbolChange::Renamed
    )
}

/// An index read once for comparing with others: what it did to each name
/// ([`changes_by_name`]). The rail's marks compare every pair of a
/// repository's worktrees, and built both maps again for each (the review
/// of d3b838de).
pub struct Comparable<'a> {
    index: &'a SymbolIndex,
    by_name: HashMap<&'a str, &'a ChangedSymbol>,
}

impl<'a> Comparable<'a> {
    pub fn new(index: &'a SymbolIndex) -> Self {
        Self {
            index,
            by_name: changes_by_name(index),
        }
    }
}

/// The symbols `here` and `there` share: both changed or removed a
/// definition of the name, or one changed or removed it and the other's
/// new lines use it.
pub fn shared_symbols(here: &SymbolIndex, there: &SymbolIndex) -> Vec<SharedSymbol> {
    shared_between(&Comparable::new(here), &Comparable::new(there))
}

/// [`shared_symbols`] of two indexes each read once.
pub fn shared_between(here: &Comparable<'_>, there: &Comparable<'_>) -> Vec<SharedSymbol> {
    let (mine_by, theirs_by) = (&here.by_name, &there.by_name);
    let (here, there) = (here.index, there.index);
    let names: BTreeSet<&str> = mine_by
        .keys()
        .chain(theirs_by.keys())
        .copied()
        .filter(|n| n.chars().count() >= MIN_NAME)
        .collect();
    let mut out = Vec::new();
    for name in names {
        let mine = mine_by.get(name).copied();
        let theirs = theirs_by.get(name).copied();
        let pair = match (mine, theirs) {
            (Some(m), Some(t)) if touches(m.change) && touches(t.change) => {
                // Renamed differently on each side: neither name is both's
                // (one field said "renamed to X" of each; the review of
                // 9a4e93ab).
                let renamed_to = match (&m.renamed_to, &t.renamed_to) {
                    (Some(a), Some(b)) if a != b => None,
                    (a, b) => a.clone().or(b.clone()),
                };
                Some((m, m.change, t.change, renamed_to))
            }
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
    fn f_wt_10_2_deeply_nested_source_is_read_on_a_small_stack() {
        // Each of these ran a recursive walk out of a 2 MiB thread's stack,
        // which aborts the app (the review of 9a4e93ab, M1). The radar parses
        // on tokio's blocking threads, which have 2 MiB.
        let deep = |n: usize| format!("{}1{}", "(".repeat(n), ")".repeat(n));
        let cases = vec![
            (
                Lang::TypeScript,
                format!("export const s = {};\n", vec!["\"ab\""; 2_000].join(" + ")),
            ),
            (
                Lang::TypeScript,
                format!("export const c = a{};\n", ".b()".repeat(5_000)),
            ),
            (Lang::TypeScript, format!("const x = {};\n", deep(100_000))),
            (Lang::Python, format!("x = {}\n", deep(100_000))),
            (
                Lang::Rust,
                format!("fn f() {{ let x = {}; }}\n", deep(100_000)),
            ),
            (Lang::Go, format!("package p\nvar x = {}\n", deep(20_000))),
            (
                Lang::Java,
                format!("class A {{ int x = {}; }}\n", deep(20_000)),
            ),
        ];
        std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(move || {
                for (lang, source) in cases {
                    assert!(source.len() <= MAX_FILE_BYTES, "{lang:?}");
                    // Read, not given up: a parse past its time would
                    // never reach the walk (the review of 30d5bcad).
                    let read = definitions_until(
                        lang,
                        &source,
                        Instant::now() + Duration::from_secs(120),
                        DEFINITION_BYTES,
                    );
                    assert!(read.is_some(), "{lang:?}");
                }
            })
            .expect("a thread")
            .join()
            .expect("read without running out of stack");
    }

    #[test]
    fn f_wt_10_5_a_generic_receiver_names_its_type() {
        // The type's arguments came first: `T.Handle`, and two types'
        // `Close` methods merged into one `T.Close` (the review of
        // 9a4e93ab, S4).
        let go = "package shop\n\ntype Server[T any] struct{ v T }\n\nfunc (s *Server[T]) Handle() {}\n\nfunc (s Server[T]) Close() {}\n\ntype Pair[K comparable, V any] struct{}\n\nfunc (p Pair[K, V]) Get() {}\n\nfunc (p *Pair[K, V]) Close() {}\n\nfunc (Pair[K, V]) Size() int { return 0 }\n";
        assert_eq!(
            names(&definitions(Lang::Go, go)),
            [
                ("Server", "struct"),
                ("Server.Handle", "method"),
                ("Server.Close", "method"),
                ("Pair", "struct"),
                ("Pair.Get", "method"),
                ("Pair.Close", "method"),
                ("Pair.Size", "method"),
            ]
        );
    }

    #[test]
    fn f_wt_10_6_a_java_constructor_moves_with_its_renamed_class() {
        use SymbolChange::*;
        // Too short to pair on its own, the constructor read as removed, and
        // `Session.Session` removed outranked the class's rename in what a
        // worktree using `Session` was told (the review of 9a4e93ab, S5).
        let base = "class Session {\n  public Session() {}\n  void refresh() { count++; }\n}\n";
        let now =
            "class UserSession {\n  public UserSession() {}\n  void refresh() { count++; }\n}\n";
        let got = changes(Lang::Java, base, now);
        for (from, to) in [
            ("Session", "UserSession"),
            ("Session.Session", "UserSession.UserSession"),
            ("Session.refresh", "UserSession.refresh"),
        ] {
            assert!(
                got.contains(&(from.to_string(), Renamed, Some(to.to_string()))),
                "{from}: {got:?}"
            );
        }
        let here = SymbolIndex {
            symbols: compare(Lang::Java, "Session.java", Some(base), Some(now)).expect("parses"),
            ..SymbolIndex::default()
        };
        let there = SymbolIndex {
            uses: BTreeSet::from(["Session".to_string()]),
            ..SymbolIndex::default()
        };
        let shared = shared_symbols(&here, &there);
        let session = shared.iter().find(|s| s.name == "Session").expect("shared");
        assert_eq!(
            (session.qualified.as_str(), session.here, session.there),
            ("Session", Renamed, Used)
        );
        assert_eq!(session.renamed_to.as_deref(), Some("UserSession"));
    }

    #[test]
    fn f_wt_10_6_overloaded_constructors_move_with_their_renamed_class() {
        use SymbolChange::*;
        // A name defined more than once never paired, so a class with two
        // constructors read as `Session.Session` removed (the review of
        // 30d5bcad, S-1); overloads pair as a whole.
        let base = "class Session {\n  Session() { this.n = 0; }\n  Session(int n) { this.n = n; }\n  void at(int i) { use(i); }\n  void at(String s) { use(s); }\n}\n";
        let now = "class UserSession {\n  UserSession() { this.n = 0; }\n  UserSession(int n) { this.n = n; }\n  void at(int i) { use(i); }\n  void at(String s) { use(s); }\n}\n";
        let got = changes(Lang::Java, base, now);
        for (from, to) in [
            ("Session", "UserSession"),
            ("Session.Session", "UserSession.UserSession"),
            ("Session.at", "UserSession.at"),
        ] {
            assert!(
                got.contains(&(from.to_string(), Renamed, Some(to.to_string()))),
                "{from}: {got:?}"
            );
        }
        assert!(got.iter().all(|(_, c, _)| *c != Removed), "{got:?}");
        // One overload edited as well: not a rename.
        let edited = now.replace("this.n = n;", "this.n = n + 1;");
        let got = changes(Lang::Java, base, &edited);
        assert!(
            got.contains(&("Session.Session".to_string(), Removed, None)),
            "{got:?}"
        );
    }

    #[test]
    fn f_wt_10_2_definitions_nested_thousands_deep_are_read_to_a_limit() {
        // 16,000 nested definitions took 64 to 142 s and 384 MB of names in
        // a debug build: each level joined its whole scope and copied and
        // hashed all the text inside it (the review of 30d5bcad, M-1).
        let deep =
            |open: &str, close: &str| format!("{}{}", open.repeat(16_000), close.repeat(16_000));
        let cases = [
            (Lang::Rust, deep("mod a{", "}")),
            (Lang::TypeScript, deep("namespace a{", "}")),
            (Lang::TypeScript, deep("function a(){", "}")),
            (Lang::Java, deep("class a{", "}")),
        ];
        for (lang, source) in cases {
            assert!(source.len() <= MAX_FILE_BYTES, "{lang:?}");
            let started = Instant::now();
            let defs = definitions_until(
                lang,
                &source,
                started + Duration::from_secs(120),
                DEFINITION_BYTES,
            )
            .expect("read");
            assert_eq!(defs.len(), MAX_DEPTH, "{lang:?}");
            let renamed = source.replacen(" a", " b", 1).replacen("a{", "b{", 1);
            let read = read_change(lang, "deep", Some(&source), Some(&renamed), None);
            let elapsed = started.elapsed();
            assert!(read.is_some(), "{lang:?}");
            assert!(elapsed < Duration::from_secs(30), "{lang:?}: {elapsed:?}");
        }
    }

    #[test]
    fn f_wt_10_6_thousands_of_look_alike_renames_are_read_quickly() {
        use SymbolChange::*;
        // Every function renamed, all with one shape: every removed one was
        // compared with every added one, a scope copied each time, 17 s for
        // 8,000 in a debug build (the review of 30d5bcad, S-4). None pairs:
        // each could be any of the others.
        let n = 8_000;
        let file = |prefix: &str| -> String {
            (0..n)
                .map(|i| format!("function {prefix}{i}(a) {{ return a + 1; }}\n"))
                .collect()
        };
        let (base, now) = (file("f"), file("g"));
        let started = Instant::now();
        let got = compare(Lang::TypeScript, "many.ts", Some(&base), Some(&now)).expect("read");
        let elapsed = started.elapsed();
        assert_eq!(got.len(), 2 * n);
        assert!(got.iter().all(|s| matches!(s.change, Added | Removed)));
        assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
    }

    #[test]
    fn f_wt_10_2_a_parse_past_its_time_is_given_up() {
        // A 512 KiB file of noise took 5 to 7 s in a debug build; a parse
        // past the limit is `None`, so the file counts as unparsed.
        let big: String = (0..20_000)
            .map(|i| format!("function f{i}(a: number) {{ return a + {i}; }}\n"))
            .collect();
        assert!(parse_within(Lang::TypeScript, &big, Duration::ZERO).is_none());
        assert!(parse_within(Lang::TypeScript, &big, PARSE_LIMIT).is_some());
    }

    #[test]
    fn f_wt_10_2_added_changed_and_removed() {
        let was =
            "def keep():\n    return 1\n\ndef edit():\n    return 1\n\ndef gone():\n    pass\n";
        let now =
            "def keep():\n    return 1\n\ndef edit():\n    return 2\n\ndef fresh():\n    pass\n";
        let mut got: Vec<(String, SymbolChange)> =
            compare(Lang::Python, "m.py", Some(was), Some(now))
                .expect("parses")
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
            .expect("parses")
            .iter()
            .all(|s| s.change == SymbolChange::Added));
        assert!(compare(Lang::Python, "m.py", Some(was), None)
            .expect("parses")
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
            .expect("parses")
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
        // A name inside a longer word isn't set aside: `parse` in `parser`,
        // and the second `ab` in `abab`, which follows a `b`.
        assert_eq!(without_name("parse(parser)", "parse"), "\u{0}(parser)");
        assert_eq!(without_name("abab ab(ab)", "ab"), "abab \u{0}(\u{0})");
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

    #[test]
    fn f_wt_10_6_renamed_differently_on_each_side_names_neither() {
        use SymbolChange::*;
        let renamed = |to: &str| {
            let mut i = idx(&[("parse", Renamed)], &[]);
            i.symbols[0].renamed_to = Some(to.into());
            i
        };
        // One field said "renamed to parseLine" of both sides.
        let got = shared_symbols(&renamed("parseLine"), &renamed("parseInput"));
        assert_eq!((got[0].here, got[0].there), (Renamed, Renamed));
        assert_eq!(got[0].renamed_to, None);
        assert_eq!(got[0].display(), "parse");
        // The same new name on both sides is kept.
        let got = shared_symbols(&renamed("parseLine"), &renamed("parseLine"));
        assert_eq!(got[0].display(), "parse → parseLine");
    }

    /// A definition for [`renames`], with a text of its own `shape`.
    fn def(qualified: &str, kind: &'static str, shape: u64) -> Definition {
        Definition {
            name: last_of(qualified, ".").to_string(),
            qualified: qualified.to_string(),
            kind,
            line: 1,
            text: shape,
            shape,
            size: MIN_RENAME_TEXT,
        }
    }

    fn pair_up(removed: &[Definition], added: &[Definition], deadline: Instant) -> usize {
        renames(
            Lang::TypeScript,
            &removed.iter().collect::<Vec<_>>(),
            &added.iter().collect::<Vec<_>>(),
            deadline,
        )
        .len()
    }

    #[test]
    fn f_wt_10_2_a_long_scope_name_with_many_children_is_given_up() {
        // Each child's qualified name repeated a scope of 200,000 letters:
        // about 10 GB from a changed file of 500 KB, two levels deep (the
        // review of d06b9e3d, MF-1). Past what one side may hold, the file
        // counts as unparsed.
        let long = "a".repeat(200_000);
        let cases = [
            (
                Lang::Rust,
                format!("mod {long} {{{}}}\n", "mod b;".repeat(50_000)),
            ),
            (
                Lang::TypeScript,
                format!("namespace {long} {{{}}}\n", "type b=1;".repeat(34_000)),
            ),
            (
                Lang::Python,
                format!("class {long}:\n{}", "  def b():0\n".repeat(26_000)),
            ),
        ];
        for (lang, source) in cases {
            assert!(source.len() <= MAX_FILE_BYTES, "{lang:?}");
            let deadline = Instant::now() + Duration::from_secs(120);
            assert!(
                definitions_until(lang, &source, deadline, DEFINITION_BYTES).is_none(),
                "{lang:?}"
            );
            assert!(
                compare(lang, "evil", None, Some(&source)).is_none(),
                "{lang:?}"
            );
        }
        // A long name with a few children is read.
        let fine = format!("mod {} {{{}}}\n", "a".repeat(300), "mod b;".repeat(1_000));
        assert_eq!(definitions(Lang::Rust, &fine).len(), 1_001);
    }

    #[test]
    fn f_wt_10_6_renames_pair_by_index_not_by_comparing_every_pair() {
        // 100,000 functions renamed, each with a text of its own. Compared
        // every removed one with every added one, that's 10 billion
        // comparisons: past the deadline however fast the machine, where
        // the test above only times 8,000 (the review of d06b9e3d, N-9).
        let n = 100_000;
        let removed: Vec<Definition> = (0..n)
            .map(|i| def(&format!("f{i}"), "function", i))
            .collect();
        let added: Vec<Definition> = (0..n)
            .map(|i| def(&format!("g{i}"), "function", i))
            .collect();
        let deadline = Instant::now() + Duration::from_secs(10);
        assert_eq!(pair_up(&removed, &added, deadline), 100_000);
    }

    #[test]
    fn f_wt_10_6_look_alike_methods_in_many_classes_pair_in_one_pass() {
        // 20,000 classes each renamed a method, every method of one shape.
        // Each removed one was checked against every other of its shape
        // and depth for a rival, 400 million times (the review of
        // d06b9e3d, N-10). Each pairs in its own class.
        let n = 20_000;
        let removed: Vec<Definition> = (0..n)
            .map(|i| def(&format!("C{i}.old"), "method", 7))
            .collect();
        let added: Vec<Definition> = (0..n)
            .map(|i| def(&format!("C{i}.new"), "method", 7))
            .collect();
        let deadline = Instant::now() + Duration::from_secs(10);
        assert_eq!(pair_up(&removed, &added, deadline), 20_000);
        // Two in one class could each be the one renamed: neither pairs.
        let removed = [def("C.a1", "method", 7), def("C.a2", "method", 7)];
        let added = [def("C.b1", "method", 7)];
        assert_eq!(pair_up(&removed, &added, deadline), 0);
    }

    #[test]
    fn f_wt_10_2_the_walk_and_the_renames_stop_at_their_deadlines() {
        let source: String = (0..2_000)
            .map(|i| format!("function f{i}() {{}}\n"))
            .collect();
        let tree = parse(Lang::TypeScript, &source).expect("parses");
        let later = Instant::now() + Duration::from_secs(60);
        let read =
            |deadline| definitions_of(Lang::TypeScript, &tree, &source, deadline, DEFINITION_BYTES);
        assert_eq!(read(later).map(|d| d.len()), Some(2_000));
        // Checked every 1,024 nodes: given up at the first check.
        assert!(read(Instant::now()).is_none());
        // Renames are checked every 256: the first 255 are kept.
        let removed: Vec<Definition> = (0..1_000)
            .map(|i| def(&format!("f{i}"), "function", i))
            .collect();
        let added: Vec<Definition> = (0..1_000)
            .map(|i| def(&format!("g{i}"), "function", i))
            .collect();
        assert_eq!(pair_up(&removed, &added, later), 1_000);
        assert_eq!(pair_up(&removed, &added, Instant::now()), 255);
    }

    #[test]
    fn f_wt_10_1_what_big_indexes_share_is_read_in_one_pass() {
        use SymbolChange::*;
        // 50,000 changed symbols a side: each name was looked up through
        // every symbol, 10 billion comparisons (the review of d06b9e3d,
        // SF-1).
        let names: Vec<String> = (0..50_000).map(|i| format!("name{i}")).collect();
        let side: Vec<(&str, SymbolChange)> = names.iter().map(|n| (n.as_str(), Changed)).collect();
        let (here, there) = (idx(&side, &[]), idx(&side, &[]));
        let started = Instant::now();
        assert_eq!(shared_symbols(&here, &there).len(), 50_000);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        // Of two changes as strong and as long, the last is the one.
        let mut two = idx(&[("parse", Changed), ("parse", Changed)], &[]);
        two.symbols[0].qualified = "a.parse".into();
        two.symbols[1].qualified = "b.parse".into();
        let got = shared_symbols(&two, &idx(&[("parse", Changed)], &[]));
        assert_eq!(got[0].qualified, "b.parse");
    }

    #[test]
    fn f_wt_10_8_a_member_renamed_with_its_type_says_so_only_when_it_kept_its_name() {
        use SymbolChange::*;
        let renamed = |from: &str, to: &str, kind: &str| ChangedSymbol {
            name: last_of(from, ".").to_string(),
            qualified: from.to_string(),
            kind: kind.to_string(),
            path: "Session.java".into(),
            line: 1,
            change: Renamed,
            renamed_to: Some(to.to_string()),
        };
        let all = vec![
            renamed("Session", "UserSession", "class"),
            renamed("Session.refresh", "UserSession.refresh", "method"),
            renamed("Session.Session", "UserSession.UserSession", "constructor"),
            renamed("Session.close", "UserSession.shut", "method"),
        ];
        let with = |s: &ChangedSymbol| renamed_with_its_type(&all, s).map(|t| t.qualified.as_str());
        assert_eq!(with(&all[0]), None);
        assert_eq!(with(&all[1]), Some("Session"));
        assert_eq!(with(&all[2]), Some("Session"));
        // Renamed on its own as well: its own rename is told (the review of
        // d06b9e3d, SF-2).
        assert_eq!(with(&all[3]), None);
    }

    /// Runs git in `dir`, outside the hardened runner, for a fixture.
    fn git_in(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", dir.join(".no-global-config"))
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A repository with one commit, its base, and git to read it.
    fn repo() -> (tempfile::TempDir, std::path::PathBuf, String, Git) {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path().join("repo");
        std::fs::create_dir_all(&root).expect("mkdir");
        git_in(&root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("README"), "base\n").expect("write");
        git_in(&root, &["add", "-A"]);
        git_in(&root, &["commit", "-qm", "base"]);
        let base = git_in(&root, &["rev-parse", "HEAD"]);
        let git = Git::locate(None, tmp.path().join("empty-hooks")).expect("git");
        (tmp, root, base, git)
    }

    #[tokio::test]
    async fn f_wt_10_2_a_refresh_out_of_time_leaves_the_rest_for_the_next() {
        let (_tmp, root, base, git) = repo();
        for name in ["a", "b", "c"] {
            std::fs::write(
                root.join(format!("{name}.ts")),
                format!("export function {name}{name}{name}() {{}}\n"),
            )
            .expect("write");
        }
        let mut cache = ParseCache::default();
        let mut refresh = async || {
            index_within(
                &git,
                &root,
                &base,
                &mut cache,
                Duration::ZERO,
                MAX_INDEX_BYTES,
            )
            .await
            .expect("index")
        };
        // Out of time from the start: one file a refresh, each further.
        for n in 1..=3 {
            let got = refresh().await;
            assert_eq!(got.parsed, 1, "refresh {n}");
            assert_eq!(got.index.symbols.len(), n, "refresh {n}");
            assert_eq!(got.index.unparsed as usize, 3 - n, "refresh {n}");
            assert_eq!(got.more, n < 3, "refresh {n}");
        }
        let got = refresh().await;
        assert_eq!((got.parsed, got.more), (0, false));
    }

    #[tokio::test]
    async fn f_wt_10_2_an_index_past_its_size_reads_the_rest_later() {
        let (_tmp, root, base, git) = repo();
        std::fs::write(root.join("a.ts"), "export function alpha() {}\n").expect("write");
        std::fs::write(root.join("b.ts"), "export function beta() {}\n").expect("write");
        let mut cache = ParseCache::default();
        let budget = Duration::from_secs(60);
        // Nothing fits: both unparsed, and not parsed again at once.
        for _ in 0..2 {
            let got = index_within(&git, &root, &base, &mut cache, budget, 1)
                .await
                .expect("index");
            assert!(got.index.symbols.is_empty());
            assert_eq!(got.index.unparsed, 2);
        }
        let got = index_within(&git, &root, &base, &mut cache, budget, MAX_INDEX_BYTES)
            .await
            .expect("index");
        assert_eq!(
            (got.parsed, got.index.unparsed),
            (0, 2),
            "kept until the retry"
        );
        // Given up long enough ago: parsed again.
        let Some(long_ago) = Instant::now().checked_sub(RETRY_GIVEN_UP + Duration::from_secs(1))
        else {
            return;
        };
        for p in cache.files.values_mut() {
            p.given_up = Some(long_ago);
        }
        let got = index_within(&git, &root, &base, &mut cache, budget, MAX_INDEX_BYTES)
            .await
            .expect("index");
        assert_eq!((got.parsed, got.index.unparsed), (2, 0));
        assert_eq!(got.index.symbols.len(), 2);
    }

    #[test]
    fn f_wt_10_2_an_index_counts_its_symbols_as_they_sit_in_memory() {
        // Names alone put 75 MB of symbols, 150 MB with the parse cache,
        // under 32 MiB (the review of d3b838de, SF-C).
        let one = |name: &str| ChangedSymbol {
            name: name.into(),
            qualified: name.into(),
            kind: "function".into(),
            path: "a.ts".into(),
            line: 1,
            change: SymbolChange::Changed,
            renamed_to: None,
        };
        let parsed = |symbols: Vec<ChangedSymbol>, uses: &[&str]| Parsed {
            content: None,
            symbols,
            uses: uses.iter().map(|u| u.to_string()).collect(),
            unparsed: false,
            given_up: None,
        };
        let single = parsed(vec![one("a")], &[]).bytes();
        // Its struct and four heap strings, in the index and in the cache.
        assert!(
            single >= 2 * (std::mem::size_of::<ChangedSymbol>() + 4 * STRING_HEAP),
            "{single}"
        );
        assert_eq!(parsed(vec![one("a"), one("b")], &[]).bytes(), 2 * single);
        assert!(parsed(vec![one("a")], &["b"]).bytes() > single);
        // 300,000 small symbols no longer fit in 32 MiB.
        assert!(300_000 * single > MAX_INDEX_BYTES);
    }
}
