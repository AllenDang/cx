#!/usr/bin/env python3
"""Combined developer pilot runner. Reads task inputs, never the separate gold.

NAV: discovery -> multi-hop review. DIFF: real commit addition/rollback review.
A/B use existing tools; B caches duplicate lookups without adding parser facts.
No model sessions or project program execution. All stdout/stderr and cost saved.
"""
import argparse
from collections import deque
import hashlib
import json
import os
import re
from pathlib import Path
import subprocess
import tempfile
import time

from ange_tasks import Workflow, BudgetExceeded


class Pipeline(Workflow):
    def check_budget(self):
        if len(self.steps)>30 or sum(s['request_bytes']+s['stdout_bytes'] for s in self.steps)>256*1024 or time.perf_counter()-self.start>60:
            raise BudgetExceeded('combined workflow budget exceeded')

    def external(self, argv, purpose):
        start=time.perf_counter();result=subprocess.run(argv,cwd=self.root,env=self.env,capture_output=True,timeout=60)
        n=len(self.steps);(self.dest/f'{n:02}.stdout').write_bytes(result.stdout);(self.dest/f'{n:02}.stderr').write_bytes(result.stderr)
        request={'tool':'git','argv':argv}
        self.steps.append({'purpose':purpose,'request':request,'request_bytes':len(json.dumps(request).encode()),'stdout_bytes':len(result.stdout),'stderr_bytes':len(result.stderr),'wall':time.perf_counter()-start,'exit_code':result.returncode,'rss':None,'user':None,'sys':None})
        self.check_budget()
        if result.returncode:raise RuntimeError(f'git failed: {result.stderr.decode(errors="replace")[:400]}')
        return result.stdout


