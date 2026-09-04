import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import piCx from "../../extensions/pi-cx/index.js";
import { SUPPORTED_PLATFORMS, currentPlatformConfig, grammarFilename } from "../../extensions/pi-cx/platform.js";

test("Cargo and Pi package versions match and vendor is ignored", async () => {
  const pkg = JSON.parse(await readFile("package.json", "utf8")); const cargo = await readFile("Cargo.toml", "utf8"); const ignore = await readFile(".gitignore", "utf8");
  assert.equal(pkg.version, cargo.match(/^version = "([^"]+)"/m)?.[1]); assert.match(pkg.pi.extensions[0], /extensions\/pi-cx\/index\.ts/); assert.match(ignore, /vendor\/pi-cx/);
});
test("platform matrix covers all release targets with native filenames", () => {
  assert.equal(SUPPORTED_PLATFORMS.length, 6);
  assert.equal(new Set(SUPPORTED_PLATFORMS.map(item => item.target)).size, 6);
  assert.equal(currentPlatformConfig("win32", "x64").binaryName, "cx.exe");
  assert.equal(grammarFilename("rust", currentPlatformConfig("linux", "arm64")), "libtree_sitter_rust.so");
  assert.equal(grammarFilename("rust", currentPlatformConfig("win32", "arm64")), "tree_sitter_rust.dll");
  assert.throws(() => currentPlatformConfig("freebsd", "x64"), /does not support/);
});

test("extension registers and activates exactly eight cx tools plus cx-status", () => {
  const tools: any[] = [], commands: string[] = [], active: string[] = ["read"];
  const api: any = {
    registerTool(tool: any) { tools.push(tool); }, registerCommand(name: string) { commands.push(name); }, registerEntryRenderer() {},
    getActiveTools() { return active; }, setActiveTools(names: string[]) { active.splice(0, active.length, ...names); },
  };
  piCx(api);
  assert.deepEqual(tools.map(t => t.name), ["cx_overview", "cx_symbols", "cx_definition", "cx_references", "cx_callers", "cx_callees", "cx_map", "cx_refresh"]);
  assert.ok(tools.every(t => active.includes(t.name))); assert.deepEqual(commands, ["cx-status"]); assert.ok(tools.every(t => !Object.hasOwn(t.parameters.properties, "root")));
});
