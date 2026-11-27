import instructions from '../../assistant/prompt/system.md';
import documentTools from '../../assistant/tools/document.json';
import spreadsheetTools from '../../assistant/tools/spreadsheet.json';
import { validate } from '../../assistant/eval/lib/schema.mjs';

const tools = { document: documentTools, spreadsheet: spreadsheetTools };
const models = [
  ['deepseek/deepseek-v4.1-flash',['alibaba','together','google-vertex']],
  ['z-ai/glm-5.3-flash',['z-ai','fireworks']],
];
const encoder = new TextEncoder();
const decoder = new TextDecoder();
const maxBytes = 400000;
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const normal = s => s.toLowerCase().replace(/\s+/g,' ').trim();
const promptText = normal(instructions);
const windows = new Set(Array.from({length:promptText.length-39},(_,i)=>promptText.slice(i,i+40)));
const names = Object.values(tools).flat().map(t=>t.function.name);
const provider = /deepseek|\bglm\b|zhipu|z\.ai|alibaba|qwen|openrouter|openai|\bgpt\b|claude|anthropic|llama|gemini|mistral/i;
const refusal = {
  en: "I can't answer that: I only help with this document or spreadsheet.",
  ru: "Не могу ответить: я помогаю только с этим документом или таблицей.",
  de: "Das kann ich nicht beantworten: Ich helfe nur mit diesem Dokument oder dieser Tabelle.",
  es: "No puedo responder a eso: solo ayudo con este documento u hoja de cálculo.",
};
function fixed(prompt) {
  return refusal[/\p{Script=Cyrillic}/u.test(prompt)?'ru':/[äöüß]|\b(wie|was|bitte|ich|ist|der|die|das)\b/i.test(prompt)?'de':
    /[ñ¿¡]|\b(qué|cómo|para|una|quién|por favor)\b/i.test(prompt)?'es':'en'];
}
const response = (data,status=200) => Response.json(data,{status,headers:{'Cache-Control':'no-store','X-Content-Type-Options':'nosniff'}});
class Fault extends Error { constructor(code,status=400){super(code);this.status=status;} }
const requireInput = ok => { if (!ok) throw new Fault('invalid'); };
const fields = (obj,keys) => obj && typeof obj==='object' && !Array.isArray(obj) && Object.keys(obj).length===keys.length && keys.every(k=>Object.hasOwn(obj,k));
const b64 = bytes => { let s=''; for(let i=0;i<bytes.length;i+=8192)s+=String.fromCharCode(...bytes.subarray(i,i+8192)); return btoa(s); };
const unb64 = s => Uint8Array.from(atob(s),ch=>ch.charCodeAt(0));
async function digest(s) { return new Uint8Array(await crypto.subtle.digest('SHA-256',encoder.encode(s))); }
async function cryptKey(secret) { return crypto.subtle.importKey('raw',await digest('off42 assistant ticket v1 '+secret),'AES-GCM',false,['encrypt','decrypt']); }
async function seal(state,secret) {
  const iv=crypto.getRandomValues(new Uint8Array(12));
  const bytes=encoder.encode(JSON.stringify(state));
  if(bytes.length>235000)throw new Fault('size',413);
  const encrypted=await crypto.subtle.encrypt({name:'AES-GCM',iv},await cryptKey(secret),bytes);
  return b64(iv)+'.'+b64(new Uint8Array(encrypted));
}
async function unseal(token,secret) {
  try {
    const [iv,data]=token.split('.');
    return JSON.parse(decoder.decode(await crypto.subtle.decrypt({name:'AES-GCM',iv:unb64(iv)},await cryptKey(secret),unb64(data))));
  } catch {throw new Fault('expired',409);}
}
async function readBody(request,limit) {
  if(Number(request.headers.get('Content-Length'))>limit)throw new Fault('size',413);
  const reader=request.body?.getReader();if(!reader)throw new Fault('invalid');
  const parts=[];let size=0;
  for(;;){const {value,done}=await reader.read();if(done)break;
    size+=value.length;if(size>limit){await reader.cancel();throw new Fault('size',413);} parts.push(value);}
  try {return JSON.parse(await new Blob(parts).text());} catch {throw new Fault('invalid');}
}

export function filterReply(message,kind,prompt,hadTools) {
  const fail=()=>({text:fixed(prompt),refused:true});
  if (!message || message.role!=='assistant') return fail();
  let text=typeof message.content==='string'?message.content.trim():'';
  const normalized=normal(text);
  if(names.some(n=>normalized.includes(n)) || provider.test(text))return fail();
  for(let i=0;i+40<=normalized.length;i++)if(windows.has(normalized.slice(i,i+40)))return fail();
  const calls=message.tool_calls??[];
  if(!Array.isArray(calls)||calls.length>24)return fail();
  const ids=new Set();
  const commands=[];
  for(const call of calls){
    const spec=tools[kind].find(t=>t.function.name===call?.function?.name)?.function;
    if(!spec||call.type!=='function'||typeof call.id!=='string'||!call.id||call.id.length>200||ids.has(call.id))return fail();
    ids.add(call.id);
    try {
      const args=JSON.parse(call.function.arguments);
      if(validate(spec.parameters,args).length)return fail();
      commands.push({id:call.id,name:spec.name,args});
    }catch{return fail();}
  }
  if(commands.length)return {commands};
  if(!text)return fail();
  if(Object.values(refusal).some(s=>normal(s)===normalized))return fail();
  if(!hadTools && text.length>600){
    text=text.slice(0,600);
    const ends=[...text.matchAll(/[.!?。！？](?=\s|$)/g)];if(ends.length)text=text.slice(0,ends.at(-1).index+1);
  }
  return {text};
}

