import assert from "node:assert/strict";
import { access, mkdtemp, mkdir, realpath, rename, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { DirtyPathCoordinator, MAX_DIRTY_PATHS, MAX_DIRTY_PATH_COMPONENTS, MAX_DIRTY_PATH_LENGTH, MAX_DIRTY_SOURCE_LENGTH } from "../../extensions/pi-cx/dirty-paths.js";
import { registerCxTools, type CxToolRuntime } from "../../extensions/pi-cx/tools.js";
import type { CxRunResult } from "../../extensions/pi-cx/types.js";
import { BINARY_PATH } from "../../extensions/pi-cx/binary.js";

async function fixture(): Promise<{ root: string; a: string; b: string }> {
  const created = await mkdtemp(join(tmpdir(), "pi-cx-dirty-"));
  const root = await realpath(created);
  await mkdir(join(root, "src"));
  const a = join(root, "src", "a.ts"), b = join(root, "src", "b.ts");
  await writeFile(a, "const a = 1;\n");
  await writeFile(b, "const b = 1;\n");
  return { root, a, b };
}

function payload(root: string, paths: string[]): unknown {
  return { version: 1, source: "hashline_edit", cwd: root, paths };
}

async function waitFor(predicate: () => boolean, timeoutMs = 5_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error(`timed out after ${timeoutMs}ms waiting for test operation`);
}

function result(command: string, generation: number, paths: string[] = []): CxRunResult {
  const results = command === "refresh"
    ? paths.map((file) => ({ file: file.replaceAll("\\", "/"), status: "unchanged" }))
    : [];
  const envelope = {
    schema_version: 1,
    query: { kind: command },
    freshness: { generation },
    page: { total: results.length, offset: 0, limit: 10, truncated: false },
    results, warnings: [], next_queries: [], error: null,
  };
  return { raw: JSON.stringify(envelope), envelope, details: { durationMs: 1, exitCode: 0, killed: false, stderr: "" } };
}

function harness(dirty: DirtyPathCoordinator, run: CxToolRuntime["run"]) {
  const tools = new Map<string, any>();
  const pi: any = { registerTool(tool: any) { tools.set(tool.name, tool); } };
  const runtime: CxToolRuntime = {
    validateBinary: async () => ({ binary: "/trusted/cx", version: "0.7.9", digest: "ok", manifest: {} as any }),
    ensureGrammars: async () => ({ copied: [], replaced: [], unchanged: [] }),
    run,
    runMaintenance: async () => {},
  };
  registerCxTools(pi, dirty, runtime);
  const ctx = (cwd: string) => ({ cwd, hasUI: false, ui: {} });
  return { tools, ctx };
}

test("dirty events validate, canonicalize, merge, and deduplicate safe project paths", async () => {
  const { root, a, b } = await fixture();
  const outside = await mkdtemp(join(tmpdir(), "pi-cx-dirty-out-"));
  const dirty = new DirtyPathCoordinator(); dirty.activate(root);

  assert.equal(dirty.markDirty(payload(root, [a, a])), 1);
  assert.equal(dirty.markDirty(payload(root, [b])), 1);
  assert.equal(dirty.markDirty(payload(root, [join(outside, "x.ts")])), 0);
  assert.equal(dirty.markDirty({ version: 2, source: "x", cwd: root, paths: [a] }), 0);
  assert.equal(dirty.markDirty({ version: 1, source: "x", cwd: root, paths: ["bad\0path"] }), 0);
  assert.deepEqual(dirty.pendingPaths(root).sort(), ["src/a.ts", "src/b.ts"]);
});

test("dirty events reject other roots and symlink escapes", async () => {
  const { root, a } = await fixture();
  const other = await mkdtemp(join(tmpdir(), "pi-cx-other-root-"));
  await writeFile(join(other, "outside.ts"), "");
  await symlink(other, join(root, "escape"));
  const dirty = new DirtyPathCoordinator(); dirty.activate(root);

  assert.equal(dirty.markDirty(payload(other, [join(other, "outside.ts")])), 0);
  assert.equal(dirty.markDirty(payload(root, [join(root, "escape", "outside.ts")])), 0);
  assert.equal(dirty.markDirty(payload(root, [a])), 1);
  assert.deepEqual(dirty.pendingPaths(root), ["src/a.ts"]);
});

test("the next query refreshes dirty paths once and reports generation metadata", async () => {
  const { root, a, b } = await fixture();
  const dirty = new DirtyPathCoordinator(); dirty.activate(root); dirty.markDirty(payload(root, [a, b, a]));
  const calls: Array<{ command: string; args: string[] }> = [];
  const h = harness(dirty, async (options) => {
    calls.push({ command: options.command, args: options.args ?? [] });
    return result(options.command, calls.length + 40, options.args);
  });

  const output = await h.tools.get("cx_overview").execute("1", {}, undefined, undefined, h.ctx(root));
  assert.deepEqual(calls, [
    { command: "refresh", args: ["src/a.ts", "src/b.ts"] },
    { command: "overview", args: [".", "--fresh", "metadata", "--limit", "100"] },
  ]);
  assert.deepEqual(output.details.dirtyRefresh, { requested: 2, refreshed: 2, generation: 41 });
  assert.deepEqual(dirty.pendingPaths(root), []);
});

test("failed automatic refresh preserves pending paths and suppresses the query", async () => {
  const { root, a } = await fixture();
  const dirty = new DirtyPathCoordinator(); dirty.activate(root); dirty.markDirty(payload(root, [a]));
  const calls: string[] = []; let fail = true;
  const h = harness(dirty, async (options) => {
    calls.push(options.command);
    if (options.command === "refresh" && fail) { fail = false; throw new Error("refresh broke"); }
    return result(options.command, calls.length, options.args);
  });

  await assert.rejects(() => h.tools.get("cx_overview").execute("1", {}, undefined, undefined, h.ctx(root)), /query was not executed/);
  assert.deepEqual(calls, ["refresh"]);
  assert.deepEqual(dirty.pendingPaths(root), ["src/a.ts"]);
  await h.tools.get("cx_overview").execute("2", {}, undefined, undefined, h.ctx(root));
  assert.deepEqual(calls, ["refresh", "refresh", "overview"]);
});

test("events arriving during refresh remain pending for the next serialized query", async () => {
  const { root, a, b } = await fixture();
  const dirty = new DirtyPathCoordinator(); dirty.activate(root); dirty.markDirty(payload(root, [a]));
  const calls: Array<{ command: string; args: string[] }> = [];
  let release!: () => void;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  let firstRefresh = true;
  const h = harness(dirty, async (options) => {
    calls.push({ command: options.command, args: options.args ?? [] });
    if (options.command === "refresh" && firstRefresh) { firstRefresh = false; await gate; }
    return result(options.command, calls.length, options.args);
  });

  const first = h.tools.get("cx_overview").execute("1", {}, undefined, undefined, h.ctx(root));
  await waitFor(() => calls.length === 1);
  dirty.markDirty(payload(root, [b]));
  release(); await first;
  assert.deepEqual(dirty.pendingPaths(root), ["src/b.ts"]);
  await h.tools.get("cx_overview").execute("2", {}, undefined, undefined, h.ctx(root));
  assert.deepEqual(calls.filter((call) => call.command === "refresh").map((call) => call.args), [["src/a.ts"], ["src/b.ts"]]);
});

test("concurrent queries serialize by root and do not duplicate the pending refresh", async () => {
  const { root, a } = await fixture();
  const dirty = new DirtyPathCoordinator(); dirty.activate(root); dirty.markDirty(payload(root, [a]));
  const calls: string[] = [];
  let release!: () => void;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  const h = harness(dirty, async (options) => {
    calls.push(options.command);
    if (options.command === "refresh") await gate;
    return result(options.command, calls.length, options.args);
  });

  const first = h.tools.get("cx_overview").execute("1", {}, undefined, undefined, h.ctx(root));
  const second = h.tools.get("cx_symbols").execute("2", { name: "a" }, undefined, undefined, h.ctx(root));
  await waitFor(() => calls.length === 1);
  assert.deepEqual(calls, ["refresh"]);
  release(); await Promise.all([first, second]);
  assert.equal(calls[0], "refresh");
  assert.deepEqual(calls.slice(1).sort(), ["overview", "symbols"]);
});

test("explicit path refresh merges pending paths and full refresh drains the snapshot", async () => {
  const { root, a, b } = await fixture();
  const dirty = new DirtyPathCoordinator(); dirty.activate(root); dirty.markDirty(payload(root, [a]));
  const calls: Array<{ command: string; args: string[] }> = [];
  const h = harness(dirty, async (options) => {
    calls.push({ command: options.command, args: options.args ?? [] });
    return result(options.command, calls.length + 10, options.args);
  });

  const explicit = await h.tools.get("cx_refresh").execute("1", { paths: [b] }, undefined, undefined, h.ctx(root));
  assert.deepEqual(calls[0], { command: "refresh", args: ["src/b.ts", "src/a.ts"] });
  assert.deepEqual(explicit.details.dirtyRefresh, { requested: 1, refreshed: 1, generation: 11 });
  assert.deepEqual(dirty.pendingPaths(root), []);

  dirty.markDirty(payload(root, [a, b]));
  await h.tools.get("cx_refresh").execute("2", { paths: [] }, undefined, undefined, h.ctx(root));
  assert.deepEqual(calls[1], { command: "refresh", args: [] });
  assert.deepEqual(calls[2], { command: "refresh", args: ["src/a.ts", "src/b.ts"] });
  assert.deepEqual(dirty.pendingPaths(root), []);
  await h.tools.get("cx_overview").execute("3", {}, undefined, undefined, h.ctx(root));
  assert.equal(calls.filter((call) => call.command === "refresh").length, 3);
});

test("full refresh retains pending paths unless the follow-up named proof succeeds", async () => {
  const { root, a } = await fixture();
  const dirty = new DirtyPathCoordinator(); dirty.activate(root); dirty.markDirty(payload(root, [a]));
  const calls: string[][] = [];
  const h = harness(dirty, async (options) => {
    calls.push(options.args ?? []);
    return result(options.command, calls.length, []);
  });

  await assert.rejects(
    () => h.tools.get("cx_refresh").execute("1", { paths: [] }, undefined, undefined, h.ctx(root)),
    /did not confirm requested paths/,
  );
  assert.deepEqual(calls, [[], ["src/a.ts"]]);
  assert.deepEqual(dirty.pendingPaths(root), ["src/a.ts"]);
});

test("without dirty events query behavior is unchanged", async () => {
  const { root } = await fixture();
  const dirty = new DirtyPathCoordinator(); dirty.activate(root);
  const calls: string[] = [];
  const h = harness(dirty, async (options) => { calls.push(options.command); return result(options.command, 1, options.args); });
  const output = await h.tools.get("cx_overview").execute("1", {}, undefined, undefined, h.ctx(root));
  assert.deepEqual(calls, ["overview"]);
  assert.equal(output.details.dirtyRefresh, undefined);
});

test("dirty events and explicit refresh reject dangling symlinks in any missing parent component", async () => {
  const { root } = await fixture();
  const outside = await mkdtemp(join(tmpdir(), "pi-cx-dangling-out-"));
  await symlink(join(outside, "missing-target"), join(root, "dangling"));
  const dirty = new DirtyPathCoordinator(); dirty.activate(root);

  assert.equal(dirty.markDirty(payload(root, [join(root, "dangling", "new.ts")])), 0);
  assert.deepEqual(dirty.pendingPaths(root), []);
  const h = harness(dirty, async (options) => result(options.command, 1, options.args));
  await assert.rejects(
    () => h.tools.get("cx_refresh").execute("1", { paths: ["dangling/new.ts"] }, undefined, undefined, h.ctx(root)),
    /unresolved symlink/,
  );
});

test("the bundled CX binary refuses a dirty ancestor replaced by an outside symlink", async (t) => {
  try { await access(BINARY_PATH); } catch { return t.skip("bundled asset is installed by postinstall/release packaging"); }
  const { root, a } = await fixture();
  const outside = await mkdtemp(join(tmpdir(), "pi-cx-swap-out-"));
  await writeFile(join(outside, "a.ts"), "const outside = 1;\n");
  const dirty = new DirtyPathCoordinator(); dirty.activate(root);
  const tools = new Map<string, any>();
  registerCxTools({ registerTool(tool: any) { tools.set(tool.name, tool); } } as any, dirty);
  const ctx = { cwd: root, hasUI: false, ui: {} };

  await tools.get("cx_symbols").execute("baseline", { name: "a" }, undefined, undefined, ctx);
  assert.equal(dirty.markDirty(payload(root, [a])), 1);
  await rename(join(root, "src"), join(root, "src-old"));
  await symlink(outside, join(root, "src"));

  await assert.rejects(
    () => tools.get("cx_overview").execute("query", {}, undefined, undefined, ctx),
    /dirty-path refresh failed/,
  );
  assert.deepEqual(dirty.pendingPaths(root), ["src/a.ts"]);
  await assert.rejects(
    () => tools.get("cx_refresh").execute("full", { paths: [] }, undefined, undefined, ctx),
    /malformed path status|unexpected path status/,
  );
  assert.deepEqual(dirty.pendingPaths(root), ["src/a.ts"]);
});

test("dirty event validation rejects hostile or oversized payloads before adding paths", async () => {
  const { root, a } = await fixture();
  const dirty = new DirtyPathCoordinator(); dirty.activate(root);

  assert.equal(dirty.markDirty(payload(root, Array(MAX_DIRTY_PATHS + 1).fill(a))), 0);
  assert.equal(dirty.markDirty({ version: 1, source: "x".repeat(MAX_DIRTY_SOURCE_LENGTH + 1), cwd: root, paths: [a] }), 0);
  assert.equal(dirty.markDirty({ version: 1, source: "x", cwd: root, paths: ["x".repeat(MAX_DIRTY_PATH_LENGTH + 1)] }), 0);
  assert.equal(dirty.markDirty(payload(root, [Array(MAX_DIRTY_PATH_COMPONENTS + 1).fill("d").join("/")])), 0);
  assert.doesNotThrow(() => dirty.markDirty(Object.defineProperty({}, "version", { get() { throw new Error("hostile getter"); } })));

  let pathReads = 0;
  const changingPayload = { version: 1, source: "x", cwd: root, get paths() { pathReads += 1; return pathReads === 1 ? [a] : Array(MAX_DIRTY_PATHS + 1).fill(a); } };
  assert.equal(dirty.markDirty(changingPayload), 1);
  assert.equal(pathReads, 1);

  const customIterator = [a];
  customIterator[Symbol.iterator] = function* () { for (;;) yield a; };
  const iteratorDirty = new DirtyPathCoordinator(); iteratorDirty.activate(root);
  assert.equal(iteratorDirty.markDirty(payload(root, customIterator)), 1);
  assert.deepEqual(iteratorDirty.pendingPaths(root), ["src/a.ts"]);
  assert.deepEqual(dirty.pendingPaths(root), ["src/a.ts"]);
});

test("a refresh result must confirm every named path before the query can run", async () => {
  const { root, a } = await fixture();
  const dirty = new DirtyPathCoordinator(); dirty.activate(root); dirty.markDirty(payload(root, [a]));
  const calls: string[] = [];
  const h = harness(dirty, async (options) => {
    calls.push(options.command);
    return options.command === "refresh" ? result("refresh", 2, []) : result(options.command, 2, options.args);
  });

  await assert.rejects(
    () => h.tools.get("cx_overview").execute("1", {}, undefined, undefined, h.ctx(root)),
    /did not confirm requested paths/,
  );
  assert.deepEqual(calls, ["refresh"]);
  assert.deepEqual(dirty.pendingPaths(root), ["src/a.ts"]);
});

test("named refresh rejects non-string status values", async () => {
  const { root, a } = await fixture();
  const dirty = new DirtyPathCoordinator(); dirty.activate(root); dirty.markDirty(payload(root, [a]));
  const h = harness(dirty, async (options) => {
    const output = result(options.command, 2, options.args);
    if (options.command === "refresh") (output.envelope.results[0] as { status: unknown }).status = ["unchanged"];
    return output;
  });
  await assert.rejects(
    () => h.tools.get("cx_overview").execute("1", {}, undefined, undefined, h.ctx(root)),
    /malformed path status/,
  );
  assert.deepEqual(dirty.pendingPaths(root), ["src/a.ts"]);
});

test("successful automatic refresh remains consumed when the following query fails", async () => {
  const { root, a } = await fixture();
  const dirty = new DirtyPathCoordinator(); dirty.activate(root); dirty.markDirty(payload(root, [a]));
  const h = harness(dirty, async (options) => {
    if (options.command === "overview") throw new Error("query failed");
    return result(options.command, 2, options.args);
  });

  await assert.rejects(() => h.tools.get("cx_overview").execute("1", {}, undefined, undefined, h.ctx(root)), /query failed/);
  assert.deepEqual(dirty.pendingPaths(root), []);
});

test("explicit refresh failure restores its pending snapshot and unions events received in flight", async () => {
  const { root, a, b } = await fixture();
  const dirty = new DirtyPathCoordinator(); dirty.activate(root); dirty.markDirty(payload(root, [a]));
  let release!: () => void;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  const h = harness(dirty, async (options) => {
    if (options.command === "refresh") { await gate; throw new Error("explicit refresh failed"); }
    return result(options.command, 1, options.args);
  });

  const refresh = h.tools.get("cx_refresh").execute("1", { paths: [b] }, undefined, undefined, h.ctx(root));
  await waitFor(() => dirty.pendingPaths(root).length === 0);
  dirty.markDirty(payload(root, [b]));
  release();
  await assert.rejects(() => refresh, /explicit refresh failed/);
  assert.deepEqual(dirty.pendingPaths(root).sort(), ["src/a.ts", "src/b.ts"]);
});

test("all seven query tools drain dirty paths before their query command", async () => {
  const { root, a } = await fixture();
  const dirty = new DirtyPathCoordinator(); dirty.activate(root);
  const calls: string[] = [];
  const h = harness(dirty, async (options) => { calls.push(options.command); return result(options.command, calls.length, options.args); });
  const cases: Array<[string, Record<string, unknown>, string]> = [
    ["cx_overview", {}, "overview"],
    ["cx_symbols", { name: "a" }, "symbols"],
    ["cx_definition", { name: "a" }, "definition"],
    ["cx_references", { name: "a" }, "references"],
    ["cx_callers", { name: "a" }, "callers"],
    ["cx_callees", { name: "a" }, "callees"],
    ["cx_map", {}, "map"],
  ];

  for (const [tool, params, command] of cases) {
    dirty.markDirty(payload(root, [a]));
    const start = calls.length;
    await h.tools.get(tool).execute(tool, params, undefined, undefined, h.ctx(root));
    assert.deepEqual(calls.slice(start), ["refresh", command], tool);
  }
});

test("canonical cwd and absolute path aliases are accepted on macOS", async (t) => {
  if (process.platform !== "darwin") return t.skip("macOS path alias test");
  const { root, a } = await fixture();
  if (!root.startsWith("/private/var/")) return t.skip("fixture is not under /private/var");
  const aliasRoot = root.replace(/^\/private\/var\//, "/var/");
  const aliasPath = a.replace(/^\/private\/var\//, "/var/");
  const dirty = new DirtyPathCoordinator(); dirty.activate(root);
  assert.equal(dirty.markDirty(payload(aliasRoot, [aliasPath])), 1);
  assert.deepEqual(dirty.pendingPaths(root), ["src/a.ts"]);
});
