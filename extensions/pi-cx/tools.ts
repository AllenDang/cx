import { Type, type Static } from "typebox";
import { StringEnum } from "@earendil-works/pi-ai";
import { Text } from "@earendil-works/pi-tui";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { BUNDLED_LANGUAGES, FRESHNESS_MODES, SYMBOL_KINDS, SYMBOL_ROLES, type CxRunResult } from "./types.js";
import { validateBundledBinary } from "./binary.js";
import { ensureBundledGrammars } from "./grammars.js";
import { canonicalRoot, projectPath } from "./paths.js";
import { CxProcessError, runCx, runCxMaintenance } from "./runner.js";
import { missingGrammarLanguage } from "./protocol.js";
import { DirtyPathCoordinator } from "./dirty-paths.js";

const common = {
  noTests: Type.Optional(Type.Boolean({ description: "Exclude test files and test symbols" })),
  fresh: Type.Optional(StringEnum(FRESHNESS_MODES, { description: "Index freshness verification mode (default metadata)" })),
  limit: Type.Optional(Type.Integer({ minimum: 1, maximum: 200 })),
  offset: Type.Optional(Type.Integer({ minimum: 0 })),
};
const detail = Type.Optional(StringEnum(["compact", "full"] as const, { description: "compact (default) preserves primary evidence; full includes repeated raw paths/matches/hunks" }));
const kind = Type.Optional(StringEnum(SYMBOL_KINDS));
const role = Type.Optional(StringEnum(SYMBOL_ROLES));
const scope = Type.Optional(Type.String({ description: "Glob matched against the complete qualified name. Use ANGE::*, ANGE::MaterialRegistry::*, or an exact name such as ANGE::MaterialRegistry::load; a parent without trailing ::* does not match members." }));
const symbolName = Type.Optional(Type.String({ description: "Symbol-name glob. Use *Material* for related-name discovery; Material is an exact match." }));

export const schemas = {
  overview: Type.Object({ path: Type.Optional(Type.String({ default: "." })), full: Type.Optional(Type.Boolean()), ...common }),
  symbols: Type.Object({ name: symbolName, file: Type.Optional(Type.String()), kind, role, scope, kinds: Type.Optional(Type.Boolean()), ...common }),
  definition: Type.Object({ name: Type.String({ description: "Lexical symbol name, not a qualified name" }), from: Type.Optional(Type.String()), kind, role, scope, maxLines: Type.Optional(Type.Integer({ minimum: 1, maximum: 200 })), ...common }),
  references: Type.Object({ name: Type.String({ description: "Lexical identifier such as load; qualified names such as ANGE::MaterialRegistry::load are rejected. Use cx_callers/cx_callees with scope for qualified call evidence." }), file: Type.Optional(Type.String()), context: Type.Optional(Type.Boolean()), ...common }),
  callers: Type.Object({ name: Type.String({ description: "Lexical callee identifier" }), scope, ...common }),
  callees: Type.Object({ name: Type.String({ description: "Lexical symbol identifier whose body should be examined" }), scope, ...common }),
  map: Type.Object({ depth: Type.Optional(Type.Integer({ minimum: 1, maximum: 8 })), includeVendor: Type.Optional(Type.Boolean()), includeGenerated: Type.Optional(Type.Boolean()), tests: Type.Optional(Type.Boolean()), exclude: Type.Optional(Type.Array(Type.String(), { maxItems: 32 })), fresh: common.fresh, limit: common.limit, offset: common.offset }),
  refresh: Type.Object({ paths: Type.Optional(Type.Array(Type.String(), { maxItems: 200, default: [] })) }),
  impact: Type.Object({ name: Type.String({ description: "Lexical function name; use scope/file/site selectors to choose exactly one root" }), scope, file: Type.Optional(Type.String()), line: Type.Optional(Type.Integer({ minimum: 1, maximum: 4294967295 })), byteOffset: Type.Optional(Type.Integer({ minimum: 0 })), maxDepth: Type.Optional(Type.Integer({ minimum: 0, maximum: 32 })), maxNodes: Type.Optional(Type.Integer({ minimum: 1, maximum: 10000 })), maxEdges: Type.Optional(Type.Integer({ minimum: 1, maximum: 1000000 })), snapshot: Type.Optional(Type.String()), byteBudget: Type.Optional(Type.Integer({ minimum: 1024, maximum: 32768 })), detail, ...common }),
  changes: Type.Object({ base: Type.Optional(Type.String()), head: Type.Optional(Type.String()), staged: Type.Optional(Type.Boolean()), mergeBase: Type.Optional(Type.Boolean()), impact: Type.Optional(Type.Boolean()), maxDepth: Type.Optional(Type.Integer({ minimum: 0, maximum: 32 })), snapshot: Type.Optional(Type.String()), byteBudget: Type.Optional(Type.Integer({ minimum: 1024, maximum: 32768 })), detail, noTests: common.noTests, limit: common.limit, offset: common.offset }),
  context: Type.Object({ query: Type.String({ description: "Task words, identifier or path. Lexical retrieval, not semantic search" }), includeBody: Type.Optional(Type.Boolean()), includeVendor: Type.Optional(Type.Boolean()), includeGenerated: Type.Optional(Type.Boolean()), includeFixtures: Type.Optional(Type.Boolean()), snapshot: Type.Optional(Type.String()), byteBudget: Type.Optional(Type.Integer({ minimum: 1024, maximum: 32768 })), detail, ...common }),
};