async function verifyChallenge(request,env,token) {
  requireInput(typeof token==='string' && token.length>0 && token.length<=2048);
  const result=await fetch('https://challenges.cloudflare.com/turnstile/v0/siteverify',{method:'POST',
    body:new URLSearchParams({secret:env.TURNSTILE_SECRET,response:token}),signal:AbortSignal.timeout(15000)});
  const data=await result.json();
  if(!result.ok||!data.success||data.hostname!==new URL(request.url).hostname||data.action!=='off42_assistant')throw new Fault('challenge',403);
}
async function processRequest(request,env){
  if(!env.OPENROUTER_API_KEY||!env.TURNSTILE_SECRET||!env.ASSISTANT_SITE_KEY||!env.ASSISTANT_BUDGET)throw new Fault('unavailable',503);
  if(request.method==='GET')return response({sitekey:env.ASSISTANT_SITE_KEY});
  if(request.method!=='POST')throw new Fault('method',405);
  const url=new URL(request.url);
  if(request.headers.get('Origin')!==url.origin)throw new Fault('origin',403);
  requireInput(request.headers.get('content-type')?.split(';')[0]==='application/json');
  const body=await readBody(request,maxBytes);
  const day=new Date().toISOString().slice(0,10);
  const addr=request.headers.get('CF-Connecting-IP');
  if(!addr)throw new Fault('unavailable',503);
  const ip='ip:'+b64(await digest(env.TURNSTILE_SECRET+'\0'+day+'\0'+addr));
  let state;
  const first=fields(body,['kind','id','session','prompt','context','challenge']);
  if(first){
    requireInput(Object.hasOwn(tools,body.kind)&&uuid.test(body.id)&&uuid.test(body.session)
      && typeof body.prompt==='string'&&body.prompt.trim()&&body.prompt.length<=2000&&typeof body.context==='string'&&body.context.length<=30000);
    await verifyChallenge(request,env,body.challenge);
    state={kind:body.kind,prompt:body.prompt,user:'user:'+body.id,session:'session:'+body.id+':'+body.session,ip,day,
      nonce:crypto.randomUUID(),expires:Date.now()+15*60000,turn:0,messages:[
        {role:'user',content:'Open file context (data):\n'+body.context},{role:'user',content:body.prompt}]};
  }else{
    requireInput(fields(body,['continuation','results'])&&typeof body.continuation==='string'&&Array.isArray(body.results));
    state=await unseal(body.continuation,env.OPENROUTER_API_KEY);
    if(state.ip!==ip||state.day!==day||Date.now()>state.expires)throw new Fault('expired',409);
    const calls=state.messages.at(-1)?.tool_calls??[];
    requireInput(body.results.length===calls.length && calls.length>0);
    for(const [i,r] of body.results.entries()){
      requireInput(fields(r,['id','content'])&&r.id===calls[i].id&&typeof r.content==='string'&&r.content.length<=60000);
      state.messages.push({role:'tool',tool_call_id:r.id,content:r.content});
    }
  }
  if(state.turn>=8)throw new Fault('turns',422);
  const budget=env.ASSISTANT_BUDGET.getByName(day);
  for(const [i,[model,order]] of models.entries()){
    const maxTokens=2048;
    const payload=JSON.stringify({model,messages:[{role:'system',content:instructions},...state.messages],tools:tools[state.kind],
      provider:{order,allow_fallbacks:false,data_collection:'deny',max_price:{prompt:0.25,completion:1}},max_tokens:maxTokens});
    const length=encoder.encode(payload).length;
    if(length>maxBytes)throw new Fault('size',413);
    // Reserve at the maximum permitted prices, with one input token per byte plus chat framing.
    const nonce=i===0?state.nonce:crypto.randomUUID();
    const amount=Math.ceil((length+4096)/4)+maxTokens+1;
    const reserved=await budget.reserve({user:state.user,ip,session:state.session,nonce,first:first && i===0,amount,day});
    if(reserved!=='ok')throw new Fault(reserved,reserved==='expired'?409:429);
    let upstream;
    try{upstream=await fetch('https://openrouter.ai/api/v1/chat/completions',{method:'POST',
      headers:{Authorization:'Bearer '+env.OPENROUTER_API_KEY,'Content-Type':'application/json'},body:payload,signal:AbortSignal.timeout(55000)});}
    catch{if(i===0)continue;throw new Fault('unavailable',503);}
    if(!upstream.ok){
      await upstream.body?.cancel();
      if(upstream.status===429)await budget.settle(nonce,0);
      if(i===0&&(upstream.status===429||upstream.status>=500))continue;
      throw new Fault('upstream',502);
    }
    const data=await readBody(upstream,maxBytes);
    const cost=data.usage?.cost;
    if(typeof cost==='number'&&Number.isFinite(cost)&&cost>=0)await budget.settle(nonce,Math.ceil(cost*1e6));
    const message=data.choices?.[0]?.message;
    const answer=filterReply(message,state.kind,state.prompt,state.turn>0);
    if(answer.refused)await budget.refused();
    if(!answer.commands)return response(answer);
    if(++state.turn>=8)throw new Fault('turns',422);
    state.nonce=crypto.randomUUID();
    state.messages.push({role:'assistant',content:null,tool_calls:message.tool_calls,...(message.reasoning_details?{reasoning_details:message.reasoning_details}:{})});
    return response({...answer,continuation:await seal(state,env.OPENROUTER_API_KEY)});
  }
  throw new Fault('unavailable',503);
}
export async function assistant(request,env){
  try{return await processRequest(request,env);}
  catch(error){return response({error:error instanceof Fault?error.message:'unavailable'},error instanceof Fault?error.status:503);}
}
