#!/usr/bin/env node
import { createHash } from "node:crypto";
import { chmod, cp, lstat, mkdir, mkdtemp, readFile, readdir, rename, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";

import { byHost, grammarFilename, grammarNames, languagePackVersion } from "./pi-cx-platforms.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const pkg = JSON.parse(await readFile(join(root, "package.json"), "utf8"));
const config = byHost();
const target = config.target, platformDir = config.platformDir, schema = 1;
const asset = `pi-cx-${target}.tar.gz`;
const digest = async (path) => createHash("sha256").update(await readFile(path)).digest("hex");
const exec = (command, args, cwd) => new Promise((resolve, reject) => {
  const child = spawn(command, args, { cwd, shell: false, stdio: ["ignore", "pipe", "pipe"] }); const out = [], err = [];
  child.stdout.on("data", c => out.push(c)); child.stderr.on("data", c => err.push(c)); child.once("error", reject);
  child.once("close", code => code === 0 ? resolve(Buffer.concat(out).toString()) : reject(new Error(`${command} exited ${code}: ${Buffer.concat(err).toString().slice(-1000)}`)));
});
async function walk(dir, prefix = "") {
  const files = [];
  for (const name of await readdir(dir)) {
    const rel = prefix ? `${prefix}/${name}` : name, path = join(dir, name), info = await lstat(path);
    if (info.isSymbolicLink()) throw new Error(`asset contains symlink: ${rel}`);
    if (info.isDirectory()) files.push(...await walk(path, rel)); else if (info.isFile()) files.push(rel); else throw new Error(`asset contains special file: ${rel}`);
  }
  return files;
}
async function validate(stage) {
  const manifest = JSON.parse(await readFile(join(stage, "manifest.json"), "utf8"));
  const failures = [];
  if (manifest.format_version !== 1) failures.push("format_version");
  if (manifest.package !== "pi-cx") failures.push("package");
  if (manifest.cx_version !== pkg.version) failures.push("cx_version");
  if (manifest.cx_schema_version !== schema) failures.push("cx_schema_version");
  if (manifest.target !== target) failures.push("target");
  if (manifest.tree_sitter_language_pack_version !== languagePackVersion) failures.push("language_pack");
  const binaryPath = `bin/${config.binaryName}`;
  if (!manifest.files?.[binaryPath]) failures.push(binaryPath);
  const expectedNative = new Set([binaryPath, ...grammarNames.map(name => `grammars/${grammarFilename(name, config)}`)]);
  for (const path of Object.keys(manifest.files ?? {})) if (!expectedNative.has(path)) failures.push(`unexpected file ${path}`);
  for (const path of expectedNative) if (!manifest.files?.[path]) failures.push(`missing file ${path}`);
  if (failures.length) throw new Error(`incompatible pi-cx manifest: ${failures.join(", ")}`);
  const actual = new Set(await walk(stage));
  const expected = new Set(["manifest.json", ...Object.keys(manifest.files)]);
  for (const path of actual) if (!expected.has(path)) throw new Error(`unlisted asset file: ${path}`);
  for (const [path, meta] of Object.entries(manifest.files)) {
    if (path.startsWith("/") || path.split(/[\\/]/).includes("..")) throw new Error(`unsafe manifest path: ${path}`);
    if (!actual.has(path)) throw new Error(`missing asset file: ${path}`);
    const full = join(stage, path), info = await stat(full);
    if (info.size !== meta.bytes || await digest(full) !== meta.sha256) throw new Error(`asset checksum/size mismatch: ${path}`);
  }
  if (config.platform !== "win32") await chmod(join(stage, binaryPath), 0o755);
  const version = await exec(join(stage, binaryPath), ["--version"], stage);
  if (!new RegExp(`\\b${pkg.version.replaceAll(".", "\\.")}\\b`).test(version)) throw new Error(`cx --version mismatch: ${version.trim()}`);
}
const work = await mkdtemp(join(root, ".pi-cx-install-"));
try {
  const stage = join(work, "stage"); await mkdir(stage);
  if (process.env.PI_CX_ASSET_DIR) {
    const source = resolve(process.env.PI_CX_ASSET_DIR);
    if (source === root || relative(source, root) === "") throw new Error("PI_CX_ASSET_DIR must name an explicit staged asset directory");
    await cp(source, stage, { recursive: true, force: false });
  } else {
    const base = `https://github.com/AllenDang/cx/releases/download/v${pkg.version}`;
    const archivePath = join(work, asset), checksumPath = `${archivePath}.sha256`;
    for (const [url, path] of [[`${base}/${asset}`, archivePath], [`${base}/${asset}.sha256`, checksumPath]]) {
      const response = await fetch(url, { redirect: "follow" }); if (!response.ok) throw new Error(`download failed ${response.status}: ${url}`);
      await writeFile(path, Buffer.from(await response.arrayBuffer()), { mode: 0o600 });
    }
    const expected = (await readFile(checksumPath, "utf8")).trim().match(/^([a-fA-F0-9]{64})(?:\s+\*?([^\s]+))?$/);
    if (!expected || (expected[2] && basename(expected[2]) !== asset)) throw new Error("invalid archive checksum file");
    if (await digest(archivePath) !== expected[1].toLowerCase()) throw new Error("archive checksum mismatch");
    const entries = (await exec("tar", ["-tzf", archivePath], work)).split("\n").filter(Boolean);
    for (const entry of entries) if (entry.startsWith("/") || entry.split("/").includes("..")) throw new Error(`unsafe archive entry: ${entry}`);
    const verboseEntries = (await exec("tar", ["-tvzf", archivePath], work)).split("\n").filter(Boolean);
    for (const entry of verboseEntries) if (!["-", "d"].includes(entry.trimStart()[0])) throw new Error(`archive links and special files are forbidden: ${entry}`);
    await exec("tar", ["-xzf", archivePath, "-C", stage], work);
  }
  await validate(stage);
  const parent = join(root, "vendor", "pi-cx"), final = join(parent, platformDir), backup = join(parent, `.${platformDir}.old-${process.pid}`);
  await mkdir(parent, { recursive: true });
  let hadFinal = false; try { await rename(final, backup); hadFinal = true; } catch (error) { if (error.code !== "ENOENT") throw error; }
  try { await rename(stage, final); } catch (error) { if (hadFinal) await rename(backup, final); throw error; }
  if (hadFinal) await rm(backup, { recursive: true, force: true });
  process.stdout.write(`pi-cx ${pkg.version}: installed verified ${target} asset\n`);
} finally { await rm(work, { recursive: true, force: true }); }
