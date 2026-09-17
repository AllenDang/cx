import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdtemp, mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";
import test from "node:test";

const exec = promisify(execFile);

async function fixture() {
  const work = await mkdtemp(join(tmpdir(), "cx-ci-grammars-"));
  const source = join(work, "source"), nested = join(source, "pack", "v1", "libs"), destination = join(work, "output");
  await mkdir(nested, { recursive: true });
  return { source, nested, destination };
}

test("CI grammar preparation handles flat/versioned caches and every native filename convention", async () => {
  const { source, nested, destination } = await fixture();
  await writeFile(join(source, "libtree_sitter_rust.so"), "linux");
  await writeFile(join(nested, "libtree_sitter_rust.dylib"), "mac");
  await writeFile(join(nested, "tree_sitter_rust.dll"), "windows");
  await writeFile(join(nested, "manifest.json"), "{}");
  await exec(process.execPath, ["scripts/prepare-ci-grammars.mjs", source, destination]);
  assert.deepEqual((await readdir(destination)).sort(), ["libtree_sitter_rust.dylib", "libtree_sitter_rust.so", "tree_sitter_rust.dll"]);
  assert.equal(await readFile(join(destination, "tree_sitter_rust.dll"), "utf8"), "windows");
});

test("CI grammar preparation fails when installation produced no native libraries", async () => {
  const { source, destination } = await fixture();
  await assert.rejects(() => exec(process.execPath, ["scripts/prepare-ci-grammars.mjs", source, destination]), /no native grammar libraries/);
});

test("CI grammar preparation deduplicates identical files but refuses conflicting versions", async () => {
  const { source, nested, destination } = await fixture();
  await writeFile(join(source, "libtree_sitter_rust.so"), "same");
  await writeFile(join(nested, "libtree_sitter_rust.so"), "same");
  await exec(process.execPath, ["scripts/prepare-ci-grammars.mjs", source, destination]);
  assert.equal((await readdir(destination)).length, 1);
  await writeFile(join(nested, "libtree_sitter_rust.so"), "different");
  await assert.rejects(() => exec(process.execPath, ["scripts/prepare-ci-grammars.mjs", source, destination]), /conflicting grammar copies/);
  assert.equal(await readFile(join(destination, "libtree_sitter_rust.so"), "utf8"), "same");
});