export type OverviewParams = Static<typeof schemas.overview>;
export type SymbolsParams = Static<typeof schemas.symbols>;

type CommonParams = { noTests?: boolean; fresh?: "metadata" | "verified"; limit?: number; offset?: number };
function assertInteger(name: string, value: unknown, minimum: number, maximum = Number.MAX_SAFE_INTEGER): void {
  if (value !== undefined && (!Number.isInteger(value) || (value as number) < minimum || (value as number) > maximum)) throw new Error(`${name} must be an integer in ${minimum}..${maximum}`);
}
function assertBoolean(name: string, value: unknown): void { if (value !== undefined && typeof value !== "boolean") throw new Error(`${name} must be a boolean`); }
function assertString(name: string, value: unknown, required = false): void {
  if ((required && (typeof value !== "string" || value.length === 0)) || (value !== undefined && typeof value !== "string")) throw new Error(`${name} must be ${required ? "a non-empty" : "a"} string`);
  if (typeof value === "string" && value.includes("\0")) throw new Error(`${name} contains NUL`);
}
function assertEnum(name: string, value: unknown, allowed: readonly string[]): void { if (value !== undefined && (typeof value !== "string" || !allowed.includes(value))) throw new Error(`${name} must be one of: ${allowed.join(", ")}`); }
function validateCommon(params: CommonParams): void {
  assertBoolean("noTests", params.noTests); assertInteger("limit", params.limit, 1, 200); assertInteger("offset", params.offset, 0);
  if (params.fresh !== undefined && !(FRESHNESS_MODES as readonly string[]).includes(params.fresh)) throw new Error("fresh must be metadata or verified");
}
function commonArgs(params: CommonParams, defaultLimit: number): string[] {
  validateCommon(params);
  const args: string[] = [];
  if (params.noTests) args.push("--no-tests");
  args.push("--fresh", params.fresh ?? "metadata", "--limit", String(params.limit ?? defaultLimit));
  if (params.offset !== undefined) args.push("--offset", String(params.offset));
  return args;
}
function option(args: string[], flag: string, value: unknown): void { if (value !== undefined) args.push(flag, String(value)); }

