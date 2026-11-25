import {pathToFileURL, fileURLToPath} from 'node:url';
import {existsSync, readFileSync} from 'node:fs';
const SRC=process.env.LABLY_DIR+'/src/';
export async function resolve(spec, ctx, next){
  let s=spec;
  if(s.startsWith('@/')) s=pathToFileURL(SRC+s.slice(2)).href;
  if((s.startsWith('file:')||s.startsWith('.'))){
    const base=s.startsWith('.')?new URL(s,ctx.parentURL).href:s;
    const p=fileURLToPath(base);
    for(const ext of ['','.ts','.tsx','/index.ts']) if(existsSync(p+ext)&&!(ext===''&&!/\.\w+$/.test(p))) return {url:pathToFileURL(p+ext).href,shortCircuit:true};
  }
  return next(spec,ctx);
}
export async function load(url, ctx, next){
  if(url.endsWith('.json')) return {format:'module',source:'export default '+readFileSync(fileURLToPath(url),'utf8'),shortCircuit:true};
  return next(url,ctx);
}
