#!/usr/bin/env node
import { createHash } from "node:crypto";
import { readFile, stat } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import { byHost, byTarget, grammarFilename, grammarNames, languagePackVersion } from "./pi-cx-platforms.mjs";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const cargo = await readFile(join(root, "Cargo.toml"), "utf8");
const cargoVersion = cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
const pkg = JSON.parse(await readFile(join(root, "package.json"), "utf8"));
if (!cargoVersion || cargoVersion !== pkg.version) throw new Error(`Cargo/package version mismatch: ${cargoVersion}/${pkg.version}`);
const tag = process.env.GITHUB_REF_NAME ?? process.env.PI_CX_TAG;
if (tag && tag.replace(/^v/, "") !== pkg.version) throw new Error(`tag/package version mismatch: ${tag}/${pkg.version}`);
const config = process.env.PI_CX_TARGET ? byTarget(process.env.PI_CX_TARGET) : byHost();
const stage = resolve(process.env.PI_CX_ASSET_DIR ?? join(root, "vendor", "pi-cx", config.platformDir));
const manifest = JSON.parse(await readFile(join(stage, "manifest.json"), "utf8"));
if (manifest.cx_version !== pkg.version || manifest.cx_schema_version !== 1 || manifest.target !== config.target || manifest.tree_sitter_language_pack_version !== languagePackVersion) throw new Error("asset manifest version/schema/target mismatch");
const expected = new Set([`bin/${config.binaryName}`, ...grammarNames.map(name => `grammars/${grammarFilename(name, config)}`)]);
if (Object.keys(manifest.files).length !== expected.size || [...expected].some(path => !manifest.files[path])) throw new Error("asset manifest file set mismatch");
for (const [path, meta] of Object.entries(manifest.files)) {
  const full = join(stage, path), data = await readFile(full);
  if ((await stat(full)).size !== meta.bytes || createHash("sha256").update(data).digest("hex") !== meta.sha256) throw new Error(`asset verification failed: ${path}`);
}
const native = byHost().target === config.target;
if (native) {
  const version = await new Promise((resolveVersion, reject) => { const child = spawn(join(stage, "bin", config.binaryName), ["--version"], { shell: false }); let out = ""; child.stdout.on("data", c => out += c); child.once("error", reject); child.once("close", code => code === 0 ? resolveVersion(out.trim()) : reject(new Error(`cx --version exited ${code}`))); });
  if (!version.includes(pkg.version)) throw new Error(`binary/package version mismatch: ${version}/${pkg.version}`);
}
console.log(`pi-cx package verified: ${pkg.version} ${config.target} (${Object.keys(manifest.files).length} files, ${native ? "native" : "static"})`);
