mod extract;
mod html;
mod markdown;
mod queries;

use crate::index::{Symbol, SymbolKind};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{LazyLock, RwLock};
use tree_sitter::{Parser, Query, StreamingIterator};

/// Cache compiled queries keyed by resolved grammar name (e.g. "rust", "tsx").
static QUERY_CACHE: LazyLock<RwLock<HashMap<&'static str, Query>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Separate cache for import queries, keyed the same way.
static IMPORT_QUERY_CACHE: LazyLock<RwLock<HashMap<&'static str, Query>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

// --- Language registry ---

pub struct LanguageConfig {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    /// Map certain file extensions to a different grammar name (e.g. tsx → "tsx").
    pub grammar_override: &'static [(&'static str, &'static str)],
    /// Names to pass to `tree_sitter_language_pack::download()`. Empty = use name.
    pub download_names: &'static [&'static str],
    pub query: &'static str,
    /// Find this child node kind to determine where the body starts; signature = text before it.
    pub sig_body_child: Option<&'static str>,
    /// Scan for this byte to split signature from body (e.g. b'{').
    pub sig_delimiter: Option<u8>,
    /// (`capture_name`, `node_kind`, `SymbolKind`) — checked before defaults.
    /// Empty `node_kind` matches any node.
    pub kind_overrides: &'static [(&'static str, &'static str, SymbolKind)],
    /// Node kinds that represent identifier references (for find-references).
    pub ref_node_types: &'static [&'static str],
}

