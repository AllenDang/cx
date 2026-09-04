use crate::index::{Symbol, SymbolKind, SymbolRole};
use std::collections::HashMap;
use tree_sitter::{Node, Query, QueryCursor, StreamingIterator};

use super::LanguageConfig;

pub struct Reference {
    pub line: usize, // 1-based
    pub byte_offset: usize,
}

// --- Test detection ---

/// Check if a symbol node represents a test (Rust `#[test]`/`#[cfg(test)]`, Zig `test` decl).
pub(super) fn detect_test_symbol(lang: &str, node: Node, source: &[u8]) -> bool {
    match lang {
        "rust" => has_rust_test_attribute(node, source),
        // Zig test blocks use `TestDecl` node type — these are not captured by our
        // symbol queries, so they never appear in the index. No filtering needed.
        _ => false,
    }
}

/// Walk previous siblings looking for `#[test]` or `#[cfg(test)]` attribute items.
/// Also checks if the node is inside a `#[cfg(test)] mod`.
fn has_rust_test_attribute(node: Node, source: &[u8]) -> bool {
    // Check immediate preceding attribute siblings
    let mut sibling = node.prev_named_sibling();
    while let Some(sib) = sibling {
        if sib.kind() == "attribute_item" {
            if let Ok(text) = sib.utf8_text(source) && is_test_attribute(text) {
                return true;
            }
        } else {
            break; // attributes are contiguous
        }
        sibling = sib.prev_named_sibling();
    }

    // Check if we're inside a #[cfg(test)] mod by walking up the tree
    let mut parent = node.parent();
    while let Some(p) = parent {
        if p.kind() == "mod_item" {
            let mut sib = p.prev_named_sibling();
            while let Some(s) = sib {
                if s.kind() == "attribute_item" {
                    if let Ok(text) = s.utf8_text(source) && text.contains("cfg(test)") {
                        return true;
                    }
                } else {
                    break;
                }
                sib = s.prev_named_sibling();
            }
        }
        parent = p.parent();
    }

    false
}

/// Check if an attribute text represents a test attribute.
/// Matches `#[test]`, `#[tokio::test]`, `#[rstest]`, etc. but not `#[attestation]`.
fn is_test_attribute(text: &str) -> bool {
    let trimmed = text.trim_start_matches("#[").trim_end_matches(']');
    // Exact match: #[test]
    if trimmed == "test" { return true; }
    // Path-qualified: #[tokio::test], #[tokio::test(...)]
    if trimmed.ends_with("::test") || trimmed.contains("::test(") { return true; }
    // cfg(test)
    if trimmed.contains("cfg(test)") { return true; }
    false
}

/// Split a capture name into its role and its kind-resolution key.
///
/// `@definition.function` → (Definition, "definition.function")
/// `@declaration.function` → (Declaration, "definition.function")
/// `@unknown.function` → (Unknown, "definition.function")
///
/// Kind lookup is always normalized to the `definition.` key so a language's
/// `kind_overrides` table covers every role without duplicated entries.
fn split_capture(capture_name: &str) -> Option<(SymbolRole, String)> {
    let (prefix, suffix) = capture_name.split_once('.')?;
    let role = match prefix {
        "definition" => SymbolRole::Definition,
        "declaration" => SymbolRole::Declaration,
        "unknown" => SymbolRole::Unknown,
        _ => return None,
    };
    Some((role, format!("definition.{suffix}")))
}

// --- Lexical scope model (roadmap §5.3) ---

/// How a language nests named scopes.
///
/// Deliberately per-language rather than a shared guess: Phase 5 models Rust,
/// C/C++ and TypeScript, which cover three different scope shapes (modules +
/// impl blocks, namespaces + out-of-line member definitions, and
/// classes/interfaces/namespaces).  A language absent from `scope_model` has its
/// scopes reported as unresolved instead of assumed flat.
struct ScopeModel {
    /// `(node_kind, name_field)` pairs that introduce a named lexical scope.
    nodes: &'static [(&'static str, &'static str)],
    /// Separator used to join the scope path into a qualified name.
    separator: &'static str,
    /// Node kinds whose `scope` field carries an explicitly written qualifier,
    /// e.g. the `EcsWorld::` in `void EcsWorld::run() {}`.
    qualifier_nodes: &'static [&'static str],
}