export async function buildOverviewArgs(root: string, p: OverviewParams): Promise<string[]> {
  assertString("path", p.path); assertBoolean("full", p.full);
  const args = [await projectPath(root, p.path ?? ".")]; if (p.full) args.push("--full"); return [...args, ...commonArgs(p, 100)];
}
export async function buildSymbolsArgs(root: string, p: SymbolsParams): Promise<string[]> {
  assertString("name", p.name); assertString("file", p.file); assertString("scope", p.scope); assertBoolean("kinds", p.kinds); assertEnum("kind", p.kind, SYMBOL_KINDS); assertEnum("role", p.role, SYMBOL_ROLES);
  if (!p.kinds && p.name === undefined && p.file === undefined && p.kind === undefined && p.role === undefined && p.scope === undefined) throw new Error("cx_symbols requires at least one filter or kinds=true");
  const args: string[] = []; option(args, "--name", p.name); if (p.file !== undefined) option(args, "--file", await projectPath(root, p.file)); option(args, "--kind", p.kind); option(args, "--role", p.role); option(args, "--scope", p.scope); if (p.kinds) args.push("--kinds"); return [...args, ...commonArgs(p, 100)];
}
export async function buildDefinitionArgs(root: string, p: Static<typeof schemas.definition>): Promise<string[]> {
  assertString("name", p.name, true); assertString("from", p.from); assertString("scope", p.scope); assertInteger("maxLines", p.maxLines, 1, 200); assertEnum("kind", p.kind, SYMBOL_KINDS); assertEnum("role", p.role, SYMBOL_ROLES);
  const args = ["--name", p.name]; if (p.from !== undefined) option(args, "--from", await projectPath(root, p.from)); option(args, "--kind", p.kind); option(args, "--role", p.role ?? "definition"); option(args, "--scope", p.scope); option(args, "--max-lines", p.maxLines ?? 200); return [...args, ...commonArgs(p, 3)];
}
export async function buildReferencesArgs(root: string, p: Static<typeof schemas.references>): Promise<string[]> {
  assertString("name", p.name, true); assertString("file", p.file); assertBoolean("context", p.context);
  if (p.name.includes("::") || p.name.includes(".")) throw new Error("cx_references.name accepts a lexical identifier such as 'load', not a qualified name. Use cx_callers/cx_callees with scope='ANGE::MaterialRegistry::load' for qualified call evidence.");
  const args = ["--name", p.name]; if (p.file !== undefined) option(args, "--file", await projectPath(root, p.file)); if (p.context) args.push("--context"); return [...args, ...commonArgs(p, 50)];
}
export function buildRelationArgs(p: Static<typeof schemas.callers>): string[] { assertString("name", p.name, true); assertString("scope", p.scope); const args = ["--name", p.name]; option(args, "--scope", p.scope); return [...args, ...commonArgs(p, 50)]; }
export function buildMapArgs(p: Static<typeof schemas.map>): string[] {
  assertInteger("depth", p.depth, 1, 8); assertBoolean("includeVendor", p.includeVendor); assertBoolean("includeGenerated", p.includeGenerated); assertBoolean("tests", p.tests);
  if (p.exclude !== undefined && (!Array.isArray(p.exclude) || p.exclude.length > 32)) throw new Error("exclude must contain at most 32 globs");
  const args = ["--depth", String(p.depth ?? 1)]; if (p.includeVendor) args.push("--include-vendor"); if (p.includeGenerated) args.push("--include-generated"); if (p.tests) args.push("--tests"); for (const glob of p.exclude ?? []) { assertString("exclude glob", glob); args.push("--exclude", glob); } return [...args, ...commonArgs(p, 40)];
}
export async function buildRefreshArgs(root: string, p: Static<typeof schemas.refresh>): Promise<string[]> {
  if (p.paths !== undefined && (!Array.isArray(p.paths) || p.paths.length > 200)) throw new Error("paths must contain at most 200 entries");
  for (const path of p.paths ?? []) assertString("refresh path", path, true);
  return Promise.all((p.paths ?? []).map((path) => projectPath(root, path, true)));
}

