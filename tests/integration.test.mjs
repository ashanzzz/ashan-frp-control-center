import test from 'node:test'
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { spawn } from 'node:child_process'
import { chmod, mkdtemp, readFile, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

function listen(server){return new Promise((resolvePromise)=>server.listen(0,'127.0.0.1',()=>resolvePromise(server.address().port)))}
function json(res,status,payload){const body=JSON.stringify(payload);res.writeHead(status,{'content-type':'application/json','content-length':Buffer.byteLength(body)});res.end(body)}
function mockProvider(){let tunnelSeq=1,dnsSeq=1;const state={containerState:'running',healthOk:true,tunnels:[{id:'t1',name:'[ashan-frp]web',node:'node-a',type:'http',localip:'127.0.0.1',nport:8080,dorp:'app.example.test',ip:'127.0.0.1'}],dns:[]};const server=createServer(async(req,res)=>{const url=new URL(req.url,'http://localhost');const chunks=[];for await(const c of req)chunks.push(c);const raw=Buffer.concat(chunks).toString();let body={};try{body=raw?JSON.parse(raw):{}}catch{}
  if(url.pathname==='/health')return json(res,state.healthOk?200:503,{ok:state.healthOk})
  if(url.pathname==='/oauth2/token')return json(res,200,{access_token:'access-token-refreshed',refresh_token:'refresh-token-refreshed',expires_in:3600,token_type:'Bearer'})
  if(url.pathname==='/login')return json(res,200,{code:200,msg:'登录成功',data:{id:42}})
  if(url.pathname==='/node')return json(res,200,{code:200,data:[{id:'n1',name:'node-a',region:'A',ip:'127.0.0.1',serverPort:server.address().port,state:'online',webSupported:true},{id:'n2',name:'node-b',region:'B',ip:'127.0.0.1',serverPort:server.address().port,state:'online',webSupported:true}]})
  if(url.pathname==='/nodeinfo')return json(res,200,{code:200,data:{state:'online',realIp:'127.0.0.1',ip:'127.0.0.1',serverPort:server.address().port}})
  if(url.pathname==='/tunnel'&&req.method==='GET')return json(res,200,{code:200,data:state.tunnels})
  if(url.pathname==='/create_tunnel'){const p=body;const t={id:`t${++tunnelSeq}`,name:p.tunnelname,node:p.node,type:p.porttype,localip:p.localip,nport:p.localport,dorp:p.banddomain||p.remoteport,remotePort:p.remoteport,ip:'127.0.0.1'};state.tunnels.push(t);return json(res,200,{code:200,data:t})}
  if(url.pathname==='/delete_tunnel'){const id=url.searchParams.get('tunnelid');state.tunnels=state.tunnels.filter(t=>t.id!==id);return json(res,200,{code:200,msg:'删除成功'})}
  if(url.pathname==='/tunnel_config'){const node=url.searchParams.get('node');return json(res,200,{code:200,data:`serverAddr = "127.0.0.1"\nserverPort = ${server.address().port}\n\n[[proxies]]\nname = "web"\ntype = "http"\nlocalIP = "127.0.0.1"\nlocalPort = 8080\ncustomDomains = ["app.example.test"]\n# node=${node}\n`})}
  if(url.pathname==='/client/v4/zones')return json(res,200,{success:true,result:[{id:'zone1',name:'example.test',status:'active'}]})
  if(url.pathname==='/client/v4/zones/zone1/dns_records'&&req.method==='GET')return json(res,200,{success:true,result:state.dns})
  if(url.pathname==='/client/v4/zones/zone1/dns_records'&&req.method==='POST'){const rec={id:`d${dnsSeq++}`,...body};state.dns.push(rec);return json(res,200,{success:true,result:rec})}
  const dnsMatch=url.pathname.match(/^\/client\/v4\/zones\/zone1\/dns_records\/(.+)$/)
  if(dnsMatch&&req.method==='PUT'){const i=state.dns.findIndex(x=>x.id===dnsMatch[1]);state.dns[i]={...state.dns[i],...body};return json(res,200,{success:true,result:state.dns[i]})}
  if(dnsMatch&&req.method==='DELETE'){state.dns=state.dns.filter(x=>x.id!==dnsMatch[1]);return json(res,200,{success:true,result:{id:dnsMatch[1]}})}
  if(url.pathname==='/graphql'){const query=String(body.query||'');if(query.includes('__schema'))return json(res,200,{data:{__schema:{mutationType:{fields:[
    {name:'startDockerContainer',description:'start docker container',args:[{name:'name',type:{kind:'NON_NULL',name:null,ofType:{kind:'SCALAR',name:'String',ofType:null}}}],type:{kind:'SCALAR',name:'Boolean',ofType:null}},
    {name:'stopDockerContainer',description:'stop docker container',args:[{name:'name',type:{kind:'NON_NULL',name:null,ofType:{kind:'SCALAR',name:'String',ofType:null}}}],type:{kind:'SCALAR',name:'Boolean',ofType:null}},
    {name:'restartDockerContainer',description:'restart docker container',args:[{name:'name',type:{kind:'NON_NULL',name:null,ofType:{kind:'SCALAR',name:'String',ofType:null}}}],type:{kind:'SCALAR',name:'Boolean',ofType:null}}
  ]}}}});if(query.includes('mutation')){state.containerState='running';const field=query.match(/\{([a-zA-Z0-9_]+)\(/)?.[1]||'restartDockerContainer';return json(res,200,{data:{[field]:true}})}return json(res,200,{data:{info:{os:{platform:'linux',distro:'Unraid',release:'7.2-test',uptime:123}},dockerContainers:[{id:'frpc-id',names:['frpc'],state:state.containerState,status:'Up 1 hour',autoStart:true,image:'snowdreamtech/frpc'}]}})}
  json(res,404,{error:'not found',path:url.pathname})
});return{server,state}}
async function waitHttp(url,timeout=10000){const end=Date.now()+timeout;while(Date.now()<end){try{const r=await fetch(url);if(r.ok)return}catch{}await new Promise(r=>setTimeout(r,100))}throw new Error('server did not start')}
async function waitJob(base,cookie,csrf,id,timeout=30000){const end=Date.now()+timeout;while(Date.now()<end){const r=await fetch(`${base}/api/v1/jobs/${id}`,{headers:{cookie}});const p=await r.json();const job=p.data;if(['succeeded','failed','canceled'].includes(job.status))return job;await new Promise(r=>setTimeout(r,200))}throw new Error(`job timeout ${id}`)}

test('complete core flow: auth, providers, reconcile, embedded frpc failover, config install and health',async(t)=>{const mock=mockProvider(),mockPort=await listen(mock.server);const appPort=mockPort+1,dataDir=await mkdtemp(join(tmpdir(),'ashan-frp-e2e-')),configPath=join(dataDir,'frpc.toml'),frpcBinary=join(dataDir,'frpc-mock.mjs'),frpcLog=join(dataDir,'frpc.log');await writeFile(frpcBinary,`#!/usr/bin/env node\nconst args=process.argv.slice(2);\nif(args.includes('--version')){console.log('0.70.1-test');process.exit(0)}\nif(args[0]==='verify'){const fs=await import('node:fs');const p=args[args.indexOf('-c')+1];const c=fs.readFileSync(p,'utf8');if(!c.includes('serverAddr')){console.error('invalid config');process.exit(1)}console.log('syntax is ok');process.exit(0)}\nconsole.log('mock frpc started');process.on('SIGTERM',()=>process.exit(0));setInterval(()=>{},1000);\n`);await chmod(frpcBinary,0o755);const root=fileURLToPath(new URL('..',import.meta.url));const child=spawn(process.execPath,['--experimental-strip-types','src/server/index.ts'],{cwd:root,env:{...process.env,DATA_DIR:dataDir,PUBLIC_DIR:join(root,'public'),FRPC_BINARY_PATH:frpcBinary,FRPC_CONFIG_PATH:configPath,FRPC_BACKUP_DIR:join(dataDir,'backups'),FRPC_LOG_PATH:frpcLog,HTTP_PORT:String(appPort),NODE_NO_WARNINGS:'1'},stdio:['ignore','pipe','pipe']});t.after(()=>{child.kill();mock.server.close()});let logs='';child.stdout.on('data',d=>logs+=d);child.stderr.on('data',d=>logs+=d);const base=`http://127.0.0.1:${appPort}`;await waitHttp(`${base}/api/v1/auth/status`);const setup=await fetch(`${base}/api/v1/auth/setup`,{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({username:'admin',password:'strong-password-123',displayName:'Admin'})});assert.equal(setup.status,200);const cookie=setup.headers.get('set-cookie').split(';')[0],session=(await setup.json()).data,csrf=session.csrfToken;const headers={'content-type':'application/json',cookie,'x-csrf-token':csrf};async function call(path,method='GET',body){const r=await fetch(`${base}/api/v1${path}`,{method,headers,body:body===undefined?undefined:JSON.stringify(body)});const p=await r.json();if(!r.ok)throw new Error(`${path} ${JSON.stringify(p)}`);return p.data}
  await call('/settings','PATCH',{'unraid.base_url':`http://127.0.0.1:${mockPort}`,'unraid.graphql_path':'/graphql','chmlfrp.base_url':`http://127.0.0.1:${mockPort}`,'chmlfrp.oauth.token_url':`http://127.0.0.1:${mockPort}/oauth2/token`,'cloudflare.api_base':`http://127.0.0.1:${mockPort}/client/v4`,'cloudflare.zone_id':'zone1','runtime.current_node':'node-a','runtime.config_path':configPath,'runtime.backup_dir':join(dataDir,'backups'),'automation.require_approval_for_high_risk':false})
  for(const [p,n,s] of [['unraid','api_key','unraid-key'],['chmlfrp','access_token','access-token'],['chmlfrp','refresh_token','refresh-token'],['chmlfrp','client_id','client-id'],['chmlfrp','client_secret','client-secret'],['cloudflare','api_token','cf-token']])await call(`/credentials/${p}/${n}`,'PUT',{secret:s})
  assert.equal((await call('/providers/unraid/test','POST',{})).dockerContainers[0].state,'running')
  assert.equal((await call('/providers/chmlfrp/test','POST',{})).code,200)
  const nodeJob=await call('/nodes/sync','POST',{});assert.equal((await waitJob(base,cookie,csrf,nodeJob.id)).status,'succeeded')
  const nodes=await call('/nodes');assert.equal(nodes.length,2)
  await call('/tunnels','POST',{name:'web',protocol:'http',localIp:'127.0.0.1',localPort:8080,domain:'app.example.test',healthUrl:`http://127.0.0.1:${mockPort}/health`})
  const syncJob=await call('/tunnels/sync','POST',{});assert.equal((await waitJob(base,cookie,csrf,syncJob.id)).status,'succeeded')
  const plan=await call('/switch-plans','POST',{targetNode:'node-b',reason:'integration-test'});assert.equal(plan.targetNode,'node-b');assert.ok(plan.tunnelPlan.actions.some(x=>x.type==='replace'))
  const executeJob=await call(`/switch-plans/${plan.id}/execute`,'POST',{});const finished=await waitJob(base,cookie,csrf,executeJob.id,45000);assert.equal(finished.status,'succeeded',`${finished.error_message||''}\n${logs}`)
  const settings=await call('/settings');assert.equal(settings['runtime.current_node'],'node-b')
  assert.equal(mock.state.tunnels.length,1);assert.equal(mock.state.tunnels[0].node,'node-b')
  assert.equal(mock.state.dns.length,1);assert.match(mock.state.dns[0].content,/node-b/)
  const config=await readFile(configPath,'utf8');assert.match(config,/node=node-b/)
  const runtime=await call('/runtime');assert.equal(runtime.mode,'embedded');assert.equal(runtime.process.state,'running');assert.match(runtime.binary.version,/0.70.1-test/)
  const runtimeLogs=await call('/runtime/logs?lines=100');assert.match(runtimeLogs.content,/mock frpc started/)
  const stopJob=await call('/runtime/action','POST',{action:'stop'});assert.equal((await waitJob(base,cookie,csrf,stopJob.id)).status,'succeeded');assert.equal((await call('/runtime')).process.state,'stopped')
  const startJob=await call('/runtime/action','POST',{action:'start'});assert.equal((await waitJob(base,cookie,csrf,startJob.id)).status,'succeeded');assert.equal((await call('/runtime')).process.state,'running')
  const health=await call('/system/health');assert.equal(health.overallStatus,'healthy')
  mock.state.healthOk=false
  const rollbackPlan=await call('/switch-plans','POST',{targetNode:'node-a',reason:'rollback-test'})
  const rollbackJob=await call(`/switch-plans/${rollbackPlan.id}/execute`,'POST',{})
  const rollbackFinished=await waitJob(base,cookie,csrf,rollbackJob.id,45000)
  assert.equal(rollbackFinished.status,'failed')
  assert.equal((await call('/settings'))['runtime.current_node'],'node-b')
  assert.equal(mock.state.tunnels.length,1);assert.equal(mock.state.tunnels[0].node,'node-b')
  assert.match(await readFile(configPath,'utf8'),/node=node-b/)
  const rollbackRows=await call('/switch-plans');assert.equal(rollbackRows.find(x=>x.id===rollbackPlan.id).status,'rolled_back')
  mock.state.healthOk=true
  const tunnelState=await call('/tunnels'),desiredTunnel=tunnelState.desired[0]
  const renamed=await call(`/tunnels/${desiredTunnel.id}`,'PUT',{...desiredTunnel,name:'web-renamed',localPort:8080,localIp:'127.0.0.1',protocol:'http',domain:'app.example.test',enabled:true})
  assert.equal(renamed.name,'web-renamed')
  const dnsCreated=await call('/dns','POST',{name:'verify.example.test',type:'TXT',content:'v1',ttl:60,enabled:true})
  const dnsUpdated=await call(`/dns/${dnsCreated.id}`,'PUT',{name:'verify.example.test',type:'TXT',content:'v2',ttl:120,enabled:true})
  assert.equal(dnsUpdated.content,'v2')
  assert.equal((await call(`/dns/${dnsCreated.id}`,'DELETE')).deleted,true)
  const credentialList=await call('/credentials');assert.equal(credentialList.some(x=>JSON.stringify(x).includes('access-token-refreshed')),false)
  const badReveal=await fetch(`${base}/api/v1/credentials/chmlfrp/access_token/reveal`,{method:'POST',headers,body:JSON.stringify({password:'wrong-password'})});assert.equal(badReveal.status,403)
  const goodReveal=await call('/credentials/chmlfrp/access_token/reveal','POST',{password:'strong-password-123'});assert.equal(goodReveal.value,'access-token')
  assert.ok(Array.isArray(await call('/providers/unraid/mutations')))
})