static LANGUAGES: &[LanguageConfig] = &[
    LanguageConfig {
        name: "html",
        extensions: &["html", "htm"],
        grammar_override: &[],
        download_names: &["html", "typescript"],
        query: "",
        sig_body_child: None,
        sig_delimiter: None,
        kind_overrides: &[],
        ref_node_types: &[],
    },
    LanguageConfig {
        name: "markdown",
        extensions: &["md", "markdown", "mdown"],
        grammar_override: &[],
        download_names: &[],
        query: "",
        sig_body_child: None,
        sig_delimiter: None,
        kind_overrides: &[],
        ref_node_types: &[],
    },
    LanguageConfig {
        name: "rust",
        extensions: &["rs"],
        grammar_override: &[],
        download_names: &[],
        query: queries::RUST,
        sig_body_child: None,
        sig_delimiter: Some(b'{'),
        kind_overrides: &[
            ("definition.class", "struct_item", SymbolKind::Struct),
            ("definition.class", "enum_item", SymbolKind::Enum),
            ("definition.class", "union_item", SymbolKind::Struct),
            ("definition.class", "type_item", SymbolKind::Type),
            ("definition.class", "", SymbolKind::Struct),
            ("definition.interface", "", SymbolKind::Trait),
            ("definition.macro", "", SymbolKind::Fn),
        ],
        ref_node_types: &["identifier", "type_identifier", "field_identifier"],
    },
    LanguageConfig {
        name: "typescript",
        extensions: &["ts", "tsx", "js", "jsx"],
        grammar_override: &[("tsx", "tsx"), ("jsx", "tsx")],
        download_names: &["typescript", "tsx"],
        query: queries::TYPESCRIPT,
        sig_body_child: None,
        sig_delimiter: Some(b'{'),
        kind_overrides: &[],
        ref_node_types: &[
            "identifier",
            "type_identifier",
            "property_identifier",
            "shorthand_property_identifier",
            "shorthand_property_identifier_pattern",
        ],
    },
    LanguageConfig {
        name: "python",
        extensions: &["py"],
        grammar_override: &[],
        download_names: &[],
        query: queries::PYTHON,
        sig_body_child: Some("block"),
        sig_delimiter: None,
        kind_overrides: &[],
        ref_node_types: &["identifier"],
    },
    LanguageConfig {
        name: "go",
        extensions: &["go"],
        grammar_override: &[],
        download_names: &[],
        query: queries::GO,
        sig_body_child: None,
        sig_delimiter: Some(b'{'),
        kind_overrides: &[],
        ref_node_types: &["identifier", "type_identifier", "field_identifier"],
    },
    LanguageConfig {
        name: "c",
        extensions: &["c"],
        grammar_override: &[],
        download_names: &[],
        query: queries::C,
        sig_body_child: None,
        sig_delimiter: Some(b'{'),
        kind_overrides: &[("definition.class", "", SymbolKind::Struct)],
        ref_node_types: &["identifier", "type_identifier", "field_identifier"],
    },
    LanguageConfig {
        name: "objc",
        extensions: &["m", "mm"],
        grammar_override: &[],
        download_names: &[],
        query: queries::OBJC,
        sig_body_child: Some("compound_statement"),
        sig_delimiter: None,
        kind_overrides: &[],
        ref_node_types: &[
            "identifier",
            "type_identifier",
            "field_identifier",
            "method_identifier",
        ],
    },
    LanguageConfig {
        name: "cpp",
        extensions: &["cpp", "cc", "cxx", "h", "hpp", "hxx", "hh"],
        grammar_override: &[],
        download_names: &[],
        query: queries::CPP,
        sig_body_child: None,
        sig_delimiter: Some(b'{'),
        kind_overrides: &[],
        ref_node_types: &["identifier", "type_identifier", "field_identifier"],
    },
    LanguageConfig {
        name: "java",
        extensions: &["java"],
        grammar_override: &[],
        download_names: &[],
        query: queries::JAVA,
        sig_body_child: None,
        sig_delimiter: Some(b'{'),
        kind_overrides: &[],
        ref_node_types: &["identifier", "type_identifier"],
    },
    LanguageConfig {
        name: "ruby",
        extensions: &["rb"],
        grammar_override: &[],
        download_names: &[],
        query: queries::RUBY,
        sig_body_child: None,
        sig_delimiter: None,
        kind_overrides: &[],
        ref_node_types: &["identifier", "constant"],
    },
    LanguageConfig {
        name: "lua",
        extensions: &["lua"],
        grammar_override: &[],
        download_names: &[],
        query: queries::LUA,
        sig_body_child: None,
        sig_delimiter: None,
        kind_overrides: &[],
        ref_node_types: &["identifier"],
    },
    LanguageConfig {
        name: "zig",
        extensions: &["zig"],
        grammar_override: &[],
        download_names: &[],
        query: queries::ZIG,
        sig_body_child: None,
        sig_delimiter: Some(b'{'),
        kind_overrides: &[("definition.class", "Decl", SymbolKind::Struct)],
        ref_node_types: &["IDENTIFIER"],
    },
    LanguageConfig {
        name: "bash",
        extensions: &["sh", "bash"],
        grammar_override: &[],
        download_names: &[],
        query: queries::BASH,
        sig_body_child: None,
        sig_delimiter: Some(b'{'),
        kind_overrides: &[],
        ref_node_types: &["word"],
    },
    LanguageConfig {
        name: "solidity",
        extensions: &["sol"],
        grammar_override: &[],
        download_names: &[],
        query: queries::SOLIDITY,
        sig_body_child: None,
        sig_delimiter: Some(b'{'),
        kind_overrides: &[],
        ref_node_types: &["identifier"],
    },
    LanguageConfig {
        name: "dart",
        extensions: &["dart"],
        grammar_override: &[],
        download_names: &[],
        query: queries::DART,
        sig_body_child: None,
        sig_delimiter: Some(b'{'),
        kind_overrides: &[],
        ref_node_types: &["identifier", "type_identifier"],
    },
    LanguageConfig {
        name: "elixir",
        extensions: &["ex", "exs"],
        grammar_override: &[],
        download_names: &[],
        query: queries::ELIXIR,
        sig_body_child: None,
        sig_delimiter: None,
        kind_overrides: &[],
        ref_node_types: &["identifier", "alias"],
    },
    LanguageConfig {
        name: "swift",
        extensions: &["swift"],
        grammar_override: &[],
        download_names: &[],
        query: queries::SWIFT,
        sig_body_child: None,
        sig_delimiter: Some(b'{'),
        kind_overrides: &[
            ("definition.struct", "", SymbolKind::Struct),
            ("definition.enum", "", SymbolKind::Enum),
        ],
        ref_node_types: &["simple_identifier", "type_identifier"],
    },
];

// --- Errors ---

#[derive(Debug)]
pub enum LangError {
    NotInstalled(String),
    ParseFailed,
}

impl std::fmt::Display for LangError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInstalled(name) => {
                write!(f, "{name} grammar not installed — run: cx lang add {name}")
            }
            Self::ParseFailed => write!(f, "parse failed"),
        }
    }
}

// --- Public API ---