def run_case(wf, task, arm):
    result={'task':task['id'],'arm':arm,'root':None,'reach':[], 'files':[], 'symbols':[], 'patch':'','error':None}
    try:
        if task['kind']=='navigate':
            if arm=='C':
                options=['context','--query',task['query'],'--limit','10','--byte-budget','32768','--no-tests','--fresh','verified']
                found=wf.query(options,'discover')['results']
                roots=[r for r in found if r['kind']=='fn' and r['role']=='definition' and r['file'].startswith('src/')]
            else:
                found,_=wf.collect(['symbols','--name',task['pattern'],'--kind','fn','--role','definition','--limit','50'],'discover')
                roots=[r for r in found if r['file'].startswith('src/')]
            if not roots:raise RuntimeError('no source function discovered')
            root=roots[0];result['root']=root['name']
            if arm=='C':
                rows,docs=wf.collect(['impact','--name',root['name'],'--file',root['file'],'--byte-offset',str(root['byte_range'][0]),'--max-depth',str(task['depth']),'--limit','50','--byte-budget','32768','--no-tests','--fresh','verified'],'impact')
                result['reach']=[{'name':r['symbol']['name'],'file':r['symbol']['id']['file'],'depth':r['depth'],'supported':r.get('evidence')=='supported' if 'evidence' in r else r['supported'] is not None} for r in rows if r['symbol']['id']['kind']=='symbol' and r['symbol']['id']['file'].startswith('src/')]
                result['analysis']=docs[-1]['analysis']
            else:
                queue=deque([(root['name'],root['qualified'],root['file'],0,False)])
                seen={(root['file'],root['qualified'])};cache={}; identities=set()
                if arm=='B' and len(roots)==1:identities.add((root['name'],root['qualified']))
                while queue:
                    name,qualified,file,depth,path_possible=queue.popleft()
                    if depth>=task['depth']:continue
                    key=(name,qualified)
                    if arm=='B' and key in cache:rows=cache[key]
                    else:
                        # Validate global identity before joining string endpoints.
                        if arm!='B' or key not in identities:
                            defs,_=wf.collect(['symbols','--name',name,'--scope',qualified,'--role','definition','--kind','fn','--limit','50'],'identity')
                            if len(defs)!=1:raise RuntimeError('baseline cannot uniquely join displayed target identity')
                            identities.add(key)
                        rows,_=wf.collect(['callers','--name',name,'--scope',qualified,'--limit','50'],'callers')
                        if arm=='B':cache[key]=rows
                    if any(r['file'].startswith('src/') and not r['to'] for r in rows):
                        refs,_=wf.collect(['references','--name',name,'--file','src','--context','--limit','100'],'syntax_fallback')
                        # Bounded mechanical fallback: only one unqualified/namespace
                        # spelling on an AST-classified call line, never a receiver.
                        for ref in refs:
                            text=ref.get('context','')
                            if ref.get('evidence')!='call' or not ref.get('caller'):continue
                            if len(re.findall(re.escape(name),text))!=1:continue
                            if not re.search(r'(?<![\w.>])(?:[A-Za-z_]\w*::)*'+re.escape(name)+r'\s*\(',text):continue
                            rows.append({'file':ref['file'],'line':ref['line'],'from':ref['caller'],'to':qualified,'resolution':'syntax'})
                    for row in rows:
                        if not row['file'].startswith('src/') or row['from'].startswith('('):continue
                        if not row['to']:continue
                        parent=row['from'].split('::')[-1].split('.')[-1]
                        parent_qualified=row['from']
                        if '::' not in parent_qualified:
                            located,_=wf.collect(['symbols','--name',parent,'--file',row['file'],'--role','definition','--kind','fn','--limit','50'],'caller_identity')
                            if len(located)!=1:raise RuntimeError('caller identity is ambiguous')
                            parent_qualified=located[0]['qualified']
                        identity=(row['file'],parent_qualified)
                        if identity in seen:continue
                        seen.add(identity);possible=path_possible or row['resolution']=='syntax'
                        result['reach'].append({'name':parent,'file':row['file'],'depth':depth+1,'supported':not possible})
                        queue.append((parent,parent_qualified,row['file'],depth+1,possible))
        else:
            if arm=='C':
                rows,docs=wf.collect(['changes','--base',task['base'],'--head',task['head'],'--limit','50','--byte-budget','32768'],'changes')
                result['files']=[r['file'] for r in rows]
                focus=next(r for r in rows if r['file']==task['focus'])
                result['symbols']=focus['symbols'];result['change']=focus['change'];result['analysis']=docs[-1]['analysis']
            else:
                prefix=task['subroot']
                command=['git','-c','core.fsmonitor=false','--no-pager','diff','--no-ext-diff','--no-textconv','--no-renames']
                status=wf.external([*command,'--relative='+prefix,'--name-status','-z',task['base'],task['head'],'--',':(top)'+prefix],'changed_files').split(b'\0')
                result['files']=[p.decode() for p in status[1::2] if p]
                patch=wf.external([*command,'--unified=3',task['base'],task['head'],'--',':(top)'+prefix+'/'+task['focus']],'source_delta').decode()
                result['patch']=patch
                # Both old workflows may read the small relevant patch directly.
        return wf.finish(result)
    except (RuntimeError, StopIteration, BudgetExceeded, subprocess.TimeoutExpired) as e:
        result['error']=str(e);return wf.finish(result)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for key in ['tasks','repo','baseline','candidate','grammars','output']:parser.add_argument('--'+key,type=Path,required=True)
    args=parser.parse_args();tasks=json.loads(args.tasks.read_text());out=args.output.resolve();out.mkdir(parents=True,exist_ok=False)
    root=args.repo.resolve();binaries={'A':args.baseline.resolve(),'B':args.baseline.resolve(),'C':args.candidate.resolve()}
    envs={};cold=[]
    for arm,binary in binaries.items():
        cache=out/('cache-'+arm);cache.mkdir();envs[arm]=dict(os.environ,CX_CACHE_DIR=str(cache),TREE_SITTER_LANGUAGE_PACK_LIBS_DIR=str(args.grammars.resolve()),TREE_SITTER_LANGUAGE_PACK_MANIFEST_URL='file:///cx-no-pilot-download.json')
        wf=Pipeline(root,binary,envs[arm],out/('cold-'+arm));doc=wf.query(['symbols','--limit','1'],'cold_index');cold.append(wf.finish({'arm':arm,'freshness':doc['freshness'],'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest()}))
    records=[]
    for repeat in range(3):
        arms=['A','B','C'];arms=arms[repeat:]+arms[:repeat]
        for task in tasks:
            for arm in arms:
                cwd=root if task['kind']=='navigate' else root/task['subroot']
                wf=Pipeline(cwd,binaries[arm],envs[arm],out/f'{repeat}-{task["id"]}-{arm}')
                record=run_case(wf,task,arm);record['repeat']=repeat;records.append(record)
                print(repeat,task['id'],arm,record['operations'],record['communication_bytes'],round(record['wall'],3),record['error'],flush=True)
                (out/'runs.json').write_text(json.dumps({'cold':cold,'records':records},ensure_ascii=False,indent=2)+'\n')


if __name__=='__main__':main()
