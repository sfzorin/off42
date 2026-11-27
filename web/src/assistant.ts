import { call } from './engine';
import { contextCommand, toolCommand, toolResult, readTools, type Command, type Selection } from './assistant-context';
import { t, language } from './i18n';
import docTools from '../../assistant/tools/document.json';
import sheetTools from '../../assistant/tools/spreadsheet.json';
import { validate } from '../../assistant/eval/lib/schema.mjs';

type Kind = 'document' | 'spreadsheet';
type Options = {
  kind: Kind;
  start: () => Promise<{ draft: number; version: number; sheet?: number; selection: Selection }>;
  commit: (commands: Command[], version: number, signal: AbortSignal) => Promise<void>;
};
type Reply = { continuation?: string; commands?: {id:string; name:string; args:Record<string,unknown>}[];
  text?:string; refused?:boolean; error?:string; sitekey?:string };
function element<K extends keyof HTMLElementTagNameMap>(tag: K, text = '') {
  const node = document.createElement(tag); node.textContent = text; return node;
}
function stored(key: string) { try { return localStorage.getItem(key); } catch { return null; } }
function store(key: string,value: string|null) { try { if(value===null)localStorage.removeItem(key);else localStorage.setItem(key,value); } catch { /* tab-only consent */ } }
const consentKey = 'off42.assistant.consent.v1';
const idKey = 'off42.assistant.id';
let tabOnlyConsent = false;
const hasConsent = () => tabOnlyConsent || stored(consentKey)==='yes';
// The Worker scopes this browser ID to its own UTC day. Read and initialize it
// under an origin-wide lock, even when two editors send their first request at once.
async function spendingId(signal: AbortSignal): Promise<string|null> {
  if (!navigator.locks) return null;
  try {
    return await navigator.locks.request(idKey, {signal}, () => {
      const existing = localStorage.getItem(idKey);
      if (existing) return existing;
      const id = crypto.randomUUID();
      localStorage.setItem(idKey, id);
      return id;
    });
  } catch {
    signal.throwIfAborted();
    return null; // The Worker uses the daily network hash when storage is unavailable.
  }
}
async function api(signal: AbortSignal, body?: object): Promise<Reply> {
  const result = await fetch('/api/assistant',{method:body?'POST':'GET',signal,cache:'no-store',
    ...(body?{headers:{'Content-Type':'application/json'}, body:JSON.stringify(body)}:{})});
  const data: Reply = await result.json();
  if(!result.ok) throw new Error(['limit','session'].includes(data.error??'')?'ai.'+data.error:'ai.error');
  return data;
}

function challenge(host: HTMLElement,key: string,signal: AbortSignal): Promise<string> {
  signal.throwIfAborted();
  return new Promise((resolve,reject)=>{
    const frame=element('iframe');
    frame.title='Cloudflare Turnstile';
    frame.src='/app/challenge/?'+new URLSearchParams({key,lang:language});
    const cleanup=()=>{window.removeEventListener('message',message);signal.removeEventListener('abort',abort);clearTimeout(timer);frame.remove();};
    const abort=()=>{cleanup();reject(new Error('ai.error'));};
    const message=(event: MessageEvent)=>{
      if(event.source!==frame.contentWindow||event.origin!==location.origin||typeof event.data?.off42Challenge!=='string')return;
      const token=event.data.off42Challenge;cleanup();
      if(token)resolve(token);else reject(new Error('ai.error'));
    };
    const timer=setTimeout(abort,120000);
    window.addEventListener('message',message);signal.addEventListener('abort',abort,{once:true});
    host.replaceChildren(frame);
  });
}

