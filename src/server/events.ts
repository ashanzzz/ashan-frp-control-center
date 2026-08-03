import type { ServerResponse } from 'node:http'
import { id, nowIso } from './util.ts'
const clients=new Set<ServerResponse>()
export function addSse(res:ServerResponse){ clients.add(res); res.writeHead(200,{'content-type':'text/event-stream','cache-control':'no-cache','connection':'keep-alive','x-accel-buffering':'no'}); res.write(`event: ready\ndata: ${JSON.stringify({at:nowIso()})}\n\n`); const timer=setInterval(()=>{if(!res.writableEnded)res.write(`: ping ${Date.now()}\n\n`)},25000); res.on('close',()=>{clearInterval(timer);clients.delete(res)}) }
export function emitEvent(type:string,data:unknown){ const payload=`id: ${id()}\nevent: ${type}\ndata: ${JSON.stringify({type,data,at:nowIso()})}\n\n`; for(const res of clients){try{res.write(payload)}catch{clients.delete(res)}} }