fn scope_model(lang: &str) -> Option<ScopeModel> {
    match lang {
        "rust" => Some(ScopeModel {
            // `impl Foo` names its scope with the `type` field, not `name`.
            nodes: &[
                ("mod_item", "name"),
                ("impl_item", "type"),
                ("trait_item", "name"),
            ],
            separator: "::",
            qualifier_nodes: &["scoped_identifier"],
        }),
        "cpp" | "c" => Some(ScopeModel {
            nodes: &[
                ("namespace_definition", "name"),
                ("class_specifier", "name"),
                ("struct_specifier", "name"),
                ("union_specifier", "name"),
                ("enum_specifier", "name"),
            ],
            separator: "::",
            qualifier_nodes: &["qualified_identifier"],
        }),
        "typescript" => Some(ScopeModel {
            nodes: &[
                ("class_declaration", "name"),
                ("abstract_class_declaration", "name"),
                ("interface_declaration", "name"),
                ("module", "name"),
                ("internal_module", "name"),
                ("enum_declaration", "name"),
            ],
            separator: ".",
            qualifier_nodes: &[],
        }),
        _ => None,
    }
}

/// Name of the lexical scope `node` introduces, if any.
fn scope_name_of(model: &ScopeModel, node: Node, source: &[u8]) -> Option<String> {
    let (_, field) = model
        .nodes
        .iter()
        .find(|(kind, _)| *kind == node.kind())?;
    let named = node.child_by_field_name(field)?;
    named.utf8_text(source).ok().map(std::string::ToString::to_string)
}

/// Lexical scope path for a symbol, outermost first.
///
/// Two sources are combined, both syntactic facts:
/// 1. enclosing scope nodes walked up the AST (`namespace ange { class W { … } }`)
/// 2. qualifiers written at the definition site (`void W::run() {}`)
///
/// No import or type resolution happens here — that is a later phase.
fn compute_scope_path(
    model: &ScopeModel,
    def_node: Node,
    name_node: Node,
    source: &[u8],
) -> Vec<String> {
    let mut enclosing: Vec<String> = Vec::new();
    let mut cursor = def_node.parent();
    while let Some(node) = cursor {
        if let Some(name) = scope_name_of(model, node, source) {
            enclosing.push(name);
        }
        cursor = node.parent();
    }
    enclosing.reverse();

    // Qualifiers written between the definition node and the name node.
    let mut written: Vec<String> = Vec::new();
    if !model.qualifier_nodes.is_empty() {
        let mut cursor = name_node.parent();
        while let Some(node) = cursor {
            if node.id() == def_node.id() {
                break;
            }
            if model.qualifier_nodes.contains(&node.kind())
                && let Some(scope) = node.child_by_field_name("scope")
                && let Ok(text) = scope.utf8_text(source)
            {
                written.push(text.to_string());
            }
            cursor = node.parent();
        }
        written.reverse();
    }

    enclosing.extend(written);
    enclosing
}

// --- Generic extractor ---

