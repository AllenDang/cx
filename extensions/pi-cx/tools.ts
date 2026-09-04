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

const common = {
  noTests: Type.Optional(Type.Boolean({ description: "Exclude test files and test symbols" })),
  fresh: Type.Optional(StringEnum(FRESHNESS_MODES, { description: "Index freshness verification mode (default metadata)" })),
  limit: Type.Optional(Type.Integer({ minimum: 1, maximum: 200 })),
  offset: Type.Optional(Type.Integer({ minimum: 0 })),
};
const kind = Type.Optional(StringEnum(SYMBOL_KINDS));
const role = Type.Optional(StringEnum(SYMBOL_ROLES));

export const schemas = {
  overview: Type.Object({ path: Type.Optional(Type.String({ default: "." })), full: Type.Optional(Type.Boolean()), ...common }),
  symbols: Type.Object({ name: Type.Optional(Type.String()), file: Type.Optional(Type.String()), kind, role, scope: Type.Optional(Type.String()), kinds: Type.Optional(Type.Boolean()), ...common }),
  definition: Type.Object({ name: Type.String(), from: Type.Optional(Type.String()), kind, role, scope: Type.Optional(Type.String()), maxLines: Type.Optional(Type.Integer({ minimum: 1, maximum: 200 })), ...common }),
  references: Type.Object({ name: Type.String(), file: Type.Optional(Type.String()), context: Type.Optional(Type.Boolean()), ...common }),
  callers: Type.Object({ name: Type.String(), scope: Type.Optional(Type.String()), ...common }),
  callees: Type.Object({ name: Type.String(), scope: Type.Optional(Type.String()), ...common }),
  map: Type.Object({ depth: Type.Optional(Type.Integer({ minimum: 1, maximum: 8 })), includeVendor: Type.Optional(Type.Boolean()), includeGenerated: Type.Optional(Type.Boolean()), tests: Type.Optional(Type.Boolean()), exclude: Type.Optional(Type.Array(Type.String(), { maxItems: 32 })), fresh: common.fresh, limit: common.limit, offset: common.offset }),
  refresh: Type.Object({ paths: Type.Optional(Type.Array(Type.String(), { maxItems: 200, default: [] })) }),
};

export type OverviewParams = Static<typeof schemas.overview>;
export type SymbolsParams = Static<typeof schemas.symbols>;

type CommonParams = { noTests?: boolean; fresh?: "metadata" | "verified"; limit?: number; offset?: number };
function commonArgs(params: CommonParams, defaultLimit: number): string[] {
  const args: string[] = [];
  if (params.noTests) args.push("--no-tests");
  args.push("--fresh", params.fresh ?? "metadata", "--limit", String(params.limit ?? defaultLimit));
  if (params.offset !== undefined) args.push("--offset", String(params.offset));
  return args;
}
function option(args: string[], flag: string, value: unknown): void { if (value !== undefined) args.push(flag, String(value)); }

export async function buildOverviewArgs(root: string, p: OverviewParams): Promise<string[]> {
  const args = [await projectPath(root, p.path ?? ".")]; if (p.full) args.push("--full"); return [...args, ...commonArgs(p, 100)];
}
export async function buildSymbolsArgs(root: string, p: SymbolsParams): Promise<string[]> {
  if (!p.kinds && p.name === undefined && p.file === undefined && p.kind === undefined && p.role === undefined && p.scope === undefined) throw new Error("cx_symbols requires at least one filter or kinds=true");
  const args: string[] = []; option(args, "--name", p.name); if (p.file !== undefined) option(args, "--file", await projectPath(root, p.file)); option(args, "--kind", p.kind); option(args, "--role", p.role); option(args, "--scope", p.scope); if (p.kinds) args.push("--kinds"); return [...args, ...commonArgs(p, 100)];
}
export async function buildDefinitionArgs(root: string, p: Static<typeof schemas.definition>): Promise<string[]> {
  const args = ["--name", p.name]; if (p.from !== undefined) option(args, "--from", await projectPath(root, p.from)); option(args, "--kind", p.kind); option(args, "--role", p.role ?? "definition"); option(args, "--scope", p.scope); option(args, "--max-lines", p.maxLines ?? 200); return [...args, ...commonArgs(p, 3)];
}
export async function buildReferencesArgs(root: string, p: Static<typeof schemas.references>): Promise<string[]> {
  const args = ["--name", p.name]; if (p.file !== undefined) option(args, "--file", await projectPath(root, p.file)); if (p.context) args.push("--context"); return [...args, ...commonArgs(p, 50)];
}
export function buildRelationArgs(p: Static<typeof schemas.callers>): string[] { const args = ["--name", p.name]; option(args, "--scope", p.scope); return [...args, ...commonArgs(p, 50)]; }
export function buildMapArgs(p: Static<typeof schemas.map>): string[] {
  const args = ["--depth", String(p.depth ?? 1)]; if (p.includeVendor) args.push("--include-vendor"); if (p.includeGenerated) args.push("--include-generated"); if (p.tests) args.push("--tests"); for (const glob of p.exclude ?? []) { if (glob.includes("\0")) throw new Error("exclude glob contains NUL"); args.push("--exclude", glob); } return [...args, ...commonArgs(p, 40)];
}
export async function buildRefreshArgs(root: string, p: Static<typeof schemas.refresh>): Promise<string[]> { return Promise.all((p.paths ?? []).map((path) => projectPath(root, path, true))); }

