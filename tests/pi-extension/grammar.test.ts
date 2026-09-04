import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { ensureBundledGrammars } from "../../extensions/pi-cx/grammars.js";
import type { AssetManifest } from "../../extensions/pi-cx/types.js";

const sha = (value: string) => createHash("sha256").update(value).digest("hex");
test("seeds, reuses, and atomically repairs bundled grammars without deleting extras", async () => {
  const vendor = await mkdtemp(join(tmpdir(), "pi-cx-vendor-")), cache = await mkdtemp(join(tmpdir(), "pi-cx-cache-"));
  await mkdir(join(vendor, "grammars")); await writeFile(join(vendor, "grammars", "libtree_sitter_rust.dylib"), "good");
  const manifest = { files: { "grammars/libtree_sitter_rust.dylib": { sha256: sha("good"), bytes: 4 } } } as unknown as AssetManifest;
  const first = await ensureBundledGrammars(manifest, vendor, cache); assert.deepEqual(first.copied, ["libtree_sitter_rust.dylib"]);
  await writeFile(join(cache, "grammars", "other.dylib"), "keep");
  assert.deepEqual((await ensureBundledGrammars(manifest, vendor, cache)).unchanged, ["libtree_sitter_rust.dylib"]);
  await writeFile(join(cache, "grammars", "libtree_sitter_rust.dylib"), "bad");
  assert.deepEqual((await ensureBundledGrammars(manifest, vendor, cache)).replaced, ["libtree_sitter_rust.dylib"]);
  assert.equal(await readFile(join(cache, "grammars", "other.dylib"), "utf8"), "keep");
});