function taskBudget(p: { snapshot?: string; byteBudget?: number }, defaultBudget: number): string[] {
  assertString("snapshot", p.snapshot); assertInteger("byteBudget", p.byteBudget, 1024, 32768);
  const args = ["--byte-budget", String(p.byteBudget ?? defaultBudget)]; option(args, "--snapshot", p.snapshot); return args;
}
export async function buildImpactArgs(root: string, p: Static<typeof schemas.impact>): Promise<string[]> {
  assertString("name", p.name, true); assertString("scope", p.scope); assertString("file", p.file);
  assertInteger("line", p.line, 1, 4294967295); assertInteger("byteOffset", p.byteOffset, 0);
  assertInteger("maxDepth", p.maxDepth, 0, 32); assertInteger("maxNodes", p.maxNodes, 1, 10000); assertInteger("maxEdges", p.maxEdges, 1, 1000000); assertEnum("detail", p.detail, ["compact", "full"]);
  if ((p.line !== undefined || p.byteOffset !== undefined) && p.file === undefined) throw new Error("line/byteOffset requires file");
  if (p.line !== undefined && p.byteOffset !== undefined) throw new Error("line and byteOffset are mutually exclusive");
  const args = ["--name", p.name]; if (p.file !== undefined) option(args, "--file", await projectPath(root, p.file));
  option(args, "--line", p.line); option(args, "--byte-offset", p.byteOffset); option(args, "--scope", p.scope);
  option(args, "--max-depth", p.maxDepth ?? 3); option(args, "--max-nodes", p.maxNodes ?? 1000); option(args, "--max-edges", p.maxEdges ?? 20000); option(args, "--detail", p.detail ?? "compact");
  return [...args, ...taskBudget(p, 32768), ...commonArgs({ ...p, fresh: p.fresh ?? "verified" }, 50)];
}
export function buildChangesArgs(p: Static<typeof schemas.changes>): string[] {
  assertString("base", p.base); assertString("head", p.head); assertBoolean("staged", p.staged); assertBoolean("mergeBase", p.mergeBase); assertBoolean("impact", p.impact); assertEnum("detail", p.detail, ["compact", "full"]);
  assertInteger("maxDepth", p.maxDepth, 0, 32); validateCommon(p);
  if (p.staged && p.head !== undefined) throw new Error("staged and head cannot be combined");
  if (p.mergeBase && p.head === undefined) throw new Error("mergeBase requires head");
  const args = ["--base", p.base ?? "HEAD"]; option(args, "--head", p.head); if (p.staged) args.push("--staged"); if (p.mergeBase) args.push("--merge-base"); if (p.impact) args.push("--impact");
  option(args, "--max-depth", p.maxDepth ?? 2); option(args, "--detail", p.detail ?? "compact"); if (p.noTests) args.push("--no-tests"); option(args, "--limit", p.limit ?? 50); option(args, "--offset", p.offset);
  return [...args, ...taskBudget(p, 32768)];
}
export function buildContextArgs(p: Static<typeof schemas.context>): string[] {
  assertString("query", p.query, true); if (!p.query.trim()) throw new Error("query must be a non-empty task description");
  for (const key of ["includeBody", "includeVendor", "includeGenerated", "includeFixtures"] as const) assertBoolean(key, p[key]); assertEnum("detail", p.detail, ["compact", "full"]);
  const args = ["--query", p.query, "--detail", p.detail ?? "compact"]; if (p.includeBody) args.push("--include-body"); if (p.includeVendor) args.push("--include-vendor"); if (p.includeGenerated) args.push("--include-generated"); if (p.includeFixtures) args.push("--include-fixtures");
  return [...args, ...taskBudget(p, 16384), ...commonArgs({ ...p, fresh: p.fresh ?? "verified" }, 10)];
}

