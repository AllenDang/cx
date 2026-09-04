#!/usr/bin/env node
import { createHash } from "node:crypto";
import { readFile, stat } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const cargo = await readFile(join(root, "Cargo.toml"), "utf8");
const cargoVersion = cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
const pkg = JSON.parse(await readFile(join(root, "package.json"), "utf8"));
if (!cargoVersion || cargoVersion !== pkg.version) throw new Error(`Cargo/package version mismatch: ${cargoVersion}/${pkg.version}`);
const tag = process.env.GITHUB_REF_NAME ?? process.env.PI_CX_TAG;
if (tag && tag.replace(/^v/, "") !== pkg.version) throw new Error(`tag/package version mismatch: ${tag}/${pkg.version}`);
const stage = resolve(process.env.PI_CX_ASSET_DIR ?? join(root, "vendor", "pi-cx", "darwin-arm64"));
const manifest = JSON.parse(await readFile(join(stage, "manifest.json"), "utf8"));
if (manifest.cx_version !== pkg.version || manifest.cx_schema_version !== 1 || manifest.target !== "aarch64-apple-darwin" || manifest.tree_sitter_language_pack_version !== "1.3.1") throw new Error("asset manifest version/schema/target mismatch");
for (const [path, meta] of Object.entries(manifest.files)) {
  const full = join(stage, path), data = await readFile(full);
  if ((await stat(full)).size !== meta.bytes || createHash("sha256").update(data).digest("hex") !== meta.sha256) throw new Error(`asset verification failed: ${path}`);
}
const version = await new Promise((resolveVersion, reject) => { const child = spawn(join(stage, "bin", "cx"), ["--version"], { shell: false }); let out = ""; child.stdout.on("data", c => out += c); child.once("error", reject); child.once("close", code => code === 0 ? resolveVersion(out.trim()) : reject(new Error(`cx --version exited ${code}`))); });
if (!version.includes(pkg.version)) throw new Error(`binary/package version mismatch: ${version}/${pkg.version}`);
console.log(`pi-cx package verified: ${pkg.version} (${Object.keys(manifest.files).length} files)`);
