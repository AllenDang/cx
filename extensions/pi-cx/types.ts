import { currentPlatformConfig } from "./platform.js";

export const PACKAGE_NAME = "pi-cx";
export const PACKAGE_VERSION = "0.7.5";
export const CX_SCHEMA_VERSION = 1;
export const ASSET_FORMAT_VERSION = 1;
export const LANGUAGE_PACK_VERSION = "1.16.1";
export const PLATFORM = currentPlatformConfig();
export const TARGET = PLATFORM.target;
export const PLATFORM_DIR = PLATFORM.platformDir;

export const BUNDLED_LANGUAGES = [
  "rust", "c", "cpp", "javascript", "jsx", "typescript", "tsx", "python", "go",
] as const;

export const GRAMMAR_NAMES = ["rust", "typescript", "tsx", "python", "go", "c", "cpp"] as const;
export const SYMBOL_KINDS = [
  "fn", "struct", "enum", "trait", "type", "const", "class", "interface", "module", "event", "field", "heading",
] as const;
export const SYMBOL_ROLES = ["definition", "declaration", "heading", "unknown"] as const;
export const FRESHNESS_MODES = ["metadata", "verified"] as const;

export interface ManifestFile { sha256: string; bytes: number }
export interface AssetManifest {
  format_version: number;
  package: string;
  cx_version: string;
  cx_schema_version: number;
  target: string;
  tree_sitter_language_pack_version: string;
  languages: string[];
  files: Record<string, ManifestFile>;
}

export interface CxEnvelope {
  schema_version: number;
  query: { kind: string; subject?: string };
  freshness: Record<string, unknown>;
  page: { total: number; offset: number; limit: number | null; truncated: boolean };
  results: unknown[];
  warnings: string[];
  next_queries: string[];
  error: null | { code: string; message: string; language?: string };
}

export interface RunDetails {
  durationMs: number;
  exitCode: number | null;
  killed: boolean;
  stderr: string;
  binaryVersion?: string;
  truncated?: { bytes: number; lines: number; path: string };
  grammarInstalled?: string;
  retried?: boolean;
}

export interface CxRunResult {
  raw: string;
  envelope: CxEnvelope;
  details: RunDetails;
}