const guidance: Record<string, string> = {
  cx_overview: "For source navigation, prefer cx_overview to a directory listing when you need code structure, and cx_definition when you need an implementation. Use read for complete text/config files.",
  cx_symbols: "Source-edit navigation policy: when the task names a code symbol, make the first source lookup with cx_symbols, then read the needed implementation with cx_definition. For unknown symbols use cx_context. This rule is for locating code, not for shell setup, tests, literal-text/config/prose edits, or files already inspected. If indexed lookup returns no useful match or cannot cover the needed text, fall back to grep/find/read immediately. No repeated calls or minimum call count are required.",
  cx_definition: "Use cx_definition for a known function/class before reading a whole source file. Pass the lexical name, plus from or scope if ambiguous. If the body is missing or truncated, use read for the remaining source. Keep read/grep for tasks that need raw text rather than code structure.",
  cx_references: "Use cx_references for syntax-classified occurrences and pass a lexical identifier such as load, never a qualified name; use cx_callers/cx_callees with scope for qualified call evidence. Do not treat unresolved edges as resolved.",
  cx_callers: "Use cx_callers/cx_callees for one-hop call evidence; do not treat unresolved edges as resolved.",
  cx_callees: "Use cx_callers/cx_callees for one-hop call evidence; do not treat unresolved edges as resolved.",
  cx_map: "Use cx_map for bounded repository orientation, not as a runtime dependency graph.",
  cx_refresh: "Use cx_refresh after edits when the next decision requires proof that the current index generation includes those paths.",
  cx_impact: "Use cx_impact for multi-hop reverse call questions, not routine body reads. Select one root with file/scope/site; preserve supported versus possible paths and unresolved frontiers. Unknown totals and traversal limits are not a safety verdict; snapshot guards output pagination.",
  cx_changes: "Use cx_changes for tracked Git changes with old/new symbol evidence. Default compares raw working bytes to HEAD; staged and commit modes are explicit. Optional impact has separate before/after snapshots. It never executes project tests or proves behavioral safety.",
  cx_context: "For source edits whose location is unknown, begin source discovery with cx_context using likely code identifiers or English keywords; if the task already names the symbol use cx_symbols instead. Follow useful matches with cx_definition. Use grep/find/read when no useful match is available. Skip this workflow for pure documentation/configuration tasks; do not call tools merely to satisfy a quota.",
};

function renderCall(name: string) { return (rawArgs: unknown, theme: any) => { const args = (rawArgs && typeof rawArgs === "object" ? rawArgs : {}) as Record<string, unknown>; return new Text(theme.fg("toolTitle", theme.bold(`${name} `)) + theme.fg("muted", Object.entries(args).slice(0, 2).map(([k, v]) => `${k}=${JSON.stringify(v)}`).join(" ")), 0, 0); }; }
export function renderCxResult(result: any, options: any, theme: any) {
  if (options.isPartial) return new Text(theme.fg("warning", "querying…"), 0, 0);
  const raw = result.content?.[0]?.text ?? "";
  if (options.isError) return new Text(theme.fg("error", options.expanded ? raw : `cx error: ${raw}`), 0, 0);
  const details = result.details ?? {};
  if (options.expanded) return new Text(raw, 0, 0);
  const partial = details.analysis?.complete === false;
  return new Text(theme.fg(partial ? "warning" : "success", `cx: ${details.resultCount ?? 0} result(s), ${details.durationMs ?? 0}ms, ${details.warningCount ?? 0} warning(s)${partial ? ", partial analysis" : ""}`), 0, 0);
}

export interface CxToolRuntime {
  validateBinary: typeof validateBundledBinary;
  ensureGrammars: typeof ensureBundledGrammars;
  run: typeof runCx;
  runMaintenance: typeof runCxMaintenance;
}

const defaultRuntime: CxToolRuntime = {
  validateBinary: validateBundledBinary,
  ensureGrammars: ensureBundledGrammars,
  run: runCx,
  runMaintenance: runCxMaintenance,
};

