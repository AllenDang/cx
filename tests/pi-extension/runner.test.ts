import assert from "node:assert/strict";
import { chmod, mkdtemp, realpath, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { CxProcessError, runCx } from "../../extensions/pi-cx/runner.js";
import { ProtocolError } from "../../extensions/pi-cx/protocol.js";

async function fake(): Promise<{ binary: string; cwd: string }> {
  const cwd = await mkdtemp(join(tmpdir(), "pi-cx-runner-")), binary = join(cwd, "fake-cx");
  await writeFile(binary, `#!/usr/bin/env node
const command = process.argv[2];
const base = {schema_version:1,query:{kind:command},freshness:{generation:1},page:{total:0,offset:0,limit:10,truncated:false},results:[],warnings:[],next_queries:[],error:null};
if(command==='sleep') setTimeout(()=>{console.log(JSON.stringify(base))},10000);
else if(command==='invalid') console.log('bad json');
else if(command==='schema2') console.log(JSON.stringify({...base,schema_version:2}));
else if(command==='error'){ console.log(JSON.stringify({...base,error:{code:'file_not_indexed',message:'nope'}})); process.exitCode=1; }
else if(command==='misuse'){ console.error('bad argv'); process.exitCode=2; }
else if(command==='large'){ base.results=[{body:'x'.repeat(70000)}]; console.log(JSON.stringify(base)); }
else { base.results=[{argv:process.argv.slice(2),cwd:process.cwd()}]; console.log(JSON.stringify(base)); }
`); await chmod(binary, 0o755); return { binary, cwd };
}
test("runs with argv, fixed cwd/root, and valid raw envelope", async () => {
  const f = await fake(); const result = await runCx({ ...f, command: "ok", args: ["semi;colon"] });
  const row = result.envelope.results[0] as any;
  assert.deepEqual(row.argv, ["ok", "semi;colon", "--root", f.cwd, "--json"]); assert.equal(row.cwd, await realpath(f.cwd)); assert.equal(result.raw.trim().startsWith("{"), true);
});
test("distinguishes cx errors, CLI mismatch, and invalid protocol", async () => {
  const f = await fake(); await assert.rejects(() => runCx({ ...f, command: "error" }), CxProcessError);
  await assert.rejects(() => runCx({ ...f, command: "misuse" }), ProtocolError);
  await assert.rejects(() => runCx({ ...f, command: "invalid" }), ProtocolError);
  await assert.rejects(() => runCx({ ...f, command: "schema2" }), /incompatible cx schema/);
});
test("oversized output remains valid JSON and points to complete output", async () => {
  const f = await fake(); const result = await runCx({ ...f, command: "large" }); const fallback = JSON.parse(result.raw);
  assert.equal(fallback.truncation.reason, "pi_output_limit"); assert.ok(result.details.truncated?.path); assert.ok(Buffer.byteLength(result.raw) < 50 * 1024);
});
test("timeout and AbortSignal terminate the process", async () => {
  const f = await fake(); await assert.rejects(() => runCx({ ...f, command: "sleep", timeoutMs: 20 }), /timed out/);
  const controller = new AbortController(); const promise = runCx({ ...f, command: "sleep", signal: controller.signal }); setTimeout(() => controller.abort(), 20);
  await assert.rejects(() => promise, /cancelled/);
});
