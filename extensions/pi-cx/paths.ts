import { lstat, realpath } from "node:fs/promises";
import { basename, dirname, isAbsolute, normalize, relative, resolve, sep } from "node:path";

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
  if (!isAbsolute(cleaned) && !isWithin(root, absolute)) throw new Error(`path escapes project root: ${input}`);
  let canonical: string;
  try {
    canonical = await realpath(absolute);
  } catch (error) {
    if (!allowMissing) throw new Error(`path does not exist: ${input}`, { cause: error });
    let ancestor = absolute;
    const suffix: string[] = [];
    for (;;) {
      try {
        const canonicalAncestor = await realpath(ancestor);
        const missingCanonical = resolve(canonicalAncestor, ...suffix);
        if (!isWithin(root, missingCanonical)) throw new Error(`path parent escapes project root: ${input}`);
        return relative(root, missingCanonical) || ".";
      } catch (ancestorError) {
        const code = (ancestorError as NodeJS.ErrnoException).code;
        if (code !== "ENOENT" && code !== "ENOTDIR") throw ancestorError;
      }
      try {
        if ((await lstat(ancestor)).isSymbolicLink()) throw new Error(`unresolved symlink is not allowed: ${input}`);
      } catch (lstatError) {
        if ((lstatError as NodeJS.ErrnoException).code !== "ENOENT") throw lstatError;
      }
      const parent = dirname(ancestor);
      if (parent === ancestor) throw new Error(`cannot resolve parent of path: ${input}`);
      suffix.unshift(basename(ancestor));
      ancestor = parent;
    }
  }
  if (!isWithin(root, canonical)) throw new Error(`symlink escapes project root: ${input}`);
  await lstat(canonical);
  return relative(root, canonical) || ".";
}