/// Detect language config name from file extension.
pub fn detect_language(path: &Path) -> Option<&'static str> {
    let ext = path.extension().and_then(|e| e.to_str())?;
    LANGUAGES
        .iter()
        .find(|c| c.extensions.contains(&ext))
        .map(|c| c.name)
}

/// Return all supported language config names.
pub fn supported_languages() -> Vec<&'static str> {
    LANGUAGES.iter().map(|c| c.name).collect()
}

/// Return the primary file extension for a language config name.
pub fn primary_extension(lang: &str) -> &str {
    LANGUAGES
        .iter()
        .find(|c| c.name == lang)
        .and_then(|c| c.extensions.first().copied())
        .unwrap_or(lang)
}

/// Return the download names for a language (for `cx lang add`).
pub fn download_names_for(lang: &str) -> Vec<&'static str> {
    LANGUAGES
        .iter()
        .find(|c| c.name == lang)
        .map(|c| {
            if c.download_names.is_empty() {
                vec![c.name]
            } else {
                c.download_names.to_vec()
            }
        })
        .unwrap_or_default()
}

/// Resolve the grammar name for a given config + file extension.
fn resolve_grammar_name(config: &LanguageConfig, ext: &str) -> &'static str {
    for &(e, grammar) in config.grammar_override {
        if e == ext {
            return grammar;
        }
    }
    config.name
}

/// Look up config, create parser, and parse source into a tree.
fn parse_source(
    lang: &str,
    source: &[u8],
    path: &Path,
) -> Result<(&'static LanguageConfig, tree_sitter::Tree, &'static str), LangError> {
    parse_range(lang, source, path, None)
}

/// Parse one bounded embedded region, retaining host byte/point coordinates.
fn parse_range(
    lang: &str,
    source: &[u8],
    path: &Path,
    range: Option<tree_sitter::Range>,
) -> Result<(&'static LanguageConfig, tree_sitter::Tree, &'static str), LangError> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let config = LANGUAGES
        .iter()
        .find(|c| c.name == lang)
        .ok_or_else(|| LangError::NotInstalled(lang.to_string()))?;
    let grammar_name = resolve_grammar_name(config, ext);

    // Queries never install code implicitly. has_parser loads an existing
    // library (and caches it) without network; lang add is the opt-in installer.
    if !tree_sitter_language_pack::has_parser(grammar_name) {
        return Err(LangError::NotInstalled(config.name.to_string()));
    }
    let ts_lang = tree_sitter_language_pack::get_language(grammar_name)
        .map_err(|_| LangError::NotInstalled(config.name.to_string()))?;

    thread_local! {
        static PARSER: std::cell::RefCell<Parser> = std::cell::RefCell::new(Parser::new());
    }

    let tree = PARSER.with_borrow_mut(|parser| {
        parser
            .set_language(&ts_lang)
            .map_err(|_| LangError::ParseFailed)?;
        parser
            .set_included_ranges(&range.into_iter().collect::<Vec<_>>())
            .map_err(|_| LangError::ParseFailed)?;
        parser.parse(source, None).ok_or(LangError::ParseFailed)
    })?;
    Ok((config, tree, grammar_name))
}

fn parse_units(
    lang: &str,
    source: &[u8],
    path: &Path,
) -> Result<Vec<(&'static LanguageConfig, tree_sitter::Tree, &'static str)>, LangError> {
    if lang != "html" {
        return Ok(vec![parse_source(lang, source, path)?]);
    }
    let (_, host, _) = parse_source(lang, source, path)?;
    html::script_ranges(&host, source)
        .into_iter()
        .map(|range| parse_range("typescript", source, Path::new("inline.js"), Some(range)))
        .collect()
}

pub use extract::CallSite;
pub use extract::RefEvidence;

