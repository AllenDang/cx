import { lstatSync, realpathSync } from "node:fs";
import { basename, dirname, isAbsolute, normalize, relative, resolve } from "node:path";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { isWithin } from "./paths.js";

export const CX_MARK_DIRTY_EVENT = "cx:mark-dirty:v1";
export const MAX_DIRTY_PATHS = 200;
export const MAX_DIRTY_SOURCE_LENGTH = 256;
export const MAX_DIRTY_PATH_LENGTH = 32_768;
export const MAX_DIRTY_PATH_COMPONENTS = 128;
export const MAX_DIRTY_TOTAL_COMPONENTS = 4_096;

export interface CxMarkDirtyV1 {
  version: 1;
  source: string;
  cwd: string;
  paths: string[];
}

interface RootState {
  pending: Set<string>;
  tail: Promise<void>;
}

function canonicalRootSync(cwd: string): string {
  if (!cwd || cwd.includes("\0")) throw new Error("project root is invalid");
  return realpathSync.native(cwd);
}

function canonicalProjectPathSync(root: string, input: string): string {
  if (!input || input.includes("\0")) throw new Error("dirty path is invalid");
  const absolute = normalize(isAbsolute(input) ? input : resolve(root, input));

  try {
    const canonical = realpathSync.native(absolute);
    if (!isWithin(root, canonical)) throw new Error("dirty path symlink escapes project root");
    return relative(root, canonical) || ".";
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code;
    if (code !== "ENOENT" && code !== "ENOTDIR") throw error;

    // Walk toward the root. Every unresolved component is checked with lstat:
    // a dangling symlink at any depth cannot be proven to remain in the root.
    let ancestor = absolute;
    const suffix: string[] = [];
    for (;;) {
      try {
        const canonicalAncestor = realpathSync.native(ancestor);
        const canonical = resolve(canonicalAncestor, ...suffix);
        if (!isWithin(root, canonical)) throw new Error("dirty path parent symlink escapes project root");
        return relative(root, canonical) || ".";
      } catch (ancestorError) {
        const ancestorCode = (ancestorError as NodeJS.ErrnoException).code;
        if (ancestorCode !== "ENOENT" && ancestorCode !== "ENOTDIR") throw ancestorError;
      }
      try {
        if (lstatSync(ancestor).isSymbolicLink()) throw new Error("dirty path contains an unresolved symlink");
      } catch (lstatError) {
        if ((lstatError as NodeJS.ErrnoException).code !== "ENOENT") throw lstatError;
      }
      const parent = dirname(ancestor);
      if (parent === ancestor) throw new Error("cannot resolve dirty path parent");
      suffix.unshift(basename(ancestor));
      ancestor = parent;
    }
  }
}

function parsePayload(value: unknown): CxMarkDirtyV1 | undefined {
  if (!value || typeof value !== "object") return undefined;
  const record = value as Record<string, unknown>;
  const version = record.version;
  const source = record.source;
  const cwd = record.cwd;
  const rawPaths = record.paths;
  if (version !== 1
    || typeof source !== "string" || source.length === 0 || source.length > MAX_DIRTY_SOURCE_LENGTH || source.includes("\0")
    || typeof cwd !== "string" || cwd.length === 0 || cwd.length > MAX_DIRTY_PATH_LENGTH || cwd.includes("\0")
    || !Array.isArray(rawPaths)) return undefined;
  const length = rawPaths.length;
  if (!Number.isSafeInteger(length) || length < 0 || length > MAX_DIRTY_PATHS) return undefined;
  const paths = new Array<string>(length);
  let totalComponents = 0;
  for (let index = 0; index < length; index++) {
    const path = rawPaths[index];
    if (typeof path !== "string" || path.length === 0 || path.length > MAX_DIRTY_PATH_LENGTH || path.includes("\0")) return undefined;
    const components = path.split(/[\\/]+/).length;
    if (components > MAX_DIRTY_PATH_COMPONENTS) return undefined;
    totalComponents += components;
    if (totalComponents > MAX_DIRTY_TOTAL_COMPONENTS) return undefined;
    paths[index] = path;
  }
  return { version: 1, source, cwd, paths };
}

export class DirtyPathCoordinator {
  private activeRoot: string | undefined;
  private readonly roots = new Map<string, RootState>();

  activate(cwd: string): string {
    const root = canonicalRootSync(cwd);
    this.activeRoot = root;
    this.state(root);
    return root;
  }

  markDirty(value: unknown): number {
    let payload: CxMarkDirtyV1;
    let root: string;
    try {
      const parsed = parsePayload(value);
      if (!parsed || !this.activeRoot) return 0;
      payload = parsed;
      root = canonicalRootSync(payload.cwd);
    } catch { return 0; }
    if (root !== this.activeRoot) return 0;

    const pending = this.state(root).pending;
    const before = pending.size;
    for (let index = 0; index < payload.paths.length; index++) {
      try { pending.add(canonicalProjectPathSync(root, payload.paths[index]!)); } catch { /* Reject unsafe paths without disrupting the emitter. */ }
    }
    return pending.size - before;
  }

  async withRootLock<T>(root: string, operation: () => Promise<T>): Promise<T> {
    const state = this.state(root);
    const result = state.tail.then(operation);
    state.tail = result.then(() => undefined, () => undefined);
    return result;
  }

  takePending(root: string): string[] {
    const state = this.state(root);
    const paths = [...state.pending];
    state.pending = new Set();
    return paths;
  }

  restorePending(root: string, paths: Iterable<string>): void {
    const pending = this.state(root).pending;
    for (const path of paths) pending.add(path);
  }

  pendingPaths(root: string): string[] {
    return [...this.state(root).pending];
  }

  clear(): void {
    this.activeRoot = undefined;
    this.roots.clear();
  }

  private state(root: string): RootState {
    let state = this.roots.get(root);
    if (!state) {
      state = { pending: new Set(), tail: Promise.resolve() };
      this.roots.set(root, state);
    }
    return state;
  }
}

export function registerDirtyPathProtocol(pi: ExtensionAPI): DirtyPathCoordinator {
  const coordinator = new DirtyPathCoordinator();
  pi.events.on(CX_MARK_DIRTY_EVENT, (payload) => { coordinator.markDirty(payload); });
  pi.on("session_start", (_event, ctx) => { coordinator.clear(); coordinator.activate(ctx.cwd); });
  pi.on("session_shutdown", () => { coordinator.clear(); });
  return coordinator;
}
