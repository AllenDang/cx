#!/usr/bin/env node
import { createHash } from "node:crypto";
import { readFile, readdir, stat, writeFile } from "node:fs/promises";
import { join, relative } from "node:path";
const [stage, version] = process.argv.slice(2);
if (!stage || !version) throw new Error("usage: make-pi-cx-manifest.mjs STAGE VERSION");
const files = {};
async function walk(dir) { for (const entry of await readdir(dir, { withFileTypes: true })) { const path = join(dir, entry.name); if (entry.isDirectory()) await walk(path); else if (entry.isFile()) { const rel = relative(stage, path).replaceAll("\\", "/"); const data = await readFile(path); files[rel] = { sha256: createHash("sha256").update(data).digest("hex"), bytes: (await stat(path)).size }; } } }
await walk(stage);
const manifest = { format_version: 1, package: "pi-cx", cx_version: version, cx_schema_version: 1, target: "aarch64-apple-darwin", tree_sitter_language_pack_version: "1.3.1", languages: ["rust", "c", "cpp", "javascript", "jsx", "typescript", "tsx", "python", "go"], files };
await writeFile(join(stage, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
