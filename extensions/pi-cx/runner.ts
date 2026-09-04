import { createWriteStream } from "node:fs";
import { mkdir, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn } from "node:child_process";
import { parseEnvelope, ProtocolError } from "./protocol.js";
import type { CxEnvelope, CxRunResult, RunDetails } from "./types.js";

export const MAX_STDOUT_BYTES = 50 * 1024;
export const MAX_STDOUT_LINES = 2000;
const MAX_STDERR_BYTES = 8 * 1024;

export class CxProcessError extends Error {
  constructor(message: string, public readonly envelope?: CxEnvelope, public readonly details?: RunDetails) {
    super(message); this.name = "CxProcessError";
  }
}

export interface RunCxOptions {
  binary: string;
  cwd: string;
  command: string;
  args?: string[];
  signal?: AbortSignal;
  timeoutMs?: number;
  binaryVersion?: string;
}

export async function runCxMaintenance(binary: string, cwd: string, args: string[], signal?: AbortSignal, timeoutMs = 120_000): Promise<void> {
  const child = spawn(binary, args, { cwd, env: limitedEnvironment(), shell: false, detached: process.platform !== "win32", stdio: ["ignore", "pipe", "pipe"] });
  let stderr = "";
  child.stderr.on("data", (chunk: Buffer) => { stderr = (stderr + chunk.toString("utf8")).slice(-MAX_STDERR_BYTES); });
  const terminate = () => terminateProcessTree(child);
  const onAbort = () => terminate();
  signal?.addEventListener("abort", onAbort, { once: true });
  const timer = setTimeout(terminate, timeoutMs); timer.unref();
  const code = await new Promise<number | null>((resolve, reject) => { child.once("error", reject); child.once("close", resolve); })
    .finally(() => { clearTimeout(timer); signal?.removeEventListener("abort", onAbort); });
  if (signal?.aborted) throw new CxProcessError("cx grammar installation cancelled");
  if (code !== 0) throw new CxProcessError(`cx grammar installation failed (${String(code)}): ${stderr}`);
}

function terminateProcessTree(child: ReturnType<typeof spawn>, force = false): void {
  if (!child.pid || child.exitCode !== null) return;
  if (process.platform === "win32") {
    const killer = spawn("taskkill", ["/pid", String(child.pid), "/T", "/F"], { windowsHide: true, stdio: "ignore", shell: false });
    killer.unref();
    return;
  }
  try { process.kill(-child.pid, force ? "SIGKILL" : "SIGTERM"); } catch { child.kill(force ? "SIGKILL" : "SIGTERM"); }
}

function limitedEnvironment(): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = {};
  for (const key of ["HOME", "PATH", "TMPDIR", "CX_CACHE_DIR"]) if (process.env[key]) env[key] = process.env[key];
  return env;
}

