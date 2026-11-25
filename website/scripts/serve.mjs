import http from 'node:http';
import {readFile,stat} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../dist');
const types={'.html':'text/html; charset=utf-8','.css':'text/css; charset=utf-8','.js':'text/javascript; charset=utf-8','.svg':'image/svg+xml','.webp':'image/webp','.xml':'application/xml','.txt':'text/plain','.jpg':'image/jpeg','.png':'image/png','.mp4':'video/mp4'};
const port=Number(process.env.PORT||4242);
http.createServer(async(req,res)=>{
 try {
  const url=new URL(req.url,'http://localhost');
  if(url.pathname==='/'){res.writeHead(302,{Location:'/en/'});res.end();return;}
  const pathname=decodeURIComponent(url.pathname);
  let target=path.resolve(root,'.'+pathname);
  if(!target.startsWith(root+path.sep)){res.writeHead(403);res.end();return;}
  const s=await stat(target);
  if(s.isDirectory()){
   if(!pathname.endsWith('/')){res.writeHead(301,{Location:url.pathname+'/'+url.search});res.end();return;}
   target=path.join(target,'index.html');
  }
  const data=await readFile(target),type=types[path.extname(target)]||'application/octet-stream';
  // Byte ranges, as the hosting serves them: Safari only plays video that answers range requests.
  const range=/^bytes=(\d*)-(\d*)$/.exec(req.headers.range||'');
  if(range&&(range[1]||range[2])){
   let start=range[1]?Number(range[1]):Math.max(0,data.length-Number(range[2])),end=range[1]&&range[2]?Math.min(Number(range[2]),data.length-1):data.length-1;
   if(start>end||start>=data.length){res.writeHead(416,{'Content-Range':`bytes */${data.length}`});res.end();return;}
   res.writeHead(206,{'Content-Type':type,'Accept-Ranges':'bytes','Content-Range':`bytes ${start}-${end}/${data.length}`,'Content-Length':end-start+1});res.end(data.subarray(start,end+1));return;
  }
  res.writeHead(200,{'Content-Type':type,'Accept-Ranges':'bytes'});res.end(data);
 }catch{res.writeHead(404,{'Content-Type':'text/html; charset=utf-8'});res.end(await readFile(path.join(root,'404.html')));}
}).listen(port,'127.0.0.1',()=>console.log(`OFF42 preview: http://127.0.0.1:${port}`));
