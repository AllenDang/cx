import assert from "node:assert/strict";
import { mkdtemp, mkdir, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { canonicalRoot, projectPath } from "../../extensions/pi-cx/paths.js";
import { buildDefinitionArgs, buildMapArgs, buildOverviewArgs, buildRefreshArgs, buildSymbolsArgs, schemas } from "../../extensions/pi-cx/tools.js";

test("schemas expose no root or all and use string enums", () => {
  for (const schema of Object.values(schemas)) {
    assert.equal("root" in (schema.properties ?? {}), false);
    assert.equal("all" in (schema.properties ?? {}), false);
  }
  assert.deepEqual((schemas.symbols.properties.kind as any).anyOf, undefined);
  assert.ok((schemas.symbols.properties.kind as any).enum || (schemas.symbols.properties.kind as any).anyOf === undefined);
});
test("flag builders map camelCase mechanically and impose limits", async () => {
  const dir = await mkdtemp(join(tmpdir(), "pi-cx-tools-")); await writeFile(join(dir, "a.rs"), "fn a() {}\n");
  const root = await canonicalRoot(dir);
  assert.deepEqual((await buildOverviewArgs(root, { path: "@a.rs", full: true })).slice(0, 2), ["a.rs", "--full"]);
  assert.deepEqual(await buildDefinitionArgs(root, { name: "a" }), ["--name", "a", "--role", "definition", "--max-lines", "200", "--fresh", "metadata", "--limit", "3"]);
  assert.throws(() => buildMapArgs({ exclude: ["bad\0glob"] }), /NUL/);
  assert.deepEqual(buildMapArgs({ includeVendor: true, depth: 2 }).slice(0, 3), ["--depth", "2", "--include-vendor"]);
  await assert.rejects(() => buildSymbolsArgs(root, {}), /requires at least one filter/);
});
test("path normalization blocks traversal and symlink escape", async () => {
  const dir = await mkdtemp(join(tmpdir(), "pi-cx-root-")); const outside = await mkdtemp(join(tmpdir(), "pi-cx-out-"));
  await mkdir(join(dir, "src")); await writeFile(join(dir, "src", "a.rs"), ""); await symlink(outside, join(dir, "escape"));
  const root = await canonicalRoot(dir);
  assert.equal(await projectPath(root, "@src/a.rs"), "src/a.rs");
  await assert.rejects(() => projectPath(root, "../outside"), /escapes project root/);
  await assert.rejects(() => projectPath(root, "escape"), /symlink escapes/);
  assert.deepEqual(await buildRefreshArgs(root, { paths: ["src/new.rs"] }), ["src/new.rs"]);
});
