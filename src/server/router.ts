import { createServer, type IncomingMessage, type ServerResponse } from 'node:http'
import { readFileSync, existsSync, statSync } from 'node:fs'
import { extname, join, normalize } from 'node:path'
import { config } from './config.ts'
import { id, safeError } from './util.ts'
import { authenticateRequest, requireCsrf } from './session.ts'

type Context = { req: IncomingMessage; res: ServerResponse; params: Record<string,string>; query: URLSearchParams; body: any; requestId: string; user?: any }
type Handler = (ctx: Context) => Promise<any> | any
const routes: Array<{ method:string; pattern:RegExp; keys:string[]; handler:Handler; auth:boolean; csrf:boolean }> = []

function compile(path:string){ const keys:string[]=[]; const pattern=path.split('/').map((part)=>{ if(part.startsWith(':')){keys.push(part.slice(1));return '([^/]+)'} return part.replace(/[.*+?^${}()|[\]\\]/g,'\\$&') }).join('/'); return { pattern:new RegExp(`^${pattern}/?$`),keys } }
export function route(method:string,path:string,handler:Handler,options:{auth?:boolean;csrf?:boolean}={}){ const {pattern,keys}=compile(path); routes.push({method:method.toUpperCase(),pattern,keys,handler,auth:options.auth!==false,csrf:options.csrf!==false}) }
export function publicRoute(method:string,path:string,handler:Handler){ route(method,path,handler,{auth:false,csrf:false}) }

function cookies(header:string|undefined):Record<string,string>{ const out:Record<string,string>={}; for(const piece of (header||'').split(';')){ const i=piece.indexOf('='); if(i>0) out[piece.slice(0,i).trim()]=decodeURIComponent(piece.slice(i+1).trim()) } return out }
export function setCookie(res:ServerResponse,name:string,value:string,maxAgeSeconds:number){ const secure=config.cookieSecure?'; Secure':''; res.setHeader('Set-Cookie',`${name}=${encodeURIComponent(value)}; Path=/; HttpOnly; SameSite=Strict; Max-Age=${maxAgeSeconds}${secure}`) }
export function clearCookie(res:ServerResponse,name:string){ res.setHeader('Set-Cookie',`${name}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0`) }
export function requestCookies(req:IncomingMessage){ return cookies(req.headers.cookie) }

async function readBody(req:IncomingMessage):Promise<any>{ if(!['POST','PUT','PATCH','DELETE'].includes(req.method||'')) return null; const chunks:Buffer[]=[]; let size=0; for await(const chunk of req){ const buf=Buffer.from(chunk); size+=buf.length; if(size>config.maxBodyBytes) throw Object.assign(new Error('请求体过大'),{code:'BODY_TOO_LARGE',status:413}); chunks.push(buf) } if(!chunks.length) return null; const text=Buffer.concat(chunks).toString('utf8'); const type=String(req.headers['content-type']||''); if(type.includes('application/json')){ try{return JSON.parse(text)}catch{throw Object.assign(new Error('JSON格式错误'),{code:'JSON_INVALID',status:400})} } return text }
function send(res:ServerResponse,status:number,payload:any,requestId:string){ const body=JSON.stringify(payload); res.writeHead(status,{'content-type':'application/json; charset=utf-8','cache-control':'no-store','x-request-id':requestId,'content-length':Buffer.byteLength(body)});res.end(body) }
const mime:Record<string,string>={'.html':'text/html; charset=utf-8','.js':'text/javascript; charset=utf-8','.css':'text/css; charset=utf-8','.svg':'image/svg+xml','.png':'image/png','.ico':'image/x-icon'}
function serveStatic(req:IncomingMessage,res:ServerResponse):boolean{ if((req.method||'GET')!=='GET') return false; const url=new URL(req.url||'/',`http://${req.headers.host||'localhost'}`); let path=decodeURIComponent(url.pathname); if(path.startsWith('/api/')) return false; if(path==='/'||!extname(path)) path='/index.html'; const file=normalize(join(config.publicDir,path)); if(!file.startsWith(config.publicDir)||!existsSync(file)||!statSync(file).isFile()){ if(path!=='/index.html'){ const index=join(config.publicDir,'index.html'); if(existsSync(index)){res.writeHead(200,{'content-type':mime['.html'],'cache-control':'no-cache'});res.end(readFileSync(index));return true} } return false } res.writeHead(200,{'content-type':mime[extname(file)]||'application/octet-stream','cache-control':path==='/index.html'?'no-cache':'public, max-age=3600'});res.end(readFileSync(file));return true }

export function startServer(){ const server=createServer(async(req,res)=>{ const requestId=id(); try{
  const url=new URL(req.url||'/',`http://${req.headers.host||'localhost'}`); const method=(req.method||'GET').toUpperCase();
  for(const item of routes){ if(item.method!==method) continue; const match=url.pathname.match(item.pattern); if(!match) continue; const params:Object=Object.fromEntries(item.keys.map((key,index)=>[key,decodeURIComponent(match[index+1])])); const body=await readBody(req); const ctx:Context={req,res,params:params as any,query:url.searchParams,body,requestId}; if(item.auth){ctx.user=authenticateRequest(req); if(!ctx.user) return send(res,401,{ok:false,error:{code:'UNAUTHORIZED',message:'请先登录'},requestId},requestId); if(item.csrf&&['POST','PUT','PATCH','DELETE'].includes(method)) requireCsrf(req,ctx.user)} const result=await item.handler(ctx); if(res.writableEnded||res.headersSent) return; return send(res,200,{ok:true,data:result,requestId},requestId) }
  if(serveStatic(req,res)) return; send(res,404,{ok:false,error:{code:'NOT_FOUND',message:'接口不存在'},requestId},requestId)
 }catch(error){ const info=safeError(error); const status=Number((error as any)?.status||500); if(!res.writableEnded) send(res,status,{ok:false,error:info,requestId},requestId) } }); server.listen(config.port,config.host,()=>console.log(`Ashan FRP Control Center listening on http://${config.host}:${config.port}`)); return server }
