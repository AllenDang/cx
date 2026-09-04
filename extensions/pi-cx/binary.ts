import { constants } from "node:fs";
import { access, mkdtemp, readFile, realpath, rm, stat, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { dirname, join, relative } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import { parseEnvelope, validateManifest } from "./protocol.js";
import { PACKAGE_VERSION, PLATFORM, PLATFORM_DIR, type AssetManifest } from "./types.js";

export const PACKAGE_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
export const VENDOR_ROOT = join(PACKAGE_ROOT, "vendor", "pi-cx", PLATFORM_DIR);
export const BINARY_PATH = join(VENDOR_ROOT, "bin", PLATFORM.binaryName);
export const MANIFEST_PATH = join(VENDOR_ROOT, "manifest.json");

export async function sha256File(path: string): Promise<string> {
  const data = await readFile(path);
  return createHash("sha256").update(data).digest("hex");
}

export async function loadAssetManifest(path = MANIFEST_PATH): Promise<AssetManifest> {
  return validateManifest(JSON.parse(await readFile(path, "utf8")));
}

async function execCapture(binary: string, args: string[], cwd: string, env = process.env): Promise<{ code: number | null; stdout: string; stderr: string }> {
  return new Promise((resolve, reject) => {
    const child = spawn(binary, args, { cwd, env, shell: false, stdio: ["ignore", "pipe", "pipe"] });
    const stdout: Buffer[] = [], stderr: Buffer[] = [];
    child.stdout.on("data", (c) => stdout.push(c)); child.stderr.on("data", (c) => stderr.push(c));
    child.once("error", reject); child.once("close", (code) => resolve({ code, stdout: Buffer.concat(stdout).toString(), stderr: Buffer.concat(stderr).toString() }));
  });
}

export interface BinaryValidation { binary: string; version: string; digest: string; manifest: AssetManifest }
let cached: Promise<BinaryValidation> | undefined;

export function resetBinaryValidationForTests(): void { cached = undefined; }

export function validateBundledBinary(options: { binary?: string; manifestPath?: string; vendorRoot?: string } = {}): Promise<BinaryValidation> {
  if (!options.binary && !options.manifestPath && !options.vendorRoot) return cached ??= validateBundledBinaryUncached(options);
  return validateBundledBinaryUncached(options);
}

async function validateBundledBinaryUncached(options: { binary?: string; manifestPath?: string; vendorRoot?: string }): Promise<BinaryValidation> {
  const binary = options.binary ?? BINARY_PATH;
  const manifestPath = options.manifestPath ?? MANIFEST_PATH;
  const vendorRoot = await realpath(options.vendorRoot ?? VENDOR_ROOT);
  const actual = await realpath(binary);
  const rel = relative(vendorRoot, actual);
  if (rel.startsWith("..") || rel === "") throw new Error("bundled cx realpath escapes vendor root; reinstall pi-cx");
  await access(actual, constants.X_OK);
  const manifest = await loadAssetManifest(manifestPath);
  const binaryManifestPath = `bin/${PLATFORM.binaryName}`;
  const digest = await sha256File(actual);
  if (digest !== manifest.files[binaryManifestPath]!.sha256) throw new Error("bundled cx checksum mismatch; reinstall pi-cx");
  if ((await stat(actual)).size !== manifest.files[binaryManifestPath]!.bytes) throw new Error("bundled cx size mismatch; reinstall pi-cx");
  const versionResult = await execCapture(actual, ["--version"], vendorRoot);
  const expected = PACKAGE_VERSION;
  const version = versionResult.stdout.trim().match(/\b(\d+\.\d+\.\d+)\b/)?.[1];
  if (versionResult.code !== 0 || version !== expected) throw new Error(`bundled cx version mismatch (${version ?? "unknown"}; expected ${expected}); reinstall pi-cx`);

  const fixture = await mkdtemp(join(tmpdir(), "pi-cx-probe-"));
  try {
    await writeFile(join(fixture, "README.md"), "# Probe\n");
    const cache = join(fixture, "cache");
    const probe = await execCapture(actual, ["overview", "README.md", "--root", fixture, "--json", "--limit", "1"], fixture, {
      HOME: process.env.HOME, PATH: process.env.PATH, TMPDIR: process.env.TMPDIR, CX_CACHE_DIR: cache,
    });
    if (probe.code !== 0) throw new Error(`bundled cx probe failed: ${probe.stderr.slice(-500)}`);
    parseEnvelope(probe.stdout);
  } finally { await rm(fixture, { recursive: true, force: true }); }
  return { binary: actual, version: expected, digest, manifest };
}
