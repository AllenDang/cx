import { lstat, realpath } from "node:fs/promises";
import { dirname, isAbsolute, normalize, relative, resolve, sep } from "node:path";

export function stripLeadingAt(value: string): string {
  return value.startsWith("@") && !value.startsWith("@@") ? value.slice(1) : value;
}

export function isWithin(root: string, candidate: string): boolean {
  const rel = relative(root, candidate);
  return rel === "" || (!rel.startsWith(`..${sep}`) && rel !== ".." && !isAbsolute(rel));
}

export async function canonicalRoot(cwd: string): Promise<string> {
  if (cwd.includes("\0")) throw new Error("project root contains NUL");
  return realpath(cwd);
}

export async function projectPath(root: string, input: string, allowMissing = false): Promise<string> {
  const cleaned = stripLeadingAt(input || ".");
  if (cleaned.includes("\0")) throw new Error("path contains NUL");
  const absolute = normalize(resolve(root, cleaned));
  if (!isWithin(root, absolute)) throw new Error(`path escapes project root: ${input}`);
  let canonical: string;
  try {
    canonical = await realpath(absolute);
  } catch (error) {
    if (!allowMissing) throw new Error(`path does not exist: ${input}`, { cause: error });
    let parent = dirname(absolute);
    for (;;) {
      try { canonical = await realpath(parent); break; } catch {
        const next = dirname(parent);
        if (next === parent) throw new Error(`cannot resolve parent of path: ${input}`);
        parent = next;
      }
    }
    if (!isWithin(root, canonical!)) throw new Error(`path parent escapes project root: ${input}`);
    return relative(root, absolute) || ".";
  }
  if (!isWithin(root, canonical)) throw new Error(`symlink escapes project root: ${input}`);
  await lstat(canonical);
  return relative(root, canonical) || ".";
}