const guidance: Record<string, string> = {
  cx_overview: "Use cx_overview before reading a whole source file when only its structure is needed.",
  cx_symbols: "Use cx_symbols for identifier-oriented discovery; use grep for raw strings, logs, SQL, routes, and generated text.",
  cx_definition: "Use cx_definition to read one implementation before falling back to a full-file read.",
  cx_references: "Use cx_references for syntax-classified occurrences; do not treat unresolved edges as resolved.",
  cx_callers: "Use cx_callers/cx_callees for one-hop call evidence; do not treat unresolved edges as resolved.",
  cx_callees: "Use cx_callers/cx_callees for one-hop call evidence; do not treat unresolved edges as resolved.",
  cx_map: "Use cx_map for bounded repository orientation, not as a runtime dependency graph.",
  cx_refresh: "Use cx_refresh after edits when the next decision requires proof that the current index generation includes those paths.",
};

function renderCall(name: string) { return (rawArgs: unknown, theme: any) => { const args = (rawArgs && typeof rawArgs === "object" ? rawArgs : {}) as Record<string, unknown>; return new Text(theme.fg("toolTitle", theme.bold(`${name} `)) + theme.fg("muted", Object.entries(args).slice(0, 2).map(([k, v]) => `${k}=${JSON.stringify(v)}`).join(" ")), 0, 0); }; }
function renderResult(result: any, options: any, theme: any) {
  if (options.isPartial) return new Text(theme.fg("warning", "querying…"), 0, 0);
  const details = result.details ?? {};
  if (options.expanded) return new Text(result.content?.[0]?.text ?? "", 0, 0);
  if (details.error) return new Text(theme.fg("error", String(details.error)), 0, 0);
  return new Text(theme.fg("success", `cx: ${details.resultCount ?? 0} result(s), ${details.durationMs ?? 0}ms, ${details.warningCount ?? 0} warning(s)`), 0, 0);
}

export function registerCxTools(pi: ExtensionAPI): void {
  async function execute(command: string, args: string[], signal: AbortSignal | undefined, ctx: any): Promise<any> {
    const root = await canonicalRoot(ctx.cwd);
    const validated = await validateBundledBinary();
    await ensureBundledGrammars(validated.manifest);
    const invoke = () => runCx({ binary: validated.binary, binaryVersion: validated.version, cwd: root, command, args, signal });
    let result: CxRunResult;
    try { result = await invoke(); } catch (error) {
      if (!(error instanceof CxProcessError) || !error.envelope) throw error;
      const language = missingGrammarLanguage(error.envelope, error.details?.stderr);
      if (!language) throw error;
      if ((BUNDLED_LANGUAGES as readonly string[]).includes(language)) throw new Error(`bundled grammar '${language}' is unavailable after repair; reinstall pi-cx`);
      if (!ctx.hasUI) throw new Error(JSON.stringify({ error: { code: "grammar_not_installed", language, fix: `${validated.binary} lang add ${language}` } }));
      const confirmed = await ctx.ui.confirm("Missing cx grammar", `Install missing cx grammar '${language}' from the network?`);
      if (!confirmed) throw new Error(JSON.stringify({ error: { code: "grammar_not_installed", language, fix: `${validated.binary} lang add ${language}` } }));
      await runCxMaintenance(validated.binary, root, ["lang", "add", language], signal);
      result = await invoke(); result.details.grammarInstalled = language; result.details.retried = true;
    }
    return {
      content: [{ type: "text", text: result.raw }],
      details: { ...result.details, resultCount: result.envelope.results?.length ?? 0, warningCount: result.envelope.warnings?.length ?? 0, freshness: result.envelope.freshness },
    };
  }
  const add = (name: string, label: string, description: string, parameters: any, builder: (root: string, p: any) => string[] | Promise<string[]>, promptSnippet: string) => pi.registerTool({
    name, label, description, parameters, promptSnippet, promptGuidelines: [guidance[name]!], renderCall: renderCall(name), renderResult,
    async execute(_id: string, params: any, signal: AbortSignal | undefined, onUpdate: any, ctx: any) {
      onUpdate?.({ content: [{ type: "text", text: "querying…" }], details: {} });
      const root = await canonicalRoot(ctx.cwd); return execute(name.slice(3), await builder(root, params), signal, ctx);
    },
  });
  add("cx_overview", "cx overview", "Show one directory level or a source file outline. Output is bounded to 50KB/2000 lines.", schemas.overview, buildOverviewArgs, "Inspect a directory or file structure without reading full source");
  add("cx_symbols", "cx symbols", "Search repository symbols by typed filters; requires a filter or kinds=true.", schemas.symbols, buildSymbolsArgs, "Search identifiers and symbol metadata across the project");
  add("cx_definition", "cx definition", "Read a symbol implementation body; defaults to role=definition.", schemas.definition, buildDefinitionArgs, "Read one symbol body instead of a whole file");
  add("cx_references", "cx references", "Find syntax-classified occurrences; this is not compiler type resolution.", schemas.references, buildReferencesArgs, "Find syntax-classified references to a symbol");
  add("cx_callers", "cx callers", "Find one-hop callers, preserving unresolved targets and candidates.", schemas.callers, (_r, p) => buildRelationArgs(p), "Find direct callers with resolution evidence");
  add("cx_callees", "cx callees", "Find one-hop callees; ambiguous symbols are not guessed and no multi-hop depth is available.", schemas.callees, (_r, p) => buildRelationArgs(p), "Find direct callees with resolution evidence");
  add("cx_map", "cx map", "Create a bounded repository map preserving ranking and import warnings.", schemas.map, (_r, p) => buildMapArgs(p), "Orient within repository subsystems and import edges");
  add("cx_refresh", "cx refresh", "Explicitly refresh changed paths, or verify the whole project when paths is empty.", schemas.refresh, buildRefreshArgs, "Refresh the cx index after edits when generation proof is needed");
}
