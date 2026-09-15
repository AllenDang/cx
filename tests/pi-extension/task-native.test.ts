import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { copyFile, mkdir, mkdtemp, readFile, realpath, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir, homedir } from "node:os";
import { join, resolve } from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import test from "node:test";
import { validateBundledBinary } from "../../extensions/pi-cx/binary.js";
import { ensureBundledGrammars } from "../../extensions/pi-cx/grammars.js";
import { registerCxTools, buildImpactArgs, buildChangesArgs, buildContextArgs } from "../../extensions/pi-cx/tools.js";
import { runCx, runCxMaintenance } from "../../extensions/pi-cx/runner.js";
import { DirtyPathCoordinator } from "../../extensions/pi-cx/dirty-paths.js";
import { GRAMMAR_NAMES, LANGUAGE_PACK_VERSION, PACKAGE_VERSION, PLATFORM, TARGET } from "../../extensions/pi-cx/types.js";
import { grammarFilename } from "../../extensions/pi-cx/platform.js";

const exec = promisify(execFile);
test("manifest-verified current binary: impact/changes/context tool output equals CLI", async () => {
  const work = await mkdtemp(join(tmpdir(), "pi-cx-task-native-"));
  const previousCache = process.env.CX_CACHE_DIR;
  try {
    const stage = join(work, "asset"), root = join(work, "project"), cache = join(work, "cache");
    await mkdir(join(stage,"bin"),{recursive:true});await mkdir(join(stage,"grammars"));await mkdir(root);
    const binary = join(stage,"bin",PLATFORM.binaryName);
    await copyFile(resolve(process.env.PI_CX_TEST_BINARY ?? "target/debug/cx"),binary);
    const grammars = process.env.PI_CX_TEST_GRAMMARS ?? join(homedir(), process.platform === "darwin" ? "Library/Caches/cx" : ".cache/cx", "grammars", "tree-sitter-language-pack", "v"+LANGUAGE_PACK_VERSION, "libs");
    const files: Record<string,{sha256:string;bytes:number}> = {};
    for (const name of GRAMMAR_NAMES) await copyFile(join(grammars,grammarFilename(name)),join(stage,"grammars",grammarFilename(name)));
    for (const path of [`bin/${PLATFORM.binaryName}`, ...GRAMMAR_NAMES.map(n=>`grammars/${grammarFilename(n)}`)]) {
      const data = await readFile(join(stage,path));files[path]={sha256:createHash("sha256").update(data).digest("hex"),bytes:data.length};
    }
    const manifest = {format_version:1,package:"pi-cx",cx_version:PACKAGE_VERSION,cx_schema_version:1,target:TARGET,tree_sitter_language_pack_version:LANGUAGE_PACK_VERSION,languages:["rust","c","cpp","javascript","jsx","typescript","tsx","python","go","markdown","html"],files};
    const manifestPath=join(stage,"manifest.json");await writeFile(manifestPath,JSON.stringify(manifest));
    const validated=await validateBundledBinary({binary,manifestPath,vendorRoot:stage});
    assert.equal(validated.digest, files[`bin/${PLATFORM.binaryName}`]!.sha256);
    assert.equal(validated.version,PACKAGE_VERSION);
    process.env.CX_CACHE_DIR=cache;
    await ensureBundledGrammars(manifest,stage,cache);
    await writeFile(join(root,"a.rs"),"fn leaf() {}\nfn middle() { leaf(); }\nfn entry() { middle(); }\n");
    await exec("git",["init","-q"],{cwd:root});await exec("git",["add","--all"],{cwd:root});
    await exec("git",["-c","user.name=Fixture","-c","user.email=fixture@example.invalid","-c","commit.gpgsign=false","-c","core.hooksPath=/dev/null","commit","-qm","base"],{cwd:root});
    const cwd=await realpath(root); const tools=new Map<string,any>();const dirty=new DirtyPathCoordinator();
    registerCxTools({registerTool(t:any){tools.set(t.name,t)}} as any,dirty,{
      validateBinary:async()=>validated,ensureGrammars:(m)=>ensureBundledGrammars(m,stage,cache),run:runCx,runMaintenance:runCxMaintenance,
    });
    const ctx={cwd,hasUI:false,ui:{}};
    const compare=async(name:string,params:any,args:string[])=>{
      const via=await tools.get("cx_"+name).execute("id",params,undefined,undefined,ctx);
      const direct=await exec(binary,[name,...args,"--root",cwd,"--json"],{cwd,env:{...process.env,TREE_SITTER_LANGUAGE_PACK_LIBS_DIR:join(cache,"grammars")}});
      const a=JSON.parse(via.content[0].text),b=JSON.parse(direct.stdout);
      assert.deepEqual(a.results,b.results);assert.deepEqual(a.analysis,b.analysis);assert.deepEqual(a.page,b.page);
      assert.equal(a.error,null);assert.equal(b.error,null);return a;
    };
    const impacted=await compare("impact",{name:"leaf"},await buildImpactArgs(cwd,{name:"leaf"}));
    assert.deepEqual(impacted.results.map((r:any)=>[r.symbol.name,r.depth]),[["middle",1],["entry",2]]);
    const context=await compare("context",{query:"middle",includeBody:true},buildContextArgs({query:"middle",includeBody:true}));
    assert.equal(context.results[0].name,"middle");
    await writeFile(join(root,"a.rs"),"fn leaf() { let n = 1; }\nfn middle() { leaf(); }\nfn entry() { middle(); }\n");
    dirty.markDirty({version:1,source:"native-test",cwd,paths:[join(root,"a.rs")]});
    const changed=await compare("changes",{impact:true},buildChangesArgs({impact:true}));
    assert.equal(changed.results.length,1);assert.equal(changed.results[0].symbols[0].after.name,"leaf");
    assert.equal(changed.results[0].symbols[0].before_impact.results[0].symbol.name,"middle");
    await assert.rejects(()=>tools.get("cx_impact").execute("bad",{name:"absent"},undefined,undefined,ctx),/subject_not_found/);
    assert.equal((await stat(binary)).size,files[`bin/${PLATFORM.binaryName}`]!.bytes);
  } finally {
    if(previousCache===undefined)delete process.env.CX_CACHE_DIR;else process.env.CX_CACHE_DIR=previousCache;
    await rm(work,{recursive:true,force:true});
  }
});
