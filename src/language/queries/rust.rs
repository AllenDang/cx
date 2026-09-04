pub const QUERY: &str = r"
; --- Type definitions ---

(struct_item
    name: (type_identifier) @name) @definition.class

(enum_item
    name: (type_identifier) @name) @definition.class

(union_item
    name: (type_identifier) @name) @definition.class

(type_item
    name: (type_identifier) @name) @definition.class

; --- Functions & methods ---

(declaration_list
    (function_item
        name: (identifier) @name) @definition.method)

(function_item
    name: (identifier) @name) @definition.function

; Signature-only items: trait method requirements and `extern` block
; declarations have no body, so they are declarations, not definitions.

(function_signature_item
    name: (identifier) @name) @declaration.function

(associated_type
    name: (type_identifier) @name) @declaration.type

; --- Traits ---

(trait_item
    name: (type_identifier) @name) @definition.interface

; --- Modules ---

(mod_item
    name: (identifier) @name) @definition.module

; --- Macros ---

(macro_definition
    name: (identifier) @name) @definition.macro

; --- Constants & statics ---

(const_item
    name: (identifier) @name) @definition.constant

(static_item
    name: (identifier) @name) @definition.constant
";