export function registerCxTools(pi: ExtensionAPI, dirty = new DirtyPathCoordinator(), runtime: CxToolRuntime = defaultRuntime): void {
  async function execute(root: string, command: string, args: string[], signal: AbortSignal | undefined, ctx: any): Promise<any> {
    return dirty.withRootLock(root, async () => {
      const validated = await runtime.validateBinary();
      await runtime.ensureGrammars(validated.manifest);

      const invoke = async (cxCommand: string, cxArgs: string[]): Promise<CxRunResult> => {
        const run = () => runtime.run({ binary: validated.binary, binaryVersion: validated.version, cwd: root, command: cxCommand, args: cxArgs, signal });
        let result: CxRunResult;
        try { result = await run(); } catch (error) {
          if (!(error instanceof CxProcessError) || !error.envelope) throw error;
          const language = missingGrammarLanguage(error.envelope, error.details?.stderr);
          if (!language) throw error;
          if ((BUNDLED_LANGUAGES as readonly string[]).includes(language)) throw new Error(`bundled grammar '${language}' is unavailable after repair; reinstall pi-cx`);
          if (!ctx.hasUI) throw new Error(JSON.stringify({ error: { code: "grammar_not_installed", language, fix: `${validated.binary} lang add ${language}` } }));
          const confirmed = await ctx.ui.confirm("Missing cx grammar", `Install missing cx grammar '${language}' from the network?`);
          if (!confirmed) throw new Error(JSON.stringify({ error: { code: "grammar_not_installed", language, fix: `${validated.binary} lang add ${language}` } }));
          await runtime.runMaintenance(validated.binary, root, ["lang", "add", language], signal);
          result = await run(); result.details.grammarInstalled = language; result.details.retried = true;
        }
        return result;
      };

      const pending = dirty.takePending(root);
      let dirtyRefresh: ReturnType<typeof refreshDetails> | undefined;
      let result: CxRunResult;
      if (command === "refresh") {
        const refreshArgs = args.length === 0 ? [] : [...new Set([...args, ...pending])];
        try {
          result = await invoke("refresh", refreshArgs);
          if (refreshArgs.length > 0) {
            assertNamedRefresh(refreshArgs, result);
            if (pending.length > 0) dirtyRefresh = refreshDetails(pending, result);
          } else if (pending.length > 0) {
            // A full-project refresh reports only changed files, so it cannot
            // prove that every dirty snapshot path was actually readable.
            // Follow it with a named proof before consuming pending paths.
            const proof = await invoke("refresh", pending);
            assertNamedRefresh(pending, proof);
            dirtyRefresh = refreshDetails(pending, proof);
          }
        } catch (error) {
          dirty.restorePending(root, pending);
          throw error;
        }
      } else {
        if (pending.length > 0) {
          try {
            const refresh = await invoke("refresh", pending);
            assertNamedRefresh(pending, refresh);
            dirtyRefresh = refreshDetails(pending, refresh);
          } catch (error) {
            dirty.restorePending(root, pending);
            const message = error instanceof Error ? error.message : String(error);
            throw new Error(`dirty-path refresh failed; CX query was not executed: ${message}`, { cause: error });
          }
        }
        result = await invoke(command, args);
      }

      return {
        content: [{ type: "text", text: result.raw }],
        details: {
          ...result.details,
          resultCount: result.envelope.results?.length ?? 0,
          warningCount: result.envelope.warnings?.length ?? 0,
          freshness: result.envelope.freshness,
          analysis: result.envelope.analysis,
          ...(dirtyRefresh ? { dirtyRefresh } : {}),
        },
      };
    });
  }
  const add = (name: string, label: string, description: string, parameters: any, builder: (root: string, p: any) => string[] | Promise<string[]>, promptSnippet: string) => pi.registerTool({
    name, label, description, parameters, promptSnippet, promptGuidelines: [guidance[name]!], renderCall: renderCall(name), renderResult: renderCxResult,
    async execute(_id: string, params: any, signal: AbortSignal | undefined, onUpdate: any, ctx: any) {
      if (signal?.aborted) throw new Error("cx query cancelled");
      onUpdate?.({ content: [{ type: "text", text: "querying…" }], details: {} });
      const root = await canonicalRoot(ctx.cwd);
      dirty.activate(root);
      return execute(root, name.slice(3), await builder(root, params), signal, ctx);
    },
  });
  add("cx_overview", "cx overview", "List a directory or outline the functions/classes in a source file, with source locations. Use path (default '.'); output is bounded to 50KB/2000 lines. Read a known implementation with cx_definition instead of scanning the whole file.", schemas.overview, buildOverviewArgs, "List source structure before choosing what to read");
  add("cx_symbols", "cx symbols", "Find the function, class or method you need to edit. Search code definitions by name, without knowing a filename. Example: name='*Cache*'. Returns names, paths, scopes and locations. Exact names are exact matches; use * for discovery. Requires a filter or kinds=true. Scope matches the complete qualified name; copy a returned scope to disambiguate.", schemas.symbols, buildSymbolsArgs, "Source-edit entry point: locate the named function/class");
  add("cx_definition", "cx definition", "Read actual source code for a function, class or method by name. No filename is required; use from or a returned scope to disambiguate. Returns bounded original source, not a generated summary. Defaults to role=definition. Scope matches complete qualified names; Parent::* selects members.", schemas.definition, buildDefinitionArgs, "Read the implementation to edit, directly by symbol name");
  add("cx_references", "cx references", "Find syntax-classified occurrences by lexical identifier. Qualified names are rejected; use cx_callers/cx_callees with scope for qualified call evidence. This is not compiler type resolution.", schemas.references, buildReferencesArgs, "Find syntax-classified references to a symbol");
  add("cx_callers", "cx callers", "Find one-hop callers, preserving unresolved targets and candidates.", schemas.callers, (_r, p) => buildRelationArgs(p), "Find direct callers with resolution evidence");
  add("cx_callees", "cx callees", "Find one-hop callees; ambiguous symbols are not guessed and no multi-hop depth is available.", schemas.callees, (_r, p) => buildRelationArgs(p), "Find direct callees with resolution evidence");
  add("cx_map", "cx map", "Create a bounded repository map preserving ranking and import warnings.", schemas.map, (_r, p) => buildMapArgs(p), "Orient within repository subsystems and import edges");
  add("cx_refresh", "cx refresh", "Explicitly refresh changed paths, or verify the whole project when paths is empty.", schemas.refresh, buildRefreshArgs, "Refresh the cx index after edits when generation proof is needed");
  add("cx_impact", "cx impact", "Analyze bounded multi-hop reverse call impact for one exact function site. Returns shortest supported/possible witnesses, unresolved frontiers, coverage and independent traversal/output limits; not compiler or runtime proof.", schemas.impact, buildImpactArgs, "Trace multi-hop impact with source witnesses and uncertainty");
  add("cx_changes", "cx changes", "Compare tracked Git/working snapshots, preserving old/new symbols and file-level/non-source changes. Optional impact is evaluated separately before and after. Does not run project commands or tests.", schemas.changes, (_r, p) => buildChangesArgs(p), "Locate changes on both sides and optionally analyze their impact");
  add("cx_context", "cx context", "Find where to implement a change from short task keywords when the file and symbol are unknown. Returns ranked source locations and bounded original excerpts; includeBody=true adds bodies. Use likely code identifiers or English terms. Lexical matching, not semantic search; results and pagination are byte-bounded.", schemas.context, (_r, p) => buildContextArgs(p), "Source-edit entry point when the symbol or file is unknown");
}

