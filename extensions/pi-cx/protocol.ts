import type { AssetManifest, CxEnvelope } from "./types.js";
import {
  ASSET_FORMAT_VERSION, BUNDLED_LANGUAGES, CX_SCHEMA_VERSION, GRAMMAR_NAMES, LANGUAGE_PACK_VERSION,
  PACKAGE_NAME, PACKAGE_VERSION, TARGET,
} from "./types.js";

export class ProtocolError extends Error {
  constructor(message: string) { super(message); this.name = "ProtocolError"; }
}

export function parseEnvelope(raw: string): CxEnvelope {
  let value: unknown;
  try { value = JSON.parse(raw); } catch (error) {
    throw new ProtocolError(`cx returned invalid JSON: ${error instanceof Error ? error.message : String(error)}`);
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new ProtocolError("cx JSON envelope is not an object");
  const envelope = value as Partial<CxEnvelope>;
  if (envelope.schema_version !== CX_SCHEMA_VERSION) {
    throw new ProtocolError(`incompatible cx schema ${String(envelope.schema_version)}; expected ${CX_SCHEMA_VERSION}. Reinstall pi-cx ${PACKAGE_VERSION}`);
  }
  if (!Array.isArray(envelope.results) || !Array.isArray(envelope.warnings) || !Array.isArray(envelope.next_queries)) {
    throw new ProtocolError("cx JSON envelope is missing required arrays");
  }
  if (!envelope.query || !envelope.page || !envelope.freshness || !("error" in envelope)) {
    throw new ProtocolError("cx JSON envelope is missing required fields");
  }
  return envelope as CxEnvelope;
}

export function validateManifest(value: unknown): AssetManifest {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new ProtocolError("asset manifest is not an object");
  const m = value as AssetManifest;
  const failures: string[] = [];
  if (m.format_version !== ASSET_FORMAT_VERSION) failures.push(`format_version=${m.format_version}`);
  if (m.package !== PACKAGE_NAME) failures.push(`package=${m.package}`);
  if (m.cx_version !== PACKAGE_VERSION) failures.push(`cx_version=${m.cx_version}`);
  if (m.cx_schema_version !== CX_SCHEMA_VERSION) failures.push(`cx_schema_version=${m.cx_schema_version}`);
  if (m.target !== TARGET) failures.push(`target=${m.target}`);
  if (m.tree_sitter_language_pack_version !== LANGUAGE_PACK_VERSION) failures.push(`language_pack=${m.tree_sitter_language_pack_version}`);
  if (!Array.isArray(m.languages) || BUNDLED_LANGUAGES.some((lang) => !m.languages.includes(lang))) failures.push("languages incomplete");
  if (!m.files || typeof m.files !== "object" || !m.files["bin/cx"]) failures.push("files missing bin/cx");
  const expectedFiles = new Set(["bin/cx", ...GRAMMAR_NAMES.map((name) => `grammars/libtree_sitter_${name}.dylib`)]);
  for (const path of Object.keys(m.files ?? {})) if (!expectedFiles.has(path)) failures.push(`unexpected file ${path}`);
  for (const path of expectedFiles) if (!m.files?.[path]) failures.push(`missing file ${path}`);
  for (const [path, entry] of Object.entries(m.files ?? {})) {
    if (path.startsWith("/") || path.split(/[\\/]/).includes("..")) failures.push(`unsafe file path ${path}`);
    if (!entry || !/^[a-f0-9]{64}$/.test(entry.sha256) || !Number.isSafeInteger(entry.bytes) || entry.bytes < 0) failures.push(`invalid file metadata ${path}`);
  }
  if (failures.length) throw new ProtocolError(`incompatible pi-cx asset manifest: ${failures.join(", ")}`);
  return m;
}

export function missingGrammarLanguage(envelope: CxEnvelope, stderr = ""): string | undefined {
  if (envelope.error?.code !== "grammar_not_installed" && !/grammar.+not installed/i.test(stderr)) return undefined;
  if (envelope.error?.language) return envelope.error.language;
  const text = `${envelope.error?.message ?? ""}\n${stderr}`;
  return text.match(/(?:grammar|language)[ '\"]+([a-z0-9_+-]+)[ '\"]/i)?.[1]?.toLowerCase()
    ?? text.match(/([a-z0-9_+-]+) grammar/i)?.[1]?.toLowerCase();
}
