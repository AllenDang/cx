import { copyFile, mkdir, open, readdir, readFile, rename, rm, stat } from "node:fs/promises";
import { homedir } from "node:os";
import { basename, join } from "node:path";
import { createHash } from "node:crypto";
import type { AssetManifest } from "./types.js";
import { VENDOR_ROOT, sha256File } from "./binary.js";

export function cxCacheDir(): string {
  if (process.env.CX_CACHE_DIR) return process.env.CX_CACHE_DIR;
  if (process.platform === "darwin") return join(homedir(), "Library", "Caches", "cx");
  if (process.platform === "win32") return join(process.env.LOCALAPPDATA ?? join(homedir(), "AppData", "Local"), "cx");
  return join(process.env.XDG_CACHE_HOME ?? join(homedir(), ".cache"), "cx");
}

async function acquireLock(lock: string): Promise<() => Promise<void>> {
  await mkdir(join(lock, ".."), { recursive: true });
  for (let i = 0; i < 100; i++) {
    try { await mkdir(lock); return () => rm(lock, { recursive: true, force: true }); } catch (error) {
      const code = (error as NodeJS.ErrnoException).code;
      if (code !== "EEXIST") throw error;
      try {
        if (Date.now() - (await stat(lock)).mtimeMs > 120_000) await rm(lock, { recursive: true, force: true });
      } catch { /* another process released it */ }
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
  }
  throw new Error(`timed out acquiring cx grammar cache lock: ${lock}`);
}

export interface GrammarSeedResult { copied: string[]; replaced: string[]; unchanged: string[] }

export async function ensureBundledGrammars(manifest: AssetManifest, vendorRoot = VENDOR_ROOT, cache = cxCacheDir()): Promise<GrammarSeedResult> {
  const grammarEntries = Object.entries(manifest.files).filter(([path]) => path.startsWith("grammars/") && /\.(dylib|so|dll)$/.test(path));
  if (!grammarEntries.length) throw new Error("pi-cx asset manifest contains no bundled grammars");
  const targetDir = join(cache, "grammars");
  await mkdir(targetDir, { recursive: true });
  const release = await acquireLock(join(cache, ".pi-cx-grammar-seed.lock"));
  const result: GrammarSeedResult = { copied: [], replaced: [], unchanged: [] };
  try {
    for (const [path, expected] of grammarEntries) {
      const source = join(vendorRoot, path);
      if (await sha256File(source) !== expected.sha256 || (await stat(source)).size !== expected.bytes) throw new Error(`bundled grammar checksum mismatch: ${path}; reinstall pi-cx`);
      const target = join(targetDir, basename(path));
      let existed = false;
      try {
        existed = true;
        if (await sha256File(target) === expected.sha256) { result.unchanged.push(basename(path)); continue; }
      } catch { existed = false; }
      const temp = join(targetDir, `.${basename(path)}.${process.pid}.${Date.now()}.tmp`);
      await copyFile(source, temp);
      const handle = await open(temp, "r");
      try { await handle.sync(); } finally { await handle.close(); }
      await rename(temp, target);
      (existed ? result.replaced : result.copied).push(basename(path));
      process.stderr.write(`pi-cx: ${existed ? "replaced" : "seeded"} ${target}\n`);
    }
  } finally { await release(); }
  return result;
}

export async function grammarStatus(manifest: AssetManifest, vendorRoot = VENDOR_ROOT, cache = cxCacheDir()) {
  const targetDir = join(cache, "grammars");
  const bundled = [];
  for (const [path, expected] of Object.entries(manifest.files).filter(([p]) => p.startsWith("grammars/"))) {
    const name = basename(path);
    let state = "available in bundle, not seeded";
    try { state = await sha256File(join(targetDir, name)) === expected.sha256 ? "installed, digest ok" : "installed, digest mismatch"; } catch { /* absent */ }
    bundled.push({ name, state });
  }
  let other: string[] = [];
  try {
    const known = new Set(bundled.map((item) => item.name));
    other = (await readdir(targetDir)).filter((name) => /\.(dylib|so|dll)$/.test(name) && !known.has(name));
  } catch { /* absent cache */ }
  return { cache, bundled, other };
}
