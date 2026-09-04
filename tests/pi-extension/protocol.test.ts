import assert from "node:assert/strict";
import test from "node:test";
import { missingGrammarLanguage, parseEnvelope, validateManifest } from "../../extensions/pi-cx/protocol.js";

const envelope = { schema_version: 1, query: { kind: "symbols" }, freshness: { generation: 1 }, page: { total: 0, offset: 0, limit: 10, truncated: false }, results: [], warnings: [], next_queries: [], error: null };
test("parses empty success without treating it as an error", () => assert.equal(parseEnvelope(JSON.stringify(envelope)).results.length, 0));
test("rejects invalid JSON and incompatible schema", () => {
  assert.throws(() => parseEnvelope("not json"), /invalid JSON/);
  assert.throws(() => parseEnvelope(JSON.stringify({ ...envelope, schema_version: 2 })), /incompatible cx schema/);
});
test("validates the fixed asset contract", () => {
  const file = { sha256: "a".repeat(64), bytes: 1 };
  const manifest = { format_version: 1, package: "pi-cx", cx_version: "0.7.2", cx_schema_version: 1, target: "aarch64-apple-darwin", tree_sitter_language_pack_version: "1.3.1", languages: ["rust", "c", "cpp", "javascript", "jsx", "typescript", "tsx", "python", "go"], files: { "bin/cx": file, ...Object.fromEntries(["rust", "typescript", "tsx", "python", "go", "c", "cpp"].map(name => [`grammars/libtree_sitter_${name}.dylib`, file])) } };
  assert.equal(validateManifest(manifest).target, "aarch64-apple-darwin");
  assert.throws(() => validateManifest({ ...manifest, cx_version: "9.9.9" }), /cx_version/);
  assert.throws(() => validateManifest({ ...manifest, files: { "../cx": { sha256: "a".repeat(64), bytes: 1 } } }), /bin\/cx/);
});
test("extracts missing grammar language", () => {
  const failed = { ...envelope, error: { code: "grammar_not_installed", message: "language 'java' grammar not installed" } };
  assert.equal(missingGrammarLanguage(failed, ""), "java");
});