pub(super) fn extract_symbols(
    config: &LanguageConfig,
    query: &Query,
    tree: &tree_sitter::Tree,
    source: &[u8],
) -> Vec<Symbol> {
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), source);
    let capture_names = query.capture_names();
    // Looked up once per file: `None` means this language's lexical scopes are
    // not modelled yet, which is reported as unresolved rather than flat.
    let model = scope_model(config.name);

    let mut symbols = Vec::new();

    while let Some(m) = matches.next() {
        let mut name_node: Option<Node> = None;
        let mut def_node: Option<Node> = None;
        let mut def_capture: Option<(SymbolRole, String)> = None;

        for capture in m.captures {
            let cname = capture_names[capture.index as usize];
            if cname == "name" {
                name_node = Some(capture.node);
            } else if let Some(split) = split_capture(cname) {
                def_node = Some(capture.node);
                def_capture = Some(split);
            }
        }

        let (Some(name_n), Some(def_n), Some((role, kind_key))) = (name_node, def_node, def_capture)
        else {
            continue;
        };

        let mut name = match name_n.utf8_text(source) {
            Ok(s) => {
                // Strip surrounding quotes from string-literal names (e.g. Zig test names)
                if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
                    s[1..s.len()-1].to_string()
                } else {
                    s.to_string()
                }
            }
            Err(_) => continue,
        };

        let kind = match resolve_kind(config, &kind_key, &def_n) {
            Some(k) => k,
            None => continue,
        };

        let byte_range = (def_n.start_byte(), def_n.end_byte());
        let signature = build_signature(config, def_n, source);
        if config.name == "objc"
            && kind == SymbolKind::Fn
            && (def_n.kind() == "method_definition" || def_n.kind() == "method_declaration")
        {
            name = objc_method_name(&signature).unwrap_or(name);
        } else if config.name == "objc"
            && kind == SymbolKind::Class
            && (def_n.kind() == "class_interface" || def_n.kind() == "class_implementation")
        {
            name = objc_class_name(&signature).unwrap_or(name);
        }
        let is_test = detect_test_symbol(config.name, def_n, source);

        let (scope_path, qualified_name) = match model.as_ref() {
            Some(model) => {
                let path = compute_scope_path(model, def_n, name_n, source);
                let qualified = if path.is_empty() {
                    name.clone()
                } else {
                    format!("{}{}{}", path.join(model.separator), model.separator, name)
                };
                (path, Some(qualified))
            }
            // Unmodelled language: record no scope and no qualified name, so a
            // consumer can tell "top-level" from "unknown" (roadmap §5.3).
            None => (Vec::new(), None),
        };

        symbols.push(Symbol {
            name,
            kind,
            role,
            scope_path,
            qualified_name,
            signature,
            byte_range,
            is_test,
        });
    }

    deduplicate(symbols)
}

fn objc_method_name(signature: &str) -> Option<String> {
    let mut parts = Vec::new();
    let bytes = signature.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b':' {
            let mut j = i;
            while j > 0 && bytes[j - 1].is_ascii_whitespace() {
                j -= 1;
            }
            let end = j;
            while j > 0 && (bytes[j - 1].is_ascii_alphanumeric() || bytes[j - 1] == b'_') {
                j -= 1;
            }
            if j < end {
                parts.push(&signature[j..end]);
            }
        }
        i += 1;
    }

    if !parts.is_empty() {
        return Some(parts.join(":") + ":");
    }

    let close = signature.find(')')?;
    let rest = signature[close + 1..].trim_start();
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    (end > 0).then(|| rest[..end].to_string())
}

fn objc_class_name(signature: &str) -> Option<String> {
    let rest = signature
        .strip_prefix("@interface")
        .or_else(|| signature.strip_prefix("@implementation"))?
        .trim_start();
    let name_end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    if name_end == 0 {
        return None;
    }

    let name = &rest[..name_end];
    let after_name = rest[name_end..].trim_start();
    if let Some(after_open) = after_name.strip_prefix('(')
        && let Some(close) = after_open.find(')')
    {
        let category = after_open[..close].trim();
        return Some(format!("{name}({category})"));
    }

    Some(name.to_string())
}