function assertNamedRefresh(paths: string[], result: CxRunResult): void {
  const expected = new Set(paths.map((path) => path.replaceAll("\\", "/")));
  const confirmed = new Set<string>();
  for (const row of result.envelope.results) {
    if (!row || typeof row !== "object") throw new Error("cx refresh returned a malformed result row");
    const { file, status } = row as { file?: unknown; status?: unknown };
    // The native scanner securely checked this path but cannot index its type.
    // Consume it without claiming source-index coverage; other failures stay fatal.
    if (typeof file !== "string" || typeof status !== "string" || !["updated", "removed", "unchanged", "not_indexed", "unsupported_file_type"].includes(status)) {
      throw new Error("cx refresh returned a malformed path status");
    }
    if (!expected.has(file) || confirmed.has(file)) throw new Error(`cx refresh returned an unexpected path status: ${file}`);
    confirmed.add(file);
  }
  if (confirmed.size !== expected.size) {
    const missing = [...expected].filter((path) => !confirmed.has(path));
    throw new Error(`cx refresh did not confirm requested paths: ${missing.join(", ")}`);
  }
}

function refreshDetails(pending: string[], result: CxRunResult): { requested: number; refreshed: number; generation?: number; unsupportedPaths?: string[] } {
  const generation = result.envelope.freshness?.generation;
  const pendingPaths = new Set(pending.map((path) => path.replaceAll("\\", "/")));
  // Explicit refreshes may also contain non-pending paths. Count only our queue.
  const unsupportedPaths = (result.envelope.results as Array<{ file: string; status: string }>)
    .filter((row) => row.status === "unsupported_file_type" && pendingPaths.has(row.file))
    .map((row) => row.file);
  return {
    requested: pending.length,
    refreshed: pending.length - unsupportedPaths.length,
    ...(typeof generation === "number" ? { generation } : {}),
    ...(unsupportedPaths.length > 0 ? { unsupportedPaths } : {}),
  };
}