export async function runCx(options: RunCxOptions): Promise<CxRunResult> {
  const start = performance.now();
  const work = await mkdtemp(join(tmpdir(), "pi-cx-output-"));
  const outputPath = join(work, "stdout.json");
  await mkdir(work, { recursive: true });
  const output = createWriteStream(outputPath, { flags: "wx", mode: 0o600 });
  const argv = [options.command, ...(options.args ?? []), "--root", options.cwd, "--json"];
  const child = spawn(options.binary, argv, {
    cwd: options.cwd,
    env: limitedEnvironment(),
    shell: false,
    detached: process.platform !== "win32",
    stdio: ["ignore", "pipe", "pipe"],
  });
  let bytes = 0;
  let lines = 0;
  let tooLarge = false;
  const chunks: Buffer[] = [];
  let bufferedBytes = 0;
  let stderr = Buffer.alloc(0);
  let timedOut = false;
  let aborted = false;

  child.stdout.on("data", (chunk: Buffer) => {
    output.write(chunk);
    bytes += chunk.length;
    lines += chunk.reduce((n, b) => n + (b === 10 ? 1 : 0), 0);
    if (!tooLarge && bytes <= MAX_STDOUT_BYTES && lines <= MAX_STDOUT_LINES) {
      chunks.push(chunk); bufferedBytes += chunk.length;
    } else {
      if (!tooLarge && bufferedBytes < MAX_STDOUT_BYTES) {
        const prefix = chunk.subarray(0, MAX_STDOUT_BYTES - bufferedBytes);
        chunks.push(prefix); bufferedBytes += prefix.length;
      }
      tooLarge = true;
    }
  });
  child.stderr.on("data", (chunk: Buffer) => {
    stderr = Buffer.concat([stderr, chunk]);
    if (stderr.length > MAX_STDERR_BYTES) stderr = stderr.subarray(stderr.length - MAX_STDERR_BYTES);
  });

  const terminate = () => {
    if (child.exitCode !== null) return;
    terminateProcessTree(child);
    setTimeout(() => { if (child.exitCode === null) terminateProcessTree(child, true); }, 1000).unref();
  };
  const abortHandler = () => { aborted = true; terminate(); };
  options.signal?.addEventListener("abort", abortHandler, { once: true });
  if (options.signal?.aborted) abortHandler();
  const timeout = setTimeout(() => { timedOut = true; terminate(); }, options.timeoutMs ?? 60_000);
  timeout.unref();

  const { code, signal } = await new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((resolve, reject) => {
    child.once("error", reject);
    child.once("close", (code, signal) => resolve({ code, signal }));
  }).finally(() => {
    clearTimeout(timeout);
    options.signal?.removeEventListener("abort", abortHandler);
  });
  await new Promise<void>((resolve, reject) => output.end((error?: Error | null) => error ? reject(error) : resolve()));
  const details: RunDetails = {
    durationMs: Math.round(performance.now() - start), exitCode: code, killed: signal !== null || child.killed,
    stderr: stderr.toString("utf8"), binaryVersion: options.binaryVersion,
  };
  if (aborted) { await rm(work, { recursive: true, force: true }); throw new CxProcessError("cx query cancelled", undefined, details); }
  if (timedOut) { await rm(work, { recursive: true, force: true }); throw new CxProcessError(`cx query timed out after ${options.timeoutMs ?? 60_000}ms`, undefined, details); }
  if (tooLarge) {
    details.truncated = { bytes, lines, path: outputPath };
    const prefix = Buffer.concat(chunks).toString("utf8");
    if (code !== 0) {
      throw new CxProcessError(`cx produced oversized output and exited ${String(code)}; stderr: ${details.stderr.slice(-1000)}`, undefined, details);
    }
    const schema = prefix.match(/"schema_version"\s*:\s*(\d+)/)?.[1];
    if (schema !== "1") throw new ProtocolError(`incompatible or missing cx schema in oversized output (${schema ?? "unknown"}); reinstall pi-cx`);
    const fallback = {
      schema_version: 1,
      truncation: { reason: "pi_output_limit", original_bytes: bytes, original_lines: lines, full_output_path: outputPath },
      query: { kind: options.command },
      page: { offset: 0, suggestion: "Use a smaller limit or increase offset to request the next page." },
    };
    const raw = JSON.stringify(fallback, null, 2);
    return { raw, envelope: fallback as unknown as CxEnvelope, details };
  }
  await rm(work, { recursive: true, force: true });
  const raw = Buffer.concat(chunks).toString("utf8");
  let envelope: CxEnvelope;
  try { envelope = parseEnvelope(raw); } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new ProtocolError(`${message}${details.stderr ? `; stderr: ${details.stderr.slice(-1000)}` : ""}`);
  }
  if (code === 0 && !envelope.error) return { raw, envelope, details };
  if (code === 1 && envelope.error) throw new CxProcessError(JSON.stringify({ error: envelope.error, process: { duration_ms: details.durationMs, exit_code: code, killed: details.killed, stderr: details.stderr } }), envelope, details);
  if (code === 2) throw new ProtocolError(`cx rejected extension argv (CLI mismatch); stderr: ${details.stderr.slice(-1000)}`);
  throw new CxProcessError(`cx exited ${String(code)}${envelope.error ? `: ${JSON.stringify(envelope.error)}` : ""}`, envelope, details);
}
