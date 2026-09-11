//! HTML is a host, not a JavaScript syntax context. Each raw script body is
//! parsed independently using an included range over the unchanged host bytes.
use tree_sitter::{Node, Range, Tree};

pub(super) fn script_ranges(tree: &Tree, source: &[u8]) -> Vec<Range> {
    let mut ranges = Vec::new();
    let mut stack = vec![(tree.root_node(), false)];
    while let Some((node, mut inert)) = stack.pop() {
        // HTML ignores the self-closing flag on plaintext as well.
        if node.kind() == "self_closing_tag"
            && tag_name(node, source).is_some_and(|name| name.eq_ignore_ascii_case("plaintext"))
        {
            break;
        }
        if node.kind() == "script_element" {
            let mut cursor = node.walk();
            let children: Vec<_> = node.named_children(&mut cursor).collect();
            let start = children.iter().find(|n| n.kind() == "start_tag");
            // Requiring a real closing tag avoids recovering an unterminated
            // host element as executable content extending beyond its bounds.
            let closed = children
                .iter()
                .any(|n| n.kind() == "end_tag" && !n.is_missing());
            if !inert && closed && start.is_some_and(|n| executable(*n, source)) {
                for body in children.iter().filter(|n| n.kind() == "raw_text") {
                    if body.start_byte() < body.end_byte() && body.end_byte() <= source.len() {
                        ranges.push(body.range());
                    }
                }
            }
            continue;
        }
        // Skip foreign/raw-text contexts. Templates are inert but still use
        // HTML tokenization: plaintext inside one consumes the host remainder.
        if node.kind() == "element" {
            let mut cursor = node.walk();
            let start = node
                .named_children(&mut cursor)
                .find(|n| n.kind() == "start_tag");
            if let Some(name) = start.and_then(|n| tag_name(n, source)) {
                if name.eq_ignore_ascii_case("plaintext") {
                    break;
                }
                inert |= name.eq_ignore_ascii_case("template");
                if [
                    "svg", "math", "textarea", "title", "xmp", "iframe", "noembed", "noframes",
                    "noscript",
                ]
                .iter()
                .any(|tag| name.eq_ignore_ascii_case(tag))
                {
                    continue;
                }
            }
        }
        for i in (0..node.named_child_count()).rev() {
            if let Some(child) = node.named_child(i as u32) {
                stack.push((child, inert));
            }
        }
    }
    ranges
}

fn tag_name<'a>(tag: Node, source: &'a [u8]) -> Option<&'a str> {
    let mut cursor = tag.walk();
    tag.named_children(&mut cursor)
        .find(|n| n.kind() == "tag_name")?
        .utf8_text(source)
        .ok()
}

fn executable(tag: Node, source: &[u8]) -> bool {
    if tag.has_error() {
        return false;
    }
    let mut script_type = None;
    let mut language = None;
    let mut cursor = tag.walk();
    for attr in tag
        .named_children(&mut cursor)
        .filter(|n| n.kind() == "attribute")
    {
        let mut cursor = attr.walk();
        let children: Vec<_> = attr.named_children(&mut cursor).collect();
        let Some(name) = children
            .iter()
            .find(|n| n.kind() == "attribute_name")
            .and_then(|n| n.utf8_text(source).ok())
        else {
            return false;
        };
        if name.eq_ignore_ascii_case("src") {
            return false;
        }
        let value = children
            .iter()
            .find(|n| matches!(n.kind(), "attribute_value" | "quoted_attribute_value"))
            .and_then(|n| {
                let value = if n.kind() == "quoted_attribute_value" {
                    n.named_child(0)?
                } else {
                    *n
                };
                value.utf8_text(source).ok()
            })
            .unwrap_or("")
            .to_ascii_lowercase();
        // HTML uses the first occurrence of a duplicate attribute.
        if name.eq_ignore_ascii_case("type") && script_type.is_none() {
            script_type = Some(value.clone());
        }
        if name.eq_ignore_ascii_case("language") && language.is_none() {
            language = Some(value);
        }
    }
    match script_type {
        Some(value) => {
            value.is_empty()
                || value == "module"
                || javascript_mime(value.trim_matches(|c: char| c.is_ascii_whitespace()))
        }
        None => language
            .is_none_or(|value| value.is_empty() || javascript_mime(&format!("text/{value}"))),
    }
}

fn javascript_mime(value: &str) -> bool {
    matches!(
        value,
        "application/ecmascript"
            | "application/javascript"
            | "application/x-ecmascript"
            | "application/x-javascript"
            | "text/ecmascript"
            | "text/javascript"
            | "text/javascript1.0"
            | "text/javascript1.1"
            | "text/javascript1.2"
            | "text/javascript1.3"
            | "text/javascript1.4"
            | "text/javascript1.5"
            | "text/jscript"
            | "text/livescript"
            | "text/x-ecmascript"
            | "text/x-javascript"
    )
}
