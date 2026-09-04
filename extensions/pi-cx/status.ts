import { constants } from "node:fs";
import { access, stat } from "node:fs/promises";
import { spawn } from "node:child_process";
import { Text } from "@earendil-works/pi-tui";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { BINARY_PATH, PACKAGE_ROOT, loadAssetManifest, sha256File, validateBundledBinary } from "./binary.js";
import { canonicalRoot } from "./paths.js";
import { cxCacheDir, grammarStatus } from "./grammars.js";
import { CX_SCHEMA_VERSION, PACKAGE_VERSION, TARGET } from "./types.js";

async function capture(binary: string, args: string[], cwd: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const child = spawn(binary, args, { cwd, shell: false, env: { HOME: process.env.HOME, PATH: process.env.PATH, TMPDIR: process.env.TMPDIR, CX_CACHE_DIR: process.env.CX_CACHE_DIR }, stdio: ["ignore", "pipe", "pipe"] });
    const out: Buffer[] = []; child.stdout.on("data", (chunk) => out.push(chunk)); child.once("error", reject); child.once("close", (code) => code === 0 ? resolve(Buffer.concat(out).toString().trim()) : reject(new Error(`cx exited ${code}`)));
  });
}

export async function buildStatusReport(cwd: string): Promise<{ ok: boolean; text: string }> {
  const lines = [`pi-cx ${PACKAGE_VERSION}`, `package root: ${PACKAGE_ROOT}`, `target: ${TARGET}`, `host: ${process.platform}/${process.arch}`, `binary: ${BINARY_PATH}`, `expected schema: ${CX_SCHEMA_VERSION}`];
  let ok = process.platform === "darwin" && process.arch === "arm64";
  if (!ok) lines.push("FAIL platform: only darwin/arm64 is supported; PATH cx fallback is disabled");
  let manifest;
  try {
    manifest = await loadAssetManifest();
    const digest = await sha256File(BINARY_PATH);
    lines.push(`binary digest: ${digest === manifest.files["bin/cx"]?.sha256 ? "OK" : "FAIL"} (${digest})`);
    const validation = await validateBundledBinary();
    lines.push(`cx version: ${validation.version}`, `probe schema: ${CX_SCHEMA_VERSION} (OK)`);
  } catch (error) { ok = false; lines.push(`FAIL binary/manifest/probe: ${error instanceof Error ? error.message : String(error)}`); }
  const root = await canonicalRoot(cwd).catch((error) => { ok = false; lines.push(`FAIL project root: ${String(error)}`); return cwd; });
  lines.push(`project root: ${root}`);
  const cache = cxCacheDir();
  let writable = false;
  try { await access(cache, constants.W_OK); writable = true; } catch { /* cache may not exist */ }
  lines.push(`cx cache: ${cache}`, `cache writable: ${writable ? "yes" : "no/not created"}`);
  if (manifest) {
    const grammars = await grammarStatus(manifest);
    for (const item of grammars.bundled) lines.push(`grammar ${item.name}: ${item.state}`);
    lines.push(`other grammars: ${grammars.other.length ? grammars.other.join(", ") : "none"}`);
  }
  try {
    const index = await capture(BINARY_PATH, ["cache", "path", "--root", root], root);
    let size = 0, exists = false; try { size = (await stat(index)).size; exists = true; } catch { /* absent */ }
    lines.push(`project index: ${index}`, `index exists: ${exists ? `yes (${size} bytes)` : "no"}`);
  } catch (error) { ok = false; lines.push(`FAIL index path diagnostic: ${String(error)}`); }
  lines.push(`overall: ${ok ? "OK" : "FAIL"}`);
  return { ok, text: lines.join("\n") };
}

export function registerStatus(pi: ExtensionAPI): void {
  pi.registerEntryRenderer("pi-cx-status", (entry: any, _options: any, theme: any) => new Text(theme.fg(entry.data.ok ? "success" : "error", entry.data.text), 0, 0));
  pi.registerCommand("cx-status", {
    description: "Diagnose the bundled pi-cx binary, schema, cache, grammars, and project index",
    handler: async (_args, ctx) => {
      const report = await buildStatusReport(ctx.cwd);
      pi.appendEntry("pi-cx-status", report);
      if (ctx.hasUI) ctx.ui.notify(report.ok ? "pi-cx status: OK" : "pi-cx status: failures found", report.ok ? "info" : "error");
      else if (ctx.mode === "print") process.stdout.write(`${report.text}\n`);
    },
  });
}