/// Whether this language has a call model (HTML uses bounded script units).
pub fn supports_calls(lang: &str) -> bool {
    lang == "html" || extract::supports_calls(lang)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CallFacts {
    pub sites: Vec<CallSite>,
    pub has_parse_errors: bool,
    pub unsupported_calls: usize,
}

impl CallFacts {
    fn empty() -> Self {
        Self {
            sites: Vec::new(),
            has_parse_errors: false,
            unsupported_calls: 0,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedCallSite {
    name: u32,
    qualifier: Option<u32>,
    line: u32,
    start: u32,
    end: u32,
    owner_start: u32,
    owner_end: u32,
    flags: u8,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedCallFacts {
    names: Vec<String>,
    qualifiers: Vec<String>,
    sites: Vec<CachedCallSite>,
    pub has_parse_errors: bool,
    pub unsupported_calls: u32,
}
impl CachedCallFacts {
    fn intern(values: &mut Vec<String>, value: &str) -> u32 {
        values.iter().position(|v| v == value).unwrap_or_else(|| {
            values.push(value.into());
            values.len() - 1
        }) as u32
    }
    pub fn from_facts(facts: CallFacts) -> Self {
        let mut names = Vec::new();
        let mut qualifiers = Vec::new();
        let mut sites = Vec::with_capacity(facts.sites.len());
        for site in facts.sites {
            let name = Self::intern(&mut names, &site.name);
            let qualifier = site
                .qualifier
                .as_deref()
                .map(|q| Self::intern(&mut qualifiers, q));
            let (owner_start, owner_end) = site.owner_range.unwrap_or((0, 0));
            sites.push(CachedCallSite {
                name,
                qualifier,
                line: site.line as u32,
                start: site.byte_offset as u32,
                end: site.byte_end as u32,
                owner_start: owner_start as u32,
                owner_end: owner_end as u32,
                flags: u8::from(site.anonymous_owner) | (u8::from(site.indirect) << 1),
            });
        }
        Self {
            names,
            qualifiers,
            sites,
            has_parse_errors: facts.has_parse_errors,
            unsupported_calls: facts.unsupported_calls as u32,
        }
    }
    pub fn expand(&self) -> CallFacts {
        CallFacts {
            sites: self
                .sites
                .iter()
                .map(|site| CallSite {
                    name: self.names[site.name as usize].clone(),
                    qualifier: site.qualifier.map(|q| self.qualifiers[q as usize].clone()),
                    line: site.line as usize,
                    byte_offset: site.start as usize,
                    byte_end: site.end as usize,
                    owner_range: (site.owner_end > site.owner_start)
                        .then_some((site.owner_start as usize, site.owner_end as usize)),
                    anonymous_owner: site.flags & 1 != 0,
                    indirect: site.flags & 2 != 0,
                })
                .collect(),
            has_parse_errors: self.has_parse_errors,
            unsupported_calls: self.unsupported_calls as usize,
        }
    }
    pub fn contains_name(&self, name: &str) -> bool {
        self.names
            .iter()
            .position(|n| n == name)
            .is_some_and(|id| self.sites.iter().any(|s| s.name as usize == id))
    }
    #[cfg(test)]
    pub fn site_count(&self) -> usize {
        self.sites.len()
    }
}

pub const TASK_FACTS_VERSION: u32 = 2;
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskFacts {
    pub version: u32,
    pub line_starts: Vec<u32>,
    pub calls: Option<CachedCallFacts>,
}
impl TaskFacts {
    pub fn line(&self, byte: usize) -> usize {
        self.line_starts
            .partition_point(|start| *start as usize <= byte)
    }
    pub(crate) fn empty(source: &[u8]) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(
            source
                .iter()
                .enumerate()
                .filter_map(|(i, b)| (*b == b'\n').then_some((i + 1) as u32)),
        );
        Self {
            version: TASK_FACTS_VERSION,
            line_starts,
            calls: None,
        }
    }
    pub(crate) fn placeholder() -> Self {
        Self {
            version: TASK_FACTS_VERSION,
            line_starts: vec![0],
            calls: None,
        }
    }
}

/// Preserve valid syntax sites in a recovery tree, but disclose its incomplete
/// enumeration. A malformed call node itself is never promoted into a fact.
pub fn find_call_facts(lang: &str, source: &[u8], path: &Path) -> Result<CallFacts, LangError> {
    let mut facts = CallFacts {
        sites: Vec::new(),
        has_parse_errors: false,
        unsupported_calls: 0,
    };
    if lang == "html" {
        facts.has_parse_errors = parse_source(lang, source, path)?.1.root_node().has_error();
    }
    for (config, tree, _) in parse_units(lang, source, path)? {
        facts.has_parse_errors |= tree.root_node().has_error();
        let calls = extract::find_call_sites(config.name, &tree, source);
        facts.sites.extend(calls.sites);
        facts.unsupported_calls += calls.unsupported_calls;
    }
    Ok(facts)
}

pub type TextRegion = (usize, usize, &'static str);

/// Text provenance for lexical context matches. Interpolation expressions are
/// not labelled strings: only literal content nodes are recorded.
pub fn text_regions(
    lang: &str,
    source: &[u8],
    path: &Path,
) -> Result<(Vec<TextRegion>, bool), LangError> {
    if lang == "markdown" {
        return Ok((vec![(0, source.len(), "document_text")], false));
    }
    let mut regions = Vec::new();
    let mut partial = false;
    for (_, tree, _) in parse_units(lang, source, path)? {
        partial |= tree.root_node().has_error();
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            let kind = node.kind();
            let field = if kind.contains("comment") {
                Some("comment_text")
            } else if matches!(
                kind,
                "string_content" | "string_fragment" | "raw_string_content" | "char_literal"
            ) {
                Some("string_text")
            } else if node.is_error() {
                Some("unparsed_text")
            } else {
                None
            };
            if let Some(field) = field {
                regions.push((node.start_byte(), node.end_byte(), field));
            }
            for i in 0..node.named_child_count() {
                if let Some(child) = node.named_child(i as u32) {
                    stack.push(child);
                }
            }
        }
    }
    regions.sort_by_key(|(start, end, _)| end - start);
    Ok((regions, partial))
}

#[cfg(test)]
pub fn find_calls(lang: &str, source: &[u8], path: &Path) -> Result<Vec<CallSite>, LangError> {
    find_call_facts(lang, source, path).map(|facts| facts.sites)
}

/// Parse source and find all identifier nodes whose text matches `name`.
pub fn find_references(
    lang: &str,
    source: &[u8],
    path: &Path,
    name: &str,
) -> Result<Vec<extract::Reference>, LangError> {
    let mut refs = Vec::new();
    for (config, tree, _) in parse_units(lang, source, path)? {
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            if node.child_count() == 0
                && config.ref_node_types.contains(&node.kind())
                && node.utf8_text(source).ok() == Some(name)
            {
                refs.push(extract::Reference {
                    line: node.start_position().row + 1,
                    byte_offset: node.start_byte(),
                    evidence: extract::classify_reference(node, source),
                });
            }
            for i in (0..node.child_count()).rev() {
                if let Some(child) = node.child(i as u32) {
                    stack.push(child);
                }
            }
        }
    }
    Ok(refs)
}

/// Everything one parse of a file yields.
pub struct FileParse {
    pub symbols: Vec<Symbol>,
    /// Import/include targets exactly as written in the source, deduplicated.
    ///
    /// Raw text, not resolved paths: resolution is the caller's job and is
    /// reported separately so an unresolvable import is never invented
    /// (roadmap §8).
    pub imports: Vec<String>,
    pub task: TaskFacts,
}

/// Tree-sitter pattern capturing import/include targets as `@import`.
///
/// Only languages whose imports are a written path or module path are modelled;
/// the rest report no imports rather than guessing.
fn import_query(lang: &str) -> Option<&'static str> {
    match lang {
        "c" | "cpp" => Some("(preproc_include path: (_) @import)"),
        "rust" => Some("(use_declaration argument: (_) @import)"),
        "typescript" => Some(
            r#"
            (import_statement source: (string) @import)
            (export_statement source: (string) @import)
            "#,
        ),
        _ => None,
    }
}

/// Strip the punctuation a language wraps its import targets in.
fn clean_import(text: &str) -> String {
    text.trim()
        .trim_start_matches(['"', '<', '\''])
        .trim_end_matches(['"', '>', '\''])
        .trim()
        .to_string()
}

/// Extract import/include targets from an already-parsed tree.
fn extract_imports(
    lang: &str,
    grammar_name: &'static str,
    tree: &tree_sitter::Tree,
    source: &[u8],
) -> Vec<String> {
    let Some(pattern) = import_query(lang) else {
        return Vec::new();
    };

    let run = |query: &Query| -> Vec<String> {
        let mut cursor = tree_sitter::QueryCursor::new();
        let mut matches = cursor.matches(query, tree.root_node(), source);
        let mut out: Vec<String> = Vec::new();
        while let Some(m) = matches.next() {
            for capture in m.captures {
                if let Ok(text) = capture.node.utf8_text(source) {
                    let cleaned = clean_import(text);
                    if !cleaned.is_empty() && !out.contains(&cleaned) {
                        out.push(cleaned);
                    }
                }
            }
        }
        out
    };

    {
        let cache = IMPORT_QUERY_CACHE
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(query) = cache.get(grammar_name) {
            return run(query);
        }
    }

    let mut cache = IMPORT_QUERY_CACHE
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let query = match cache.entry(grammar_name) {
        std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
        std::collections::hash_map::Entry::Vacant(e) => {
            match Query::new(&tree.language(), pattern) {
                Ok(q) => e.insert(q),
                // A grammar without these node kinds is not an error; it simply
                // has no imports to report.
                Err(_) => return Vec::new(),
            }
        }
    };
    run(query)
}

pub fn parse_task_facts(lang: &str, source: &[u8], path: &Path) -> Result<TaskFacts, LangError> {
    let mut task = TaskFacts::empty(source);
    let mut calls = supports_calls(lang).then(CallFacts::empty);
    if lang == "markdown" {
        parse_source(lang, source, path)?;
        return Ok(task);
    }
    for (config, tree, _) in parse_units(lang, source, path)? {
        if let Some(facts) = &mut calls {
            facts.has_parse_errors |= tree.root_node().has_error();
            let extracted = extract::find_call_sites(config.name, &tree, source);
            facts.sites.extend(extracted.sites);
            facts.unsupported_calls += extracted.unsupported_calls;
        }
    }
    task.calls = calls.map(CachedCallFacts::from_facts);
    Ok(task)
}

/// Parse a file and extract symbols for the given language.
/// `path` is used to distinguish .tsx from .ts for grammar selection.
pub fn parse_and_extract(lang: &str, source: &[u8], path: &Path) -> Result<FileParse, LangError> {
    parse_file(lang, source, path, true)
}
pub fn parse_index_facts(lang: &str, source: &[u8], path: &Path) -> Result<FileParse, LangError> {
    parse_file(lang, source, path, false)
}
fn parse_file(
    lang: &str,
    source: &[u8],
    path: &Path,
    include_task: bool,
) -> Result<FileParse, LangError> {
    if lang == "markdown" {
        parse_source(lang, source, path)?;
        return Ok(FileParse {
            symbols: markdown::extract_headings(source),
            imports: Vec::new(),
            task: if include_task {
                TaskFacts::empty(source)
            } else {
                TaskFacts::placeholder()
            },
        });
    }

    let mut result = FileParse {
        symbols: Vec::new(),
        imports: Vec::new(),
        task: if include_task {
            TaskFacts::empty(source)
        } else {
            TaskFacts::placeholder()
        },
    };
    let mut call_facts = (include_task && supports_calls(lang)).then(CallFacts::empty);
    for (config, tree, grammar_name) in parse_units(lang, source, path)? {
        if let Some(facts) = &mut call_facts {
            facts.has_parse_errors |= tree.root_node().has_error();
            let calls = extract::find_call_sites(config.name, &tree, source);
            facts.sites.extend(calls.sites);
            facts.unsupported_calls += calls.unsupported_calls;
        }
        let parsed = extract_unit(config, &tree, grammar_name, source)?;
        result.symbols.extend(parsed.symbols);
        for import in parsed.imports {
            if !result.imports.contains(&import) {
                result.imports.push(import);
            }
        }
    }
    result.task.calls = call_facts.map(CachedCallFacts::from_facts);
    Ok(result)
}

fn extract_unit(
    config: &'static LanguageConfig,
    tree: &tree_sitter::Tree,
    grammar_name: &'static str,
    source: &[u8],
) -> Result<FileParse, LangError> {
    let imports = extract_imports(config.name, grammar_name, tree, source);

    // Fast path: read lock for cache hits (concurrent reads don't block each other)
    {
        let cache = QUERY_CACHE
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(query) = cache.get(grammar_name) {
            return Ok(FileParse {
                symbols: extract::extract_symbols(config, query, tree, source),
                imports,
                task: TaskFacts::empty(source),
            });
        }
    }

    // Slow path: write lock for cache miss
    let mut cache = QUERY_CACHE
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let query = cache.entry(grammar_name).or_insert_with(|| {
        Query::new(&tree.language(), config.query).expect("query compilation failed")
    });

    Ok(FileParse {
        symbols: extract::extract_symbols(config, query, tree, source),
        imports,
        task: TaskFacts::empty(source),
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod call_tests;
