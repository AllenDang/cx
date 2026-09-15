//! Independent call-head oracles. Casts are operators, template arguments are
//! not callees, and calls inside their operands/arguments remain discoverable.
use super::*;

fn check(lang: &str, file: &str, source: &str, expected: &[(&str, Option<&str>, bool)]) {
    let (_, tree, _) = parse_source(lang, source.as_bytes(), Path::new(file)).unwrap();
    assert!(
        !tree.root_node().has_error(),
        "{}",
        tree.root_node().to_sexp()
    );
    let calls = find_calls(lang, source.as_bytes(), Path::new(file)).unwrap();
    let actual: Vec<_> = calls
        .iter()
        .map(|c| (c.name.as_str(), c.qualifier.as_deref(), c.indirect))
        .collect();
    assert_eq!(actual, expected, "{}", tree.root_node().to_sexp());
}

#[test]
fn cpp_casts_do_not_create_calls_but_operands_do() {
    check(
        "cpp",
        "a.cpp",
        r#"
void probe(int n, const void *p) {
  consume(static_cast<unsigned char>(read()));
  reinterpret_cast<const char *>(p);
  const_cast<void *>(p);
  dynamic_cast<Base *>(object());
  int(n);
  (void)sizeof(read());
}
"#,
        &[
            ("consume", None, false),
            ("read", None, false),
            ("object", None, false),
            ("read", None, false),
        ],
    );
}

#[test]
fn cpp_template_heads_are_names_not_type_arguments() {
    check(
        "cpp",
        "a.cpp",
        r#"
void probe() {
  ns::leaf<int>(argument());
  leaf<Pair<int, double>>(argument());
  object.method<int>();
  pointer->method<double>();
}
"#,
        &[
            ("leaf", Some("ns"), false),
            ("argument", None, false),
            ("leaf", None, false),
            ("argument", None, false),
            ("method", Some("object"), true),
            ("method", Some("pointer"), true),
        ],
    );
}

#[test]
fn rust_turbofish_retains_function_and_receiver() {
    check(
        "rust",
        "a.rs",
        "fn probe() { ns::leaf::<u8>(argument()); object.method::<u8>(); }",
        &[
            ("leaf", Some("ns"), false),
            ("argument", None, false),
            ("method", Some("object"), true),
        ],
    );
}

#[test]
fn cast_spelling_in_rust_is_still_an_ordinary_function_name() {
    check(
        "rust",
        "a.rs",
        "fn probe() { static_cast::<u8>(read()); }",
        &[("static_cast", None, false), ("read", None, false)],
    );
}

#[test]
fn computed_callees_do_not_invent_calls_to_argument_names() {
    check(
        "typescript",
        "a.ts",
        "function probe() { factory(value)(); table[key](); object.run<T>(); }",
        &[("factory", None, false), ("run", Some("object"), true)],
    );
    let facts = find_call_facts(
        "typescript",
        b"function probe() { factory(value)(); table[key](); }",
        Path::new("a.ts"),
    )
    .unwrap();
    assert_eq!(facts.unsupported_calls, 2);
}

#[test]
fn cpp_reference_return_definitions_and_declarations_keep_qualified_identity() {
    let source = "struct Box { int& get(); int&& take(); };\nint& free_ref();\nint&& free_move();\nint& Box::get() { return value; }\nint&& Box::take() { return value; }\nint& free_ref() { return value; }\nint&& free_move() { return value; }\n";
    let parsed = parse_and_extract("cpp", source.as_bytes(), Path::new("a.cpp")).unwrap();
    let mut functions: Vec<_> = parsed
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Fn)
        .map(|s| (s.qualified_name.as_deref().unwrap(), s.role.as_str()))
        .collect();
    functions.sort();
    assert_eq!(
        functions,
        vec![
            ("Box::get", "declaration"),
            ("Box::get", "definition"),
            ("Box::take", "declaration"),
            ("Box::take", "definition"),
            ("free_move", "declaration"),
            ("free_move", "definition"),
            ("free_ref", "declaration"),
            ("free_ref", "definition")
        ]
    );
}

#[test]
fn reference_return_template_is_one_definition_with_its_template_header() {
    let source = b"template<class T> T& relay(T& value) { return value; }\n";
    let parsed = parse_and_extract("cpp", source, Path::new("a.cpp")).unwrap();
    let functions: Vec<_> = parsed
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Fn)
        .collect();
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0].qualified_name.as_deref(), Some("relay"));
    assert_eq!(functions[0].byte_range, (0, source.len() - 1));
    assert!(functions[0].signature.starts_with("template<class T>"));
}
