import assert from "node:assert/strict";
import { mkdtemp, writeFile, symlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { buildImpactArgs, buildChangesArgs, buildContextArgs, registerCxTools } from "../../extensions/pi-cx/tools.js";
import { canonicalRoot } from "../../extensions/pi-cx/paths.js";
import { parseEnvelope } from "../../extensions/pi-cx/protocol.js";
import { DirtyPathCoordinator } from "../../extensions/pi-cx/dirty-paths.js";

const taskEnvelope = (kind: string) => ({ schema_version:1, query:{kind}, freshness:{generation:1}, page:{total:null,offset:0,limit:50,truncated:false}, results:[],warnings:["partial"],next_queries:[],error:null,analysis:{complete:false,discovered_count:0,snapshot:"abc"} });
test("task envelopes distinguish unknown totals from output pagination", () => {
  for (const kind of ["impact","changes","context"]) {
    const parsed=parseEnvelope(JSON.stringify(taskEnvelope(kind)));
    assert.equal(parsed.page.total,null);assert.equal(parsed.analysis?.complete,false);
    assert.throws(()=>parseEnvelope(JSON.stringify({...taskEnvelope(kind),analysis:undefined})), /analysis/);
    assert.throws(()=>parseEnvelope(JSON.stringify({...taskEnvelope(kind),page:{...taskEnvelope(kind).page,total:0}})), /total/);
  }
});
test("new tool argv bounds, selectors and paths are validated", async () => {
  const temp=await mkdtemp(join(tmpdir(),"pi-cx-tasks-"));await writeFile(join(temp,"a.rs"),"fn leaf() {}\n");const root=await canonicalRoot(temp);
  const argv=await buildImpactArgs(root,{name:"leaf",file:"@a.rs",line:1,maxDepth:2,maxNodes:10,maxEdges:20});
  assert.deepEqual(argv.slice(0,6),["--name","leaf","--file","a.rs","--line","1"]);
  assert.ok(argv.includes("--max-depth"));assert.ok(argv.includes("--max-edges"));
  await assert.rejects(()=>buildImpactArgs(root,{name:"leaf",line:1}),/file/);
  await assert.rejects(()=>buildImpactArgs(root,{name:"leaf",file:"a.rs",line:1,byteOffset:0}),/line.*byteOffset/);
  await assert.rejects(()=>buildImpactArgs(root,{name:"leaf",maxNodes:0}),/maxNodes/);
  await assert.rejects(()=>buildImpactArgs(root,{name:"leaf",maxDepth:33}),/maxDepth/);
  const outside=await mkdtemp(join(tmpdir(),"pi-cx-escape-"));await symlink(outside,join(root,"escape"));
  await assert.rejects(()=>buildImpactArgs(root,{name:"leaf",file:"escape"}),/escapes/);
  assert.throws(()=>buildChangesArgs({staged:true,head:"HEAD"}),/staged/);
  assert.throws(()=>buildChangesArgs({mergeBase:true}),/head/);
  assert.ok(buildChangesArgs({base:"HEAD",head:"feature;literal",mergeBase:true,impact:true}).includes("feature;literal"));
  assert.throws(()=>buildContextArgs({query:""}),/non-empty/);
  assert.throws(()=>buildContextArgs({query:"cache",byteBudget:40000}),/byteBudget/);
  assert.ok(buildContextArgs({query:"cache invalidation",includeBody:true}).includes("--include-body"));
});
test("all three tools refresh dirty paths before querying, and pre-abort starts nothing", async () => {
  const temp=await mkdtemp(join(tmpdir(),"pi-cx-task-dirty-"));await writeFile(join(temp,"a.rs"),"fn leaf() {}\n");const root=await canonicalRoot(temp);
  for(const [name,params] of [["impact",{name:"leaf"}],["changes",{}],["context",{query:"leaf"}]] as const) {
    const dirty=new DirtyPathCoordinator();dirty.activate(root);dirty.markDirty({version:1,source:"test",cwd:root,paths:["a.rs"]});
    const tools=new Map<string,any>();const calls:string[]=[];
    registerCxTools({registerTool(t:any){tools.set(t.name,t)}} as any,dirty,{
      validateBinary:async()=>({binary:"/verified/cx",version:"0.8.0",digest:"verified",manifest:{} as any}),
      ensureGrammars:async()=>({copied:[],replaced:[],unchanged:[]}),runMaintenance:async()=>{},
      run:async(options)=>{calls.push(options.command);const envelope=options.command==="refresh"?{...taskEnvelope("refresh"),page:{total:1,offset:0,limit:1,truncated:false},results:[{file:"a.rs",status:"unchanged"}]}:taskEnvelope(name);
        return {raw:JSON.stringify(envelope),envelope,details:{durationMs:1,exitCode:0,killed:false,stderr:""}};},
    });
    const ctx={cwd:root,hasUI:false,ui:{}};
    const result=await tools.get("cx_"+name).execute("id",params,undefined,undefined,ctx);
    assert.deepEqual(calls,["refresh",name]);assert.equal(result.details.analysis.complete,false);
    const controller=new AbortController();controller.abort();
    await assert.rejects(()=>tools.get("cx_"+name).execute("id",params,controller.signal,undefined,ctx),/cancelled/);
    assert.deepEqual(calls,["refresh",name]);
  }
});
