#!/usr/bin/env node
// Flatten only native grammar libraries into an explicit loader directory.
// This avoids depending on process-global language-pack initialization order or
// whether a pack version uses a flat or versioned download-cache layout.
import { copyFile, mkdir, readdir, readFile } from "node:fs/promises";
import { resolve, join } from "node:path";

const [sourceArg, destinationArg] = process.argv.slice(2);
if (!sourceArg || !destinationArg) throw new Error("usage: prepare-ci-grammars.mjs SOURCE DESTINATION");
const source = resolve(sourceArg), destination = resolve(destinationArg);
const files = new Map();
async function collect(directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) await collect(path);
    else if (entry.isFile() && /^(?:lib)?tree_sitter_.+\.(so|dylib|dll)$/.test(entry.name)) {
      const previous = files.get(entry.name);
      if (previous && !(await readFile(previous)).equals(await readFile(path))) throw new Error(`conflicting grammar copies: ${entry.name}`);
      files.set(entry.name, path);
    }
  }
}
await collect(source);
if (files.size === 0) throw new Error(`no native grammar libraries found under ${source}`);
await mkdir(destination, { recursive: true });
for (const [name, path] of files) await copyFile(path, join(destination, name));
console.log(`Prepared ${files.size} native grammar libraries in ${destination}`);
