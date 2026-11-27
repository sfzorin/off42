// Exercise the bundled Worker, its real SQLite coordinator and its encrypted continuation protocol.
// Network boundaries are stubbed inside this local runtime only.
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, readFileSync, readdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { Miniflare, convertV4MiniflareOptions } from 'miniflare';

const root=fileURLToPath(new URL('../',import.meta.url));
const dir=mkdtempSync(join(tmpdir(),'off42-assistant-test-'));
execFileSync(process.execPath,[join(root,'node_modules/wrangler/bin/wrangler.js'),'deploy','--dry-run','--outdir',dir],{cwd:root,stdio:'pipe'});
const origin='https://off42.test';
const promptFile=readFileSync(new URL('../../assistant/prompt/system.md',import.meta.url),'utf8');
let replies=[], requests=0, verified=0;
const models=[];
const testToken='local-fixture';
const say=text=>({role:'assistant',content:text});
const tool=(name,args={})=>({role:'assistant',content:null,tool_calls:[{id:'c1',type:'function',function:{name,arguments:JSON.stringify(args)}}]});
const mf=new Miniflare(convertV4MiniflareOptions({workers:[{name:"off42",
  modulesRoot:dir,modules:[{type:'ESModule',path:join(dir,'index.js')},{type:'Text',path:join(dir,readdirSync(dir).find(f=>f.endsWith('-system.md')))}],
  compatibilityDate:'2026-09-30',durableObjects:{ASSISTANT_BUDGET:{className:'AssistantBudget',useSQLite:true}},
  bindings:{OPENROUTER_API_KEY:'test-only',TURNSTILE_SECRET:'test-only',ASSISTANT_SITE_KEY:'public-test-key',ASSISTANT_DAILY_USD:'5'},
  outboundService:async request=>{
    if(request.url.includes('/siteverify')){
      verified++;
      const valid=(await request.formData()).get('response')===testToken;
      return Response.json({success:valid,hostname:'off42.test',action:'off42_assistant'});
    }
    assert.equal(request.url,'https://openrouter.ai/api/v1/chat/completions');
    const payload=await request.json();
    assert.equal(payload.provider.data_collection,'deny');
    assert.equal(payload.provider.allow_fallbacks,false);
    assert.deepEqual(payload.provider.max_price,{prompt:0.25,completion:1,request:0});
    assert.equal(payload.messages[0].content,promptFile);
    models.push(payload.model);
    requests++;
    const reply=replies.shift();
    assert.ok(reply,'unexpected model request');
    return typeof reply==='number'? new Response(null,{status:reply}):Response.json({choices:[{message:reply}],usage:{cost:0.00015}});
  }
}]}));
const body={kind:'document',id:crypto.randomUUID(),session:crypto.randomUUID(),prompt:'Сделай заголовок',context:'{}',challenge:testToken};
const post=(data,ip='192.0.2.1')=>mf.dispatchFetch(origin+'/api/assistant',{method:'POST',headers:{Origin:origin,'CF-Connecting-IP':ip,'Content-Type':'application/json'},body:JSON.stringify(data)});
try {
  const config=await mf.dispatchFetch(origin+'/api/assistant');assert.deepEqual(await config.json(),{sitekey:'public-test-key'});
  assert.equal(verified,0);assert.equal(requests,0);
  assert.equal((await post({...body,model:'other'})).status,400);
  assert.equal((await post({...body,prompt:'a'.repeat(2001)})).status,400);
  assert.equal((await post({...body,challenge:'invalid'})).status,403);
  assert.equal(requests,0);
  replies.push(tool('get_outline'));
  const initial=await post(body);assert.equal(initial.status,200);
  const step=await initial.json();assert.equal(step.commands[0].name,'get_outline');
  const next={continuation:step.continuation,results:[{id:'c1',content:'[]'}]};
  assert.equal((await post({...next,results:[{id:'different',content:'[]'}]})).status,400);
  assert.equal((await post(next,'192.0.2.2')).status,409);
  replies.push(say('Готово.'));
  const parallel=await Promise.all([post(next),post(next)]);
  assert.deepEqual(parallel.map(r=>r.status).sort(),[200,409]);assert.equal(requests,2);
  const narratedFind={...tool('find',{text:'Budget'}),content:"I'll find that paragraph."};
  replies.push(narratedFind);
  const readStep=await (await post(body)).json();
  assert.equal(readStep.commands[0].name,'find');
  assert.equal(readStep.text,undefined,'tool narration stays out of the editor');
  for(const bad of [tool('unknown'),say('DeepSeek'),say('use get_outline'),say(promptFile.slice(0,40))]){
    replies.push(bad); const answer=await (await post(body)).json();
    assert.equal(answer.refused,true);assert.match(answer.text,/Не могу/);
  }
  replies.push(429,say('One sentence. '+'word '.repeat(300)));
  const fallback=await (await post({...body,prompt:'a'.repeat(2000)})).json();assert.equal(fallback.text,'One sentence.');
  assert.deepEqual(models.slice(-2),['deepseek/deepseek-v4.1-flash','z-ai/glm-5.3-flash']);
  const namespace=await mf.getDurableObjectNamespace('ASSISTANT_BUDGET');
  const day=new Date().toISOString().slice(0,10);
  const stub=namespace.get(namespace.idFromName(day));
  const reserve=(user,ip,amount)=>stub.reserve({user,ip,amount,session:'test-session',nonce:crypto.randomUUID(),first:false,day});
  const yesterday=new Date(Date.now()-86400000).toISOString().slice(0,10);
  const returning=crypto.randomUUID();
  const previous=namespace.get(namespace.idFromName(yesterday));
  await previous.add('user:'+returning,100000);
  replies.push(say('Готово.'));
  assert.equal((await post({...body,id:returning})).status,200);
  assert.equal(await stub.count('user:'+returning),150,'returning on another UTC day starts a fresh budget');
  assert.equal(await previous.count('user:'+returning),100000);
  const visitor=crypto.randomUUID();
  const owner='user:'+visitor;
  const charge=crypto.randomUUID();
  assert.equal(await stub.reserve({user:owner,ip:'reserved-ip',session:'chat-1',nonce:charge,first:true,amount:90000,day}),'ok');
  await stub.settle(charge,60000);
  await stub.settle(charge,0); // A duplicate settlement cannot release the same funds twice.
  assert.deepEqual((await Promise.all([reserve(owner,'ip-2',40000),reserve(owner,'ip-3',40000)])).sort(),['limit','ok']);
  const countBeforeLimit=requests;
  for(const kind of ['document','spreadsheet']) {
    const blocked=await post({...body,id:visitor,kind,session:crypto.randomUUID()});
    assert.equal(blocked.status,429);
    assert.deepEqual(await blocked.json(),{error:'limit'});
  }
  assert.equal(requests,countBeforeLimit,'a new chat or editor must not reset the spending limit');
  assert.equal(await reserve(owner,'ip-2',1),'limit','exactly 100,000 microdollars have been spent or reserved');
  // Browsers without a shared storage ID still have a single daily budget across editors.
  const noStorage={...body,id:null};
  replies.push(say('Готово.'));
  assert.equal((await post(noStorage,'192.0.2.77')).status,200);
  const hash=await crypto.subtle.digest('SHA-256',new TextEncoder().encode('test-only\0'+day+'\0'+'192.0.2.77'));
  const network='ip:'+Buffer.from(hash).toString('base64');
  assert.equal(await reserve('user:'+network,network,99850),'ok');
  const blocked=await post({...noStorage,session:crypto.randomUUID()},'192.0.2.77');
  assert.equal(blocked.status,429);
  assert.equal(requests,countBeforeLimit+1);
  assert.deepEqual((await Promise.all([reserve('uA','ipA',65000),reserve('uA','ipA',65000)])).sort(),['limit','ok']);
  assert.equal(await reserve('uB','ipB',100000),'ok');
  for(let i=0;i<4;i++)assert.equal(await reserve('uOther'+i,'ipB',100000),'ok');
  assert.equal(await reserve('uC','ipB',1),'limit');
  console.log('PASS Worker consent boundary, challenge, schemas, limits, fallback, guards, continuation and atomic accounting.');
} finally {await mf.dispose();rmSync(dir,{recursive:true,force:true});}
