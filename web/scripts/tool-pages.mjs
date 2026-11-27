// Build crawlable entry pages next to the shared application bundle.
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const toolNames = { documents: 'nav.documents', spreadsheets: 'nav.spreadsheets', presentations: 'nav.presentations',
  pdf: null, images: 'nav.images', labels: 'tab.labels', signatures: 'tab.marks' };
export const toolPath = (lang, tool) => '/app/' + lang + '/' + tool + '/';
const escape = value => String(value).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));

export function buildToolPages(dir) {
  const locales = fileURLToPath(new URL('../src/locales/', import.meta.url));
  const languages = readdirSync(locales).filter(f => f.endsWith('.json')).map(f => f.slice(0,-5));
  const template = readFileSync(join(dir,'index.html'),'utf8');
  for (const lang of languages) {
    const strings = JSON.parse(readFileSync(join(locales,lang+'.json'),'utf8'));
    const name = id => toolNames[id] ? strings[toolNames[id]] : 'PDF';
    const nav = Object.keys(toolNames).map(id => '<a href="'+toolPath(lang,id)+'">'+escape(name(id))+'</a>').join(' ');
    for (const id of Object.keys(toolNames)) {
      const description = escape(strings['landing.'+id]);
      const title = escape(name(id)) + ' – OFF42';
      const canonical = 'https://off42.cc' + toolPath(lang,id);
      const alternates = [...languages,'x-default'].map(code => '<link rel="alternate" hreflang="'+code+'" href="https://off42.cc'+toolPath(code==='x-default'?'en':code,id)+'">').join('\n');
      const head = '<title>'+title+'</title>\n<meta name="description" content="'+description+'">\n<link rel="canonical" href="'+canonical+'">\n'+alternates;
      const entry = '<main id="tool-entry"><h1>'+escape(name(id))+'</h1><p>'+description+'</p><nav aria-label="'+escape(strings['bar.menu'])+'">'+nav+'</nav></main>';
      const html = template.replace('<html lang="en">','<html lang="'+lang+'" dir="'+(lang==='ar'?'rtl':'ltr')+'">')
        .replaceAll('href="/en/','href="/'+lang+'/')
        .replace(/<title>[^<]*<\/title>/,head).replace('<body>','<body>'+entry);
      const dest = join(dir,lang,id);
      mkdirSync(dest,{recursive:true});
      writeFileSync(join(dest,'index.html'),html);
    }
  }
}
if (process.argv[2] && import.meta.url === new URL('file://' + process.argv[1]).href) buildToolPages(process.argv[2]);