export function fileAssistant(options: Options) {
  let active: AbortController|undefined;
  let session = crypto.randomUUID();
  let messages = 0;
  const dialog=element('dialog'); dialog.className='dialog assistant-dialog';
  const heading=element('h2',t('ai.title')); dialog.setAttribute('aria-label',t('ai.title'));
  const policy=element('p',t('ai.consent'));
  const agree=element('button',t('ai.agree'));agree.className='pill-button';
  const prompt=element('textarea');prompt.placeholder=t('ai.placeholder');prompt.setAttribute('aria-label',t('ai.placeholder'));prompt.maxLength=2000;
  const send=element('button',t('ai.send'));send.className='pill-button';
  const revoke=element('button',t('ai.revoke'));revoke.className='text-button';
  const close=element('button',t('ai.stop'));close.className='text-button';
  const output=element('p');output.setAttribute('role','status');
  const widget=element('div');
  const form=element('form');form.append(prompt,send);
  const actions=element('div');actions.className='actions end';actions.append(revoke,close);
  dialog.append(heading,policy,agree,form,widget,output,actions);
  const display=()=>{const consent=hasConsent();policy.hidden=consent;agree.hidden=consent;form.hidden=!consent;revoke.hidden=!consent;};
  close.onclick=()=>dialog.close();
  dialog.addEventListener('close',()=>{active?.abort();});
  agree.onclick=()=>{store(consentKey,'yes');tabOnlyConsent=stored(consentKey)!=='yes';display();prompt.focus();};
  function resetPanel() {
    active?.abort();prompt.value='';output.textContent='';messages=0;session=crypto.randomUUID();
  }
  revoke.onclick=()=>{tabOnlyConsent=false;store(consentKey,null);resetPanel();display();};
  function syncConsent(event: StorageEvent) {
    if (event.key !== consentKey && event.key !== null) return;
    tabOnlyConsent = false;
    if (!hasConsent()) resetPanel();
    display();
  }
  window.addEventListener('storage',syncConsent);
  async function run(signal: AbortSignal) {
    const question=prompt.value.trim();
    if(!question)return;
    if(messages>=30) {messages=0;session=crypto.randomUUID();throw new Error('ai.session');}
    const {sitekey}=await api(signal);if(!sitekey)throw new Error('ai.error');
    const token=await challenge(widget,sitekey,signal);
    const {draft,version,sheet,selection}=await options.start();
    try {
      signal.throwIfAborted();
      const context=await call<string>('editApply',draft,JSON.stringify(contextCommand(selection,sheet)));
      let request: object={id:await spendingId(signal),session,kind:options.kind,context,prompt:question,challenge:token};
      messages++;
      const writes: Command[]=[];
      for(let turn=0;turn<8;turn++){
        const reply=await api(signal,request);
        if(!reply.commands){
          if(typeof reply.text!=='string')throw new Error('ai.error');
          if(!reply.refused && writes.length)await options.commit(writes,version,signal);
          signal.throwIfAborted();output.textContent=reply.text;return;
        }
        if(!reply.continuation || !reply.commands.length || reply.commands.length>24)throw new Error('ai.error');
        const results=[];
        for(const {name,args,id} of reply.commands){
          signal.throwIfAborted();
          const spec=(options.kind==='document'?docTools:sheetTools).find(x=>x.function.name===name);
          if(!spec||validate(spec.function.parameters,args).length)throw new Error('ai.error');
          const command=toolCommand(name,args,sheet);
          let content;
          let applied=false;
          try {
            content=await call<string>('editApply',draft,JSON.stringify(command));
            if(!readTools.has(name)){writes.push(command);applied=true;}
          } catch(error) {content=JSON.stringify({error:error instanceof Error?error.message:'Command failed'});}
          results.push({id,content:toolResult(content,reply.commands.length,applied)});
        }
        request={continuation:reply.continuation,results};
      }
      throw new Error('ai.error');
    } finally {await call('editClose',draft);}
  }
  form.onsubmit=async event=>{
    event.preventDefault();if(!hasConsent()||active||!prompt.value.trim())return;
    const task=new AbortController();active=task;send.disabled=true;output.textContent=t('ai.wait');
    try {await run(task.signal);}
    catch(error){if(!task.signal.aborted){const key=error instanceof Error?error.message:'';output.textContent=t(['ai.session','ai.limit','ai.stale'].includes(key)?key:'ai.error');}}
    finally {if(active===task){active=undefined;send.disabled=false;}}
  };
  return {
    open(){if(!dialog.isConnected)document.body.append(dialog);display();output.textContent='';dialog.showModal();(hasConsent()?prompt:agree).focus();},
    dispose(){active?.abort();window.removeEventListener('storage',syncConsent);dialog.remove();},
  };
}
