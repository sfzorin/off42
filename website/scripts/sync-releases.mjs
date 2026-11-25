// Run explicitly after publishing suite installers to GitHub Releases. Does not publish the website.
import {readFile,writeFile} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const file=path.join(root,'site.config.json');
const config=JSON.parse(await readFile(file,'utf8'));
const repo=new URL(config.repository);
if(repo.hostname!=='github.com')throw new Error('Expected an official GitHub repository URL');
const response=await fetch(`https://api.github.com/repos${repo.pathname.replace(/\/$/,'')}/releases?per_page=100`,{headers:{Accept:'application/vnd.github+json','X-GitHub-Api-Version':'2022-11-28'}});
if(!response.ok)throw new Error(`GitHub release lookup failed: ${response.status}`);
const releases=(await response.json()).filter(r=>!r.draft&&!r.prerelease).sort((a,b)=>new Date(b.published_at)-new Date(a.published_at));
let changed=false;
for(const [platform,extension] of [['macos','dmg'],['windows','exe']]){
 const matches=releases.flatMap(r=>r.assets.filter(a=>new RegExp(`^(OFF42|Suite42)[-_.].*\\.${extension}$`,'i').test(a.name)).map(a=>({r,a})));
 if(!matches.length){console.log(`${platform}: no published OFF42/Suite42 .${extension} installer; current setting preserved.`);continue;}
 const newest=matches.filter(m=>m.r.id===matches[0].r.id);
 if(newest.length!==1)throw new Error(`${platform}: multiple installers in latest release; choose architecture explicitly in site.config.json.`);
 const {r,a}=newest[0];
 const architecture=/arm64|aarch64/i.test(a.name)?'arm64':/x64|x86_64|amd64/i.test(a.name)?'x86_64':/universal/i.test(a.name)?'Universal':null;
 if(!architecture)throw new Error(`Architecture is not identified in ${a.name}; configure manually.`);
 if(!a.digest?.startsWith('sha256:'))throw new Error(`GitHub SHA-256 digest missing for ${a.name}; verify and configure manually.`);
 if(platform==='windows')throw new Error('Windows release detected. Update translated platform support copy and verify the Windows release before enabling it.');
 config.releases[platform]={url:a.browser_download_url,version:r.tag_name,architecture,requirements:'macOS 14+',sha256:a.digest.slice(7)};changed=true;
 console.log(`${platform}: selected ${a.name} (${r.tag_name})`);
}
if(changed){await writeFile(file,JSON.stringify(config,null,2)+'\n');console.log('Updated site.config.json. Review, rebuild, check and redeploy.');}