fn resolve_kind(config: &LanguageConfig, capture_name: &str, node: &Node) -> Option<SymbolKind> {
    let node_kind = node.kind();

    // Exact match on (capture_name, node_kind)
    for &(cap, nk, sk) in config.kind_overrides {
        if cap == capture_name && nk == node_kind {
            return Some(sk);
        }
    }
    // Wildcard match (empty node_kind)
    for &(cap, nk, sk) in config.kind_overrides {
        if cap == capture_name && nk.is_empty() {
            return Some(sk);
        }
    }

    // Defaults
    match capture_name {
        "definition.function" => Some(SymbolKind::Fn),
        "definition.method" => Some(SymbolKind::Fn),
        "definition.class" => Some(SymbolKind::Class),
        "definition.interface" => Some(SymbolKind::Interface),
        "definition.type" => Some(SymbolKind::Type),
        "definition.enum" => Some(SymbolKind::Enum),
        "definition.module" => Some(SymbolKind::Module),
        "definition.constant" => Some(SymbolKind::Const),
        "definition.struct" => Some(SymbolKind::Struct),
        "definition.trait" => Some(SymbolKind::Trait),
        "definition.event" => Some(SymbolKind::Event),
        "definition.field" => Some(SymbolKind::Field),
        "definition.heading" => Some(SymbolKind::Heading),
        "definition.macro" => Some(SymbolKind::Fn),
        _ => None,
    }
}

fn build_signature(config: &LanguageConfig, node: Node, source: &[u8]) -> String {
    let start = node.start_byte();
    let end = node.end_byte();
    let text = &source[start..end];

    // Strategy 1: find body child node, take text before it
    if let Some(body_kind) = config.sig_body_child {
        let mut walker = node.walk();
        for child in node.children(&mut walker) {
            if child.kind() == body_kind {
                let sig_text = &source[start..child.start_byte()];
                let sig = String::from_utf8_lossy(sig_text)
                    .trim()
                    .trim_end_matches(':')
                    .trim()
                    .to_string();
                if !sig.is_empty() {
                    return sig;
                }
            }
        }
    }

    // Strategy 2: scan for delimiter byte
    if let Some(delim) = config.sig_delimiter
        && let Some(pos) = text.iter().position(|&b| b == delim) {
            let sig = String::from_utf8_lossy(&text[..pos]).trim().to_string();
            if !sig.is_empty() {
                return sig;
            }
        }

    // Strategy 3: for arrow functions, truncate at =>
    if let Some(pos) = text.windows(2).position(|w| w == b"=>") {
        let sig = String::from_utf8_lossy(&text[..pos + 2]).trim().to_string();
        if !sig.is_empty() {
            return sig;
        }
    }

    // Fallback: first line, strip trailing delimiters
    let first_line = text
        .iter()
        .position(|&b| b == b'\n')
        .map_or(text, |p| &text[..p]);

    String::from_utf8_lossy(first_line)
        .trim()
        .trim_end_matches('{')
        .trim_end_matches(':')
        .trim()
        .to_string()
}

fn deduplicate(symbols: Vec<Symbol>) -> Vec<Symbol> {
    let mut seen: HashMap<(usize, usize), usize> = HashMap::new();
    let mut deduped: Vec<Symbol> = Vec::new();

    for sym in symbols {
        if let Some(&idx) = seen.get(&sym.byte_range) {
            // Later pattern wins for same byte range (more specific capture)
            deduped[idx] = sym;
        } else {
            seen.insert(sym.byte_range, deduped.len());
            deduped.push(sym);
        }
    }

    // Remove symbols whose range is strictly contained within another symbol
    // of the same name (e.g. class_specifier inside template_declaration).
    let ranges: Vec<_> = deduped.iter().map(|s| (s.name.as_str(), s.byte_range)).collect();
    let mut keep = vec![true; deduped.len()];
    for (i, (name_i, (start_i, end_i))) in ranges.iter().enumerate() {
        for (j, (name_j, (start_j, end_j))) in ranges.iter().enumerate() {
            if i != j && name_i == name_j && deduped[i].kind == deduped[j].kind && start_j <= start_i && end_i <= end_j && (start_j, end_j) != (start_i, end_i) {
                keep[i] = false;
                break;
            }
        }
    }

    deduped.into_iter().zip(keep).filter(|(_, k)| *k).map(|(s, _)| s).collect()
}
