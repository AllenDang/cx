import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import test from "node:test";
import piCx from "../../extensions/pi-cx/index.js";
import { SUPPORTED_PLATFORMS, currentPlatformConfig, grammarFilename } from "../../extensions/pi-cx/platform.js";
import { PACKAGE_VERSION } from "../../extensions/pi-cx/types.js";

const exec = promisify(execFile);

test("Cargo and Pi package versions match and vendor is ignored", async () => {
  const pkg = JSON.parse(await readFile("package.json", "utf8")); const cargo = await readFile("Cargo.toml", "utf8"); const ignore = await readFile(".gitignore", "utf8");
  assert.equal(pkg.version, cargo.match(/^version = "([^"]+)"/m)?.[1]); assert.match(pkg.pi.extensions[0], /extensions\/pi-cx\/index\.ts/); assert.match(ignore, /vendor\/pi-cx/);
  assert.equal(pkg.version, PACKAGE_VERSION);
  const lock = JSON.parse(await readFile("package-lock.json", "utf8"));
  assert.equal(lock.version, pkg.version); assert.equal(lock.packages[""].version, pkg.version);
  const cargoLock = await readFile("Cargo.lock", "utf8");
  assert.equal(cargoLock.match(/name = "cx-cli"\nversion = "([^"]+)"/)?.[1], pkg.version);
});
test("platform matrix covers all release targets with native filenames", () => {
  assert.equal(SUPPORTED_PLATFORMS.length, 6);
  assert.equal(new Set(SUPPORTED_PLATFORMS.map(item => item.target)).size, 6);
  assert.equal(currentPlatformConfig("win32", "x64").binaryName, "cx.exe");
  assert.equal(grammarFilename("rust", currentPlatformConfig("linux", "arm64")), "libtree_sitter_rust.so");
  assert.equal(grammarFilename("rust", currentPlatformConfig("win32", "arm64")), "tree_sitter_rust.dll");
  assert.throws(() => currentPlatformConfig("freebsd", "x64"), /does not support/);
});

test("release matrix and installer URL helpers agree on every platform asset", async () => {
  // @ts-expect-error Release scripts are plain ESM and intentionally have no declaration file.
  const release = await import("../../scripts/pi-cx-platforms.mjs") as any;
  const workflow = await readFile(".github/workflows/release.yml", "utf8");
  const pkg = JSON.parse(await readFile("package.json", "utf8"));
  assert.equal(release.platforms.length, 6);
  for (const platform of release.platforms) {
    const asset = `pi-cx-${platform.target}.tar.gz`;
    assert.equal(release.piAssetFilename(platform.target), asset);
    assert.equal(
      release.githubPiAssetUrl(pkg.version, platform.target),
      `https://github.com/AllenDang/cx/releases/download/v${pkg.version}/${asset}`,
    );
    assert.equal(release.githubPiAssetUrl(pkg.version, platform.target, true), `https://github.com/AllenDang/cx/releases/download/v${pkg.version}/${asset}.sha256`);
    assert.match(workflow, new RegExp(`target: ${platform.target.replaceAll("-", "\\-")}`));
  }
});

test("manifest generation is idempotent and never hashes its previous manifest", async () => {
  const stage=await mkdtemp(join(tmpdir(),"pi-cx-manifest-"));await mkdir(join(stage,"bin"));
  await writeFile(join(stage,"bin","cx"),"binary");
  const args=["scripts/make-pi-cx-manifest.mjs",stage,"0.8.0","fixture-target","1.16.1"];
  await exec("node",args);const first=await readFile(join(stage,"manifest.json"),"utf8");
  await exec("node",args);const second=await readFile(join(stage,"manifest.json"),"utf8");
  assert.equal(second,first);assert.equal(Object.hasOwn(JSON.parse(second).files,"manifest.json"),false);
});

test("extension factory uses only load-safe registration methods", () => {
  const tools: any[] = [], commands: string[] = [], handlers: string[] = [], eventChannels: string[] = [];
  const api: any = {
    registerTool(tool: any) { tools.push(tool); }, registerCommand(name: string) { commands.push(name); }, registerEntryRenderer() {},
    on(name: string) { handlers.push(name); }, events: { on(name: string) { eventChannels.push(name); return () => {}; } },
    getActiveTools() { throw new Error("action method called during extension loading"); },
    setActiveTools() { throw new Error("action method called during extension loading"); },
  };
  assert.doesNotThrow(() => piCx(api));
  assert.deepEqual(tools.map(t => t.name), ["cx_overview", "cx_symbols", "cx_definition", "cx_references", "cx_callers", "cx_callees", "cx_map", "cx_refresh", "cx_impact", "cx_changes", "cx_context"]);
  assert.deepEqual(commands, ["cx-status"]); assert.ok(tools.every(t => !Object.hasOwn(t.parameters.properties, "root")));
  assert.deepEqual(eventChannels, ["cx:mark-dirty:v1"]);
  assert.deepEqual(handlers, ["session_start", "session_shutdown", "before_agent_start"]);
});
