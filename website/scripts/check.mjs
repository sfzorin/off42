import { readFile, readdir, stat } from 'node:fs/promises';
import path from 'node:path';
import assert from 'node:assert/strict';
import {fileURLToPath} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const read=p=>readFile(path.join(root,p),'utf8');
const config=JSON.parse(await read('site.config.json'));
const manifest=JSON.parse(await read('build-manifest.json'));
const source=JSON.parse(await read('locales/en.json'));
const expected=['en','zh-Hans','zh-Hant','es','ar','fr','pt','id','ru','de','ja','vi','tr','fil','ko','th','it','pl','ms','hu'];
assert.deepEqual(manifest.languages,expected);assert.equal(manifest.pages,300);
function shape(value,ref,key='') {
 assert.equal(Array.isArray(value),Array.isArray(ref),key);
 assert.equal(typeof value,typeof ref,key);
 if(typeof ref==='string'){assert(value.trim().length>0,key);return 1;}
 assert.deepEqual(Object.keys(value).sort(),Object.keys(ref).sort(),key);
 return Object.keys(ref).reduce((sum,k)=>sum+shape(value[k],ref[k],key+'.'+k),0);
}
let translations=0,links=0;
for(const lang of expected){const dict=JSON.parse(await read(`locales/${lang}.json`));translations+=shape(dict,source,lang);}
// Every language has its own demo video and poster, rendered from the apps in that interface language.
for(const lang of expected)for(const asset of [`assets/video/off42-demo-${lang}.mp4`,`assets/video/off42-demo-${lang}.jpg`])assert((await stat(path.join(root,'dist',asset))).size>1000,asset);
const known=new Set(manifest.files.map(p=>'/'+p.path.replace(/index.html$/,'')));
const canonicalSeen=new Set(),titles=new Set(),descriptions=new Set();
for(const page of manifest.files){
 const html=await read('dist/'+page.path);
 assert(html.startsWith('<!doctype html>'),page.path);
 assert(html.includes(`<html lang="${page.lang}" dir="${page.lang==='ar'?'rtl':'ltr'}"`),page.path);
 assert.equal((html.match(/<h1[ >]/g)||[]).length,1,page.path+' h1');
 if(page.route==='features/')assert.equal((html.match(/<h2>[^<]+<\/h2><p>/g)||[]).length,Object.keys(source.features).length+1,page.path+' feature card heading levels');
 const langCode=page.lang==='zh-Hans'?'简':page.lang==='zh-Hant'?'繁':page.lang.toUpperCase();
 assert(html.includes(': '+langCode+'"><svg class="i i-globe"'),page.path+' language control accessible name includes visible code');
 assert(!/undefined|\[object Object\]/.test(html),page.path);
 const canonical=html.match(/<link rel="canonical" href="([^"]+)"/)[1];
 assert.equal(canonical,page.url);assert(!canonicalSeen.has(canonical));canonicalSeen.add(canonical);
 const title=html.match(/<title>(.*?)<\/title>/)[1];assert(!titles.has(title),`Duplicate page title: ${title}`);titles.add(title);
 assert.equal((html.match(/hreflang="/g)||[]).length,41,page.path+' alternate + language links');
 for(const code of expected)assert(html.includes(`rel="alternate" hreflang="${code}" href="${config.url}/${code}/${page.route}"`));
 for(const match of html.matchAll(/<script type="application\/ld\+json">(.*?)<\/script>/g)){
  for(const item of JSON.parse(match[1]))if(item['@type']==='SoftwareApplication'){
   assert(!('downloadUrl' in item),page.path+' must not advertise a landing page as a binary download');
   assert(!('aggregateRating' in item)&&!('review' in item),page.path+' must not invent ratings or reviews');
  }
 }
 const types=[...html.matchAll(/<script type="application\/ld\+json">(.*?)<\/script>/g)].flatMap(m=>JSON.parse(m[1])).map(x=>x['@type']);
 if(page.route===''||/^features\/.+\//.test(page.route)){assert(types.includes('SoftwareApplication'),page.path+' SoftwareApplication');assert(types.includes('FAQPage'),page.path+' FAQPage');}
 if(/^features\/.+\//.test(page.route))assert(types.includes('BreadcrumbList'),page.path+' BreadcrumbList');
 const description=html.match(/<meta name="description" content="([^"]*)"/)[1];assert(description.length>=40&&description.length<=320,page.path+' description length '+description.length);
 assert(!descriptions.has(description),page.path+' duplicate description');descriptions.add(description);
 assert(title.includes('OFF42'),page.path+' brand in title');assert([...title].length<=80,page.path+' title length '+title);
 assert(html.includes(`content="${config.url}/assets/og-image.jpg"`),page.path+' og:image');
 for(const set of html.matchAll(/srcset="([^"]+)"/g))for(const part of set[1].split(',')){const u=part.trim().split(/\s+/)[0];assert(u.startsWith('/assets/'),u);assert((await stat(path.join(root,'dist',u))).isFile(),page.path+' '+u);}
 const attrs=[...html.matchAll(/(?:href|src)="([^"]+)"/g)].map(x=>x[1]);
 for(const dest of attrs){
  if(dest.startsWith('https:')){const u=new URL(dest.replace(/&amp;/g,'&'));assert(['https:'].includes(u.protocol));continue;}
  if(dest.startsWith('mailto:')){assert.equal(dest,`mailto:${config.contactEmail}`,'Only the configured contact address may be linked');continue;}
  assert(dest.startsWith('/')||dest.startsWith('#'),`Unsafe URL ${dest}`);
  if(dest.startsWith('#')){assert(html.includes(`id="${dest.slice(1)}"`));continue;}
  links++;
  const [pathAndQuery,hash]=dest.split('#');
  const url=pathAndQuery.split('?')[0];
  if(url.startsWith('/app/')){assert(url.startsWith('/app/'+page.lang+'/'),page.path+' localized tool link');assert((await stat(path.join(root,'dist',url,'index.html'))).isFile(),url);}
  else if(url.startsWith('/assets/')||/^\/(favicon\.ico|apple-touch-icon\.png|site\.webmanifest)$/.test(url))assert((await stat(path.join(root,'dist',url))).isFile(),url);
  else {assert(known.has(url),`Missing route ${dest} from ${page.path}`); if(hash){const target=await read('dist'+url+'index.html');assert(target.includes(`id="${hash}"`),dest);}}
 }
 if(page.route===''){
  assert(types.includes('VideoObject')&&types.includes('Organization'),page.path+' VideoObject and Organization');
  assert(html.includes(`<meta property="og:video" content="${config.url}/assets/video/off42-demo-${page.lang}.mp4">`),page.path+' og:video');
 }
 assert(html.includes('max-image-preview:large')&&html.includes('rel="apple-touch-icon"')&&html.includes('rel="manifest"')&&html.includes('href="/favicon.ico"'),page.path+' robots preview, icons and manifest');
 assert.equal((html.match(/og:locale:alternate/g)||[]).length,expected.length-1,page.path+' og:locale alternates');
 assert(html.includes('href="/app/'+page.lang+'/documents/"'),page.path+' localized app navigation');
 if(page.route==='')assert(html.includes('id="web-app"'),page.path+' web app section');
 if(page.route===''){assert(html.includes(`src="/assets/video/off42-demo-${page.lang}.mp4"`)&&html.includes(`poster="/assets/video/off42-demo-${page.lang}.jpg"`),page.path+' localized demo video');assert(html.includes('id="codes"'),page.path+' codes section');}
 if(page.route==='download/')for(const [platform,release] of Object.entries(config.releases)){
  if(release){assert(new URL(release.url).protocol==='https:');assert(release.version&&release.requirements&&release.architecture);assert(!release.sha256||/^[a-f0-9]{64}$/.test(release.sha256));assert(html.includes(release.url));}
  else assert(!html.includes({macos:source.labels.downloadDmg,windows:source.labels.downloadZip,linux:source.labels.downloadTarGz}[platform]));
 }
}
const sitemap=await read('dist/sitemap.xml');assert.equal((sitemap.match(/<loc>/g)||[]).length,300+7*expected.length);assert(!sitemap.includes('<lastmod>'),'Sitemap must not claim unverified modification dates');
for(const page of manifest.files){
 const entry=sitemap.match(new RegExp(`<url><loc>${page.url.replace(/[.*+?^${}()|[\]\\]/g,'\\$&')}</loc>(.*?)</url>`));
 assert(entry,page.path+' sitemap entry');
 for(const code of [...expected,'x-default']){
  const dest=code==='x-default'?'en':code;
  assert(entry[1].includes(`<xhtml:link rel="alternate" hreflang="${code}" href="${config.url}/${dest}/${page.route}"/>`),page.path+' sitemap alternate '+code);
 }
 assert.equal((entry[1].match(/<xhtml:link /g)||[]).length,21,page.path+' sitemap alternate count');
}
// Safari plays video only with 206 byte ranges; static assets answer 200, so a Worker serves /assets/video/*.
const wrangler=JSON.parse(await read('wrangler.jsonc'));
assert(wrangler.assets.run_worker_first.includes('/assets/video/*') && wrangler.assets.run_worker_first.includes('/api/assistant') && wrangler.main==='worker/index.js','video and assistant Worker routes');
assert((await read('worker/index.js')).includes("status: 206"),'video Worker answers byte ranges');
// Video sitemap: one localized entry per home page; icons and manifest are served.
assert.equal((sitemap.match(/<video:video>/g)||[]).length,expected.length,'video sitemap entries');
for(const lang of expected)assert(sitemap.includes(`<video:content_loc>${config.url}/assets/video/off42-demo-${lang}.mp4</video:content_loc>`),'video sitemap '+lang);
for(const file of ['favicon.ico','apple-touch-icon.png','site.webmanifest','assets/icon-192.png','assets/icon-512.png'])assert((await stat(path.join(root,'dist',file))).size>200,file);
JSON.parse(await read('dist/site.webmanifest'));
const headers=await read('dist/_headers');
assert(/^\/\*\n(?:  [^\n]+\n)*?  Cache-Control: public, no-transform$/m.test(headers),'global response must prohibit automatic transformations');
assert(/^\/assets\/\*\n  Cache-Control: public, max-age=86400$/m.test(headers),'retain static asset cache lifetime');
assert(headers.includes("script-src 'self'"),'retain same-origin script policy');
assert((await read('dist/robots.txt')).includes(config.url+'/sitemap.xml'));
const errorPage=await read('dist/404.html');
assert(errorPage.includes('name="robots" content="noindex"'));
assert(!/rel="canonical"|rel="alternate"|property="og:url"|application\/ld\+json/.test(errorPage),'404 must not describe the home page as its canonical or alternate');
assert((await stat(path.join(root,'dist/assets/office-still-life.webp'))).size<700000);assert((await stat(path.join(root,'dist/assets/og-image.jpg'))).size<300000);
// The web app (built from web/ and committed to public/app, because the Cloudflare build cannot compile Rust):
// its page exists, every file it references exists, the engine is served as a local .wasm file under the 25 MiB
// static asset limit, and both its own policy and the site's header policy let it compile WebAssembly.
const appDir=path.join(root,'dist/app');
const appHtml=await readFile(path.join(appDir,'index.html'),'utf8');
assert(/http-equiv="Content-Security-Policy" content="[^"]*script-src 'self' 'wasm-unsafe-eval'/.test(appHtml),'web app CSP allows wasm');
assert(/connect-src 'self'/.test(appHtml),'web app fetches only from its own origin');
const appRefs=[...appHtml.matchAll(/(?:src|href)="([^"]+)"/g)].map(m=>m[1]);
assert(appRefs.some(r=>/^\/app\/assets\/index-[\w-]+\.js$/.test(r)),'web app script is a hashed asset');
for(const ref of appRefs) assert((await stat(path.join(root,'dist',ref,ref.endsWith('/')?'index.html':''))).isFile(),'web app target '+ref);
for(const lang of expected) for(const id of ['documents','spreadsheets','presentations','pdf','images','labels','signatures']){
 const url='/app/'+lang+'/'+id+'/'; const html=await read('dist'+url+'index.html');
 assert(html.includes('rel="canonical" href="'+config.url+url+'"') && html.includes('id="tool-entry"'),url);
 assert(html.includes('<html lang="'+lang+'"') && sitemap.includes('<loc>'+config.url+url+'</loc>'),url);
 assert.equal((html.match(/rel="alternate" hreflang=/g)||[]).length,21,url);
 assert(!html.includes('undefined'),url);
}
const appAssets=await readdir(path.join(appDir,'assets'));
const wasm=appAssets.filter(f=>/^off42_wasm_bg-[\w-]+\.wasm$/.test(f));assert.equal(wasm.length,1,'one hashed wasm engine');
const entry=appRefs.find(r=>/index-[\w-]+\.js$/.test(r));assert((await readFile(path.join(root,'dist',entry),'utf8')).includes(wasm[0]),'the script loads the bundled wasm file');
for(const f of appAssets){const size=(await stat(path.join(appDir,'assets',f))).size;assert(size<25*1024*1024,`${f} is over the 25 MiB static asset limit`);}
assert.equal(appAssets.filter(f=>/^(?:zh-Hans|zh-Hant|es|ar|fr|pt|id|ru|de|ja|vi|tr|fil|ko|th|it|pl|ms|hu)-[\w-]+\.js$/.test(f)).length,expected.length-1,'web app catalogs for every site language');
assert(/^source-sha256: [0-9a-f]{64}$/m.test(await readFile(path.join(appDir,'BUILD.txt'),'utf8')),'web app BUILD.txt records its sources');
assert(/^\/\*\n(?:  [^\n]+\n)*?  Content-Security-Policy: [^\n]*script-src 'self' 'wasm-unsafe-eval'/m.test(headers),'site policy lets the web app compile its wasm');
assert(!wrangler.assets.run_worker_first.some(route=>route.startsWith('/app')),'the web app is served as static assets (MIME type from the extension)');
console.log(`PASS: ${manifest.pages} website pages, ${7*expected.length} tool pages; ${expected.length} locales; ${translations} localized values; ${links} internal link/asset targets; unique titles/canonicals; language alternates; JSON-LD; sitemap; release states; web app at /app/ (${appAssets.length} assets, wasm ${(await stat(path.join(appDir,'assets',wasm[0]))).size} bytes).`);
