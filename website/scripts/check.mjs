import { readFile, stat } from 'node:fs/promises';
import path from 'node:path';
import assert from 'node:assert/strict';
import {fileURLToPath} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const read=p=>readFile(path.join(root,p),'utf8');
const config=JSON.parse(await read('site.config.json'));
const manifest=JSON.parse(await read('build-manifest.json'));
const source=JSON.parse(await read('locales/en.json'));
const expected=['en','zh-Hans','zh-Hant','es','ar','fr','pt','id','ru','de','ja','vi','tr','fil','ko','th','it','pl','ms','hu'];
assert.deepEqual(manifest.languages,expected);assert.equal(manifest.pages,280);
function shape(value,ref,key='') {
 assert.equal(Array.isArray(value),Array.isArray(ref),key);
 assert.equal(typeof value,typeof ref,key);
 if(typeof ref==='string'){assert(value.trim().length>0,key);return 1;}
 assert.deepEqual(Object.keys(value).sort(),Object.keys(ref).sort(),key);
 return Object.keys(ref).reduce((sum,k)=>sum+shape(value[k],ref[k],key+'.'+k),0);
}
let translations=0,links=0;
// The demo video (and its copy) exists only for English; every other locale has the full schema without it.
const {video:_video,...shared}=source;assert(_video,'en video copy');
for(const lang of expected){const dict=JSON.parse(await read(`locales/${lang}.json`));translations+=shape(dict,lang==='en'?source:shared,lang);}
for(const asset of ['assets/video/off42-demo-en.mp4','assets/video/off42-demo-poster.jpg'])assert((await stat(path.join(root,'dist',asset))).size>1000,asset);
const known=new Set(manifest.files.map(p=>'/'+p.path.replace(/index.html$/,'')));
const canonicalSeen=new Set(),titles=new Set();
for(const page of manifest.files){
 const html=await read('dist/'+page.path);
 assert(html.startsWith('<!doctype html>'),page.path);
 assert(html.includes(`<html lang="${page.lang}" dir="${page.lang==='ar'?'rtl':'ltr'}"`),page.path);
 assert.equal((html.match(/<h1[ >]/g)||[]).length,1,page.path+' h1');
 assert(!/undefined|\[object Object\]/.test(html),page.path);
 const canonical=html.match(/<link rel="canonical" href="([^"]+)"/)[1];
 assert.equal(canonical,page.url);assert(!canonicalSeen.has(canonical));canonicalSeen.add(canonical);
 const title=html.match(/<title>(.*?)<\/title>/)[1];assert(!titles.has(title),`Duplicate page title: ${title}`);titles.add(title);
 assert.equal((html.match(/hreflang="/g)||[]).length,41,page.path+' alternate + language links');
 for(const code of expected)assert(html.includes(`rel="alternate" hreflang="${code}" href="${config.url}/${code}/${page.route}"`));
 for(const match of html.matchAll(/<script type="application\/ld\+json">(.*?)<\/script>/g))JSON.parse(match[1]);
 const types=[...html.matchAll(/<script type="application\/ld\+json">(.*?)<\/script>/g)].flatMap(m=>JSON.parse(m[1])).map(x=>x['@type']);
 if(page.route===''||/^features\/.+\//.test(page.route)){assert(types.includes('SoftwareApplication'),page.path+' SoftwareApplication');assert(types.includes('FAQPage'),page.path+' FAQPage');}
 if(/^features\/.+\//.test(page.route))assert(types.includes('BreadcrumbList'),page.path+' BreadcrumbList');
 const description=html.match(/<meta name="description" content="([^"]*)"/)[1];assert(description.length>=40&&description.length<=320,page.path+' description length '+description.length);
 assert(title.includes('OFF42'),page.path+' brand in title');assert([...title].length<=80,page.path+' title length '+title);
 assert(html.includes(`content="${config.url}/assets/og-image.jpg"`),page.path+' og:image');
 for(const set of html.matchAll(/srcset="([^"]+)"/g))for(const part of set[1].split(',')){const u=part.trim().split(/\s+/)[0];assert(u.startsWith('/assets/'),u);assert((await stat(path.join(root,'dist',u))).isFile(),page.path+' '+u);}
 const attrs=[...html.matchAll(/(?:href|src)="([^"]+)"/g)].map(x=>x[1]);
 for(const dest of attrs){
  if(dest.startsWith('https:')){const u=new URL(dest.replace(/&amp;/g,'&'));assert(['https:'].includes(u.protocol));continue;}
  assert(dest.startsWith('/')||dest.startsWith('#'),`Unsafe URL ${dest}`);
  if(dest.startsWith('#')){assert(html.includes(`id="${dest.slice(1)}"`));continue;}
  links++;
  const [url,hash]=dest.split('#');
  if(url.startsWith('/assets/'))assert((await stat(path.join(root,'dist',url))).isFile());
  else {assert(known.has(url),`Missing route ${dest} from ${page.path}`); if(hash){const target=await read('dist'+url+'index.html');assert(target.includes(`id="${hash}"`),dest);}}
 }
 if(page.route===''){const hasVideo=html.includes('<video ');assert.equal(hasVideo,page.lang==='en',page.path+' video only on the English home page');if(hasVideo)assert(html.includes('src="/assets/video/off42-demo-en.mp4"')&&html.includes('poster="/assets/video/off42-demo-poster.jpg"'));assert(html.includes('id="codes"'),page.path+' codes section');}
 if(page.route==='download/')for(const [platform,release] of Object.entries(config.releases)){
  if(release){assert(new URL(release.url).protocol==='https:');assert(release.version&&release.requirements&&release.architecture);assert(!release.sha256||/^[a-f0-9]{64}$/.test(release.sha256));assert(html.includes(release.url));}
  else assert(!html.includes(platform==='macos'?source.labels.downloadDmg:source.labels.downloadExe));
 }
}
const sitemap=await read('dist/sitemap.xml');assert.equal((sitemap.match(/<loc>/g)||[]).length,280);assert.equal((sitemap.match(/<lastmod>\d{4}-\d{2}-\d{2}<\/lastmod>/g)||[]).length,280);
for(const page of manifest.files)assert(sitemap.includes(`<loc>${page.url}</loc>`));
assert((await read('dist/robots.txt')).includes(config.url+'/sitemap.xml'));
assert((await read('dist/404.html')).includes('name="robots" content="noindex"'));
assert((await stat(path.join(root,'dist/assets/office-still-life.webp'))).size<700000);assert((await stat(path.join(root,'dist/assets/og-image.jpg'))).size<300000);
console.log(`PASS: ${manifest.pages} pages; ${expected.length} locales; ${translations} localized values; ${links} internal link/asset targets; unique titles/canonicals; language alternates; JSON-LD; sitemap; release states.`);
