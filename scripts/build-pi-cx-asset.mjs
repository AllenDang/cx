#!/usr/bin/env node
import { createHash } from "node:crypto";
import { chmod, copyFile, mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import { byHost, byTarget, grammarFilename, grammarNames, languagePackVersion } from "./pi-cx-platforms.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const pkg = JSON.parse(await readFile(join(root, "package.json"), "utf8"));
const target = process.argv[2] ?? process.env.PI_CX_TARGET;
if (!target) throw new Error("usage: build-pi-cx-asset.mjs <rust-target> [binary] [output-dir]");
const config = byTarget(target);
const binary = resolve(process.argv[3] ?? process.env.PI_CX_BINARY ?? join(root, "target", target, "release", config.binaryName));
const out = resolve(process.argv[4] ?? process.env.PI_CX_OUT_DIR ?? join(root, "dist"));
const digestBuffer = value => createHash("sha256").update(value).digest("hex");
const digestFile = async path => digestBuffer(await readFile(path));
const exec = (command, args, cwd, env = process.env) => new Promise((resolveExec, reject) => {
  const child = spawn(command, args, { cwd, env, shell: false, windowsHide: true, stdio: ["ignore", "pipe", "pipe"] }); const stdout = [], stderr = [];
  child.stdout.on("data", chunk => stdout.push(chunk)); child.stderr.on("data", chunk => stderr.push(chunk)); child.once("error", reject);
  child.once("close", code => code === 0 ? resolveExec(Buffer.concat(stdout).toString()) : reject(new Error(`${command} exited ${code}: ${Buffer.concat(stderr).toString().slice(-2000)}`)));
});
async function download(url, label) {
  let lastError;
  for (let attempt = 1; attempt <= 4; attempt++) {
    try {
      const response = await fetch(url, { redirect: "follow" });
      if (!response.ok) throw new Error(`${label} download failed: ${response.status}`);
      return Buffer.from(await response.arrayBuffer());
    } catch (error) {
      lastError = error;
      if (attempt < 4) await new Promise(resolve => setTimeout(resolve, attempt * 1000));
    }
  }
  throw new Error(`${label} download failed after retries`, { cause: lastError });
}
const info = await stat(binary).catch(() => undefined);
if (!info?.isFile()) throw new Error(`missing target binary: ${binary}`);
const work = await mkdtemp(join(tmpdir(), "pi-cx-build-"));
try {
  const stage = join(work, "stage"), parsers = join(work, "parsers"); await mkdir(join(stage, "bin"), { recursive: true }); await mkdir(join(stage, "grammars")); await mkdir(parsers);
  const manifestUrl = `https://github.com/kreuzberg-dev/tree-sitter-language-pack/releases/download/v${languagePackVersion}/parsers.json`;
  const parserManifest = JSON.parse((await download(manifestUrl, "parser manifest")).toString("utf8"));
  if (parserManifest.version !== languagePackVersion) throw new Error(`parser manifest version mismatch: ${parserManifest.version}`);
  const bundle = parserManifest.platforms?.[config.bundleKey]; if (!bundle) throw new Error(`language pack ${languagePackVersion} has no bundle for ${config.bundleKey}`);
  const bundleData = await download(bundle.url, `parser bundle ${config.bundleKey}`);
  if (bundleData.length !== bundle.size || digestBuffer(bundleData) !== bundle.sha256) throw new Error(`parser bundle checksum/size mismatch for ${config.bundleKey}`);
  const bundlePath = join(work, "parsers.tar.zst"); await writeFile(bundlePath, bundleData, { mode: 0o600 });
  await exec("tar", ["-xf", "parsers.tar.zst", "-C", "parsers"], work);
  await copyFile(binary, join(stage, "bin", config.binaryName));
  if (config.platform !== "win32") await chmod(join(stage, "bin", config.binaryName), 0o755);
  for (const name of grammarNames) {
    const filename = grammarFilename(name, config), source = join(parsers, filename);
    if (!(await stat(source).catch(() => undefined))?.isFile()) throw new Error(`parser bundle missing ${filename}`);
    await copyFile(source, join(stage, "grammars", filename));
  }
  const native = (() => { try { return byHost().target === target; } catch { return false; } })();
  if (native) {
    const stagedBinary = join(stage, "bin", config.binaryName);
    const version = await exec(stagedBinary, ["--version"], stage);
    if (!version.includes(pkg.version)) throw new Error(`binary/package version mismatch: ${version.trim()}/${pkg.version}`);
    const fixture = join(work, "probe"); await mkdir(fixture); await writeFile(join(fixture, "README.md"), "# Probe\n");
    const probe = JSON.parse(await exec(stagedBinary, ["overview", "README.md", "--root", fixture, "--json", "--limit", "1"], fixture, { HOME: process.env.HOME, PATH: process.env.PATH, TMPDIR: process.env.TMPDIR, CX_CACHE_DIR: join(work, "probe-cache") }));
    if (probe.schema_version !== 1 || probe.error !== null) throw new Error("native cx schema probe failed");
  }
  await exec(process.execPath, [join(root, "scripts", "make-pi-cx-manifest.mjs"), stage, pkg.version, target, languagePackVersion], root);
  await mkdir(out, { recursive: true });
  const archive = join(out, `pi-cx-${target}.tar.gz`), temporaryArchive = join(work, "pi-cx-asset.tar.gz");
  await exec("tar", ["-czf", "pi-cx-asset.tar.gz", "-C", "stage", "manifest.json", "bin", "grammars"], work);
  await copyFile(temporaryArchive, archive);
  await writeFile(`${archive}.sha256`, `${await digestFile(archive)}  ${basename(archive)}\n`);
  console.log(JSON.stringify({ archive, target, nativeVerified: native, files: 8 }));
} finally { await rm(work, { recursive: true, force: true }); }
